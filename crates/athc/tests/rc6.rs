//! **v1.0-RC6: o contrato por rota.** Uma API real não conhece `Idempotency-Key`, põe
//! parâmetros no caminho e devolve listas. O `bind` declara, por ferramenta, onde a chave viaja
//! (`.key`: um marcador num parâmetro de texto) e como achar a operação por ela (`.find`).
//!
//! Aqui, sem rede: o recurso `notes` do `ath-world` cria uma nota a cada `POST`, sem
//! deduplicar. A mesma construção é exercitada contra a API do GitHub por
//! `scripts/rc6-github.sh`.

use serde_json::Value as Json;
use std::path::PathBuf;
use std::process::{Command, Output};

const NOTES: &str = r#"module rc6.notes

tool add_note(c: Cap[Notes.Write], topic: Text[..40], text: Text[..200], k: own IdemKey) -> Int[1..1000000]
  ! { Notes.Write } irrevocable provider queryable latency 5.0

tool note_ids(c: Cap[Notes.Write], topic: Text[..40]) -> List[Int[1..1000000], 0..50] ^ Untrusted
  ! { Notes.Write } reversible latency 5.0

bind notes {
  url           = env("NOTES_URL")
  add_note      = "POST /v1/notes/{topic} -> id"
  add_note.key  = "text"
  add_note.find = "GET /v1/notes/{topic} -> id"
  note_ids      = "GET /v1/notes/{topic} -> [].id"
}

agent Scribe(notes: Cap[Notes.Write], keys: Cap[IdemKey.Mint])
  budget [tok <= 10, usd <= 0.01, wall <= 60.0]
{
  on write(topic: Text[..40] ^ Trusted) -> Text ! { Notes.Write, TimeoutUnknown } {
    commit add_note(notes, topic, "primeira", keys.mint())
    commit add_note(notes, topic, "segunda", keys.mint())
    let ids = note_ids(notes, topic)
    "duas notas"
  }

  handle timeout_unknown(p: own Pending[_]) -> Text {
    reconcile p fuel 2 else {
      transfer p
      "pendente"
    }
  }
}
"#;

struct World {
    dir: PathBuf,
    db: PathBuf,
    api: String,
}

impl World {
    fn start(tag: &str) -> World {
        let dir = std::env::temp_dir().join(format!("ath-rc6-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("world.sqlite");
        let c = ath_world::open(&db).unwrap();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let api = l.local_addr().unwrap().to_string();
        std::thread::spawn(move || ath_world::serve_http(l, c, None, None));
        std::fs::write(dir.join("notes.ath"), NOTES).unwrap();
        World { dir, db, api }
    }

    fn athc(&self, sub: &str, topic: &str) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_athc"));
        c.env("NO_COLOR", "1").env("NOTES_URL", format!("http://{}", self.api));
        c.arg(sub).arg(self.dir.join("notes.ath")).args(["--timeout", "5", "--arg", &format!("{topic:?}")]);
        c.arg("--wal").arg(self.dir.join("n.wal")).arg("--journal").arg(self.dir.join(format!("{sub}.jsonl")));
        if sub != "resume" {
            c.args(["--run-id", "n"]);
        }
        c
    }

    /// As notas de um tópico, como o provedor as guardou: (id, texto).
    fn notes(&self, topic: &str) -> Vec<(i64, String)> {
        let c = ath_world::open(&self.db).unwrap();
        // (direto do banco: tudo o que foi criado, visível ou não na listagem)
        let mut st = c.prepare("SELECT id, text FROM notes WHERE topic = ?1 ORDER BY id").unwrap();
        st.query_map([topic], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|x| x.unwrap()).collect()
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[cfg(unix)]
fn killed(o: &Output) -> bool {
    use std::os::unix::process::ExitStatusExt;
    o.status.signal() == Some(9)
}

/// Cada texto aparece uma só vez, com o marcador da chave que o criou.
fn exactly_two(w: &World, topic: &str, what: &str) {
    let notes = w.notes(topic);
    let of = |t: &str| notes.iter().filter(|(_, x)| x.starts_with(t)).count();
    assert_eq!((notes.len(), of("primeira"), of("segunda")), (2, 1, 1), "{what}: {notes:?}");
    assert!(notes.iter().all(|(_, x)| x.contains("<!-- aletheia:n:k")), "{what}: sem marcador: {notes:?}");
}

/// **A ligação a um recurso que não conhece chaves.** O tópico vai no caminho, a chave viaja
/// como marcador no texto, o resultado é o `id` (um `Int` conferido), e a lista de ids volta
/// por projeção (`[].id`). Um tópico com `/` e `?` é UM segmento do caminho: não cria rota.
#[test]
fn a_tool_is_bound_to_a_resource_that_knows_no_keys() {
    let w = World::start("plain");
    let o = w.athc("run", "obra").output().unwrap();
    assert!(o.status.success() && text(&o).contains("duas notas"), "{}", text(&o));
    exactly_two(&w, "obra", "execução");
    let j = std::fs::read_to_string(w.dir.join("run.jsonl")).unwrap();
    let ids: Vec<i64> = w.notes("obra").iter().map(|(i, _)| *i).collect();
    assert!(j.contains(&format!("\"result\":{}", serde_json::json!(ids))), "a lista projetada não chegou: {j}");

    let w = World::start("path");
    let o = w.athc("run", "a/b?x=1&y").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    exactly_two(&w, "a/b?x=1&y", "tópico com / e ?");
}

/// **Queda e retomada sem duplicar, num provedor que não deduplica.** Morto depois de enviar
/// (a nota existe, a resposta não foi gravada): a consulta por chave a acha pelo marcador, e a
/// retomada não a reenvia. Morto antes de enviar: a consulta prova que não existe, e a
/// retomada a cria com uma chave nova. Em todo ponto de queda de cada operação, ficam
/// exatamente duas notas.
#[cfg(unix)]
#[test]
fn a_killed_agent_resumes_without_duplicating_on_a_provider_that_ignores_keys() {
    for point in ["authorized", "intent", "sent", "confirmed", "closed"] {
        for n in 1..=2 {
            let tag = format!("{point}-{n}");
            let w = World::start(&tag);
            let o = w.athc("run", "obra").args(["--kill-at", &format!("{point}:{n}")]).output().unwrap();
            assert!(killed(&o), "{tag}: {}", text(&o));
            let o = w.athc("resume", "obra").output().unwrap();
            assert!(o.status.success() && text(&o).contains("duas notas"), "{tag}: {}", text(&o));
            exactly_two(&w, "obra", &tag);
        }
    }
}

/// **Ausência numa listagem atrasada não é prova de ausência.** Num tópico `lento-…` o provedor
/// só lista a nota 2 s depois de criá-la (como a listagem de issues do GitHub, que foi onde
/// isto apareceu). O agente é morto depois de enviar a primeira nota. Sem `.settle`, a consulta
/// imediata não a acha, conclui "não aplicada" e a retomada a cria de novo: três notas. Com
/// `.settle`, a consulta espera e consulta de novo antes de concluir: duas.
#[cfg(unix)]
#[test]
fn absence_in_a_lagging_listing_is_not_taken_as_not_applied() {
    let run = |tag: &str, settle: &str| {
        let w = World::start(tag);
        let find = "  add_note.find = \"GET /v1/notes/{topic} -> id\"\n";
        std::fs::write(w.dir.join("notes.ath"), NOTES.replace(find, &format!("{find}{settle}"))).unwrap();
        let o = w.athc("run", "lento-obra").args(["--kill-at", "sent:1"]).output().unwrap();
        assert!(killed(&o), "{tag}: {}", text(&o));
        let o = w.athc("resume", "lento-obra").output().unwrap();
        assert!(o.status.success(), "{tag}: {}", text(&o));
        std::thread::sleep(std::time::Duration::from_millis(2200));
        w.notes("lento-obra").iter().filter(|(_, x)| x.starts_with("primeira")).count()
    };
    assert_eq!(run("lag-none", ""), 2, "sem `.settle` a listagem atrasada duplica (o defeito que `.settle` fecha)");
    assert_eq!(run("lag-settle", "  add_note.settle = \"3\"\n"), 1, "com `.settle` a operação é achada");
}

/// **O contrato da rota é conferido antes de rodar.** Parâmetro de caminho que a ferramenta
/// não tem (`E0435`); marcador num parâmetro que não é texto do corpo, e consulta sem marcador
/// (`E0436`). E o caminho só aceita o que é caminho: o host nunca vem de um parâmetro.
#[test]
fn a_route_contract_is_checked_before_anything_runs() {
    let w = World::start("static");
    let check = |tag: &str, from: &str, to: &str| {
        assert!(NOTES.contains(from), "{tag}: `{from}`");
        let p = w.dir.join(format!("{tag}.ath"));
        std::fs::write(&p, NOTES.replace(from, to)).unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_athc")).env("NO_COLOR", "1").arg("check").arg(&p).output().unwrap();
        assert!(!o.status.success(), "{tag}: aceito: {}", text(&o));
        text(&o)
    };
    assert!(check("param", "POST /v1/notes/{topic} -> id", "POST /v1/notes/{tpic} -> id").contains("E0435"));
    assert!(check("keycap", "add_note.key  = \"text\"", "add_note.key  = \"topic\"").contains("E0436"));
    assert!(check("nokey", "  add_note.key  = \"text\"\n", "").contains("E0436"));
    assert!(check(
        "post",
        "add_note.find = \"GET /v1/notes/{topic} -> id\"",
        "add_note.find = \"POST /v1/notes/{topic} -> id\""
    )
    .contains("E0436"));
    assert!(
        check("host", "POST /v1/notes/{topic} -> id", "POST //{topic}/v1/notes -> id").contains("rota inválida")
            || check("host2", "POST /v1/notes/{topic} -> id", "POST {topic}/v1/notes -> id").contains("rota inválida")
    );
    use ath_syntax::ast::Route;
    for bad in ["GET http://evil/x", "GET /a/{b}c", "GET /a?x=y z", "GET /a -> [].", "DELETE /a", "GET /a/{b/c}"] {
        assert!(Route::parse(bad).is_none(), "{bad}");
    }
    let r = Route::parse("GET /repos/o/r/issues/{issue}/comments?per_page=100 -> [].id").unwrap();
    assert_eq!(r.path_params(), vec!["issue"]);
    let _: Json = serde_json::json!(null);
}

// ---------------------------------------------------------------- o supervisor

impl World {
    fn supervise(&self, topic: &str) -> Command {
        let mut c = self.athc("supervise", topic);
        c.args(["--backoff", "0.05"]);
        c
    }

    fn attempts(&self) -> Vec<Json> {
        std::fs::read_to_string(self.dir.join("n.wal.supervisor.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

/// **O agente termina sem ninguém olhando.** Um só comando: o executor é morto três vezes
/// (depois de enviar, antes de enviar, depois de confirmar) e o supervisor o retoma a cada vez.
/// Termina com exatamente duas notas, num provedor que não deduplica; cada tentativa está no
/// registro do supervisor.
#[cfg(unix)]
#[test]
fn a_supervised_agent_finishes_through_three_crashes() {
    let w = World::start("sup");
    let o = w.supervise("obra").args(["--inject-kills", "sent:1,intent:1,confirmed:1"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert!(
        text(&o).contains("duas notas") && text(&o).contains("concluído em 4 tentativa(s), 3 retomada(s)"),
        "{}",
        text(&o)
    );
    exactly_two(&w, "obra", "supervisionado");
    let a = w.attempts();
    let got: Vec<(&str, &str)> =
        a.iter().map(|e| (e["mode"].as_str().unwrap(), e["outcome"].as_str().unwrap())).collect();
    assert_eq!(got, [("run", "signal:9"), ("resume", "signal:9"), ("resume", "signal:9"), ("resume", "exit:0")]);
}

/// **O supervisor tem limite, e pode ele mesmo cair.** Com mais quedas que `--max-resumes`,
/// desiste (saída 5) em vez de tentar para sempre, e o que está em aberto fica no WAL. Um
/// supervisor novo, sobre o mesmo WAL, começa pela retomada e termina.
#[cfg(unix)]
#[test]
fn the_supervisor_gives_up_at_its_limit_and_can_be_restarted() {
    let w = World::start("sup-limit");
    let o =
        w.supervise("obra").args(["--inject-kills", "sent:1,sent:1,sent:1", "--max-resumes", "1"]).output().unwrap();
    assert_eq!(o.status.code(), Some(5), "{}", text(&o));
    assert_eq!(w.attempts().iter().filter(|e| e["mode"].is_string()).count(), 2, "{:?}", w.attempts());
    let o = w.supervise("obra").output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert!(text(&o).contains("tentativa 1 (resume)"), "{}", text(&o));
    exactly_two(&w, "obra", "supervisor reiniciado");
}

/// **Resultado desconhecido não vira sucesso.** A API aceita o pedido (`202`) e não diz se o
/// efeito aconteceu; não há consulta por chave. O programa transfere a obrigação e termina; o
/// supervisor sai com 3 e nomeia as chaves. Nenhuma retomada é tentada: não foi uma queda.
#[test]
fn an_unknown_result_is_reported_not_retried_and_not_called_success() {
    let w = World::start("sup-unknown");
    let lines = "  add_note      = \"POST /v1/notes/{topic} -> id\"\n  add_note.key  = \"text\"\n  add_note.find = \"GET /v1/notes/{topic} -> id\"\n";
    assert!(NOTES.contains(lines));
    std::fs::write(w.dir.join("notes.ath"), NOTES.replace(lines, "  add_note      = \"POST /v1/accepted -> id\"\n"))
        .unwrap();
    let o = w.supervise("obra").output().unwrap();
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(text(&o).contains("RESULTADO DESCONHECIDO") && text(&o).contains("k1"), "{}", text(&o));
    assert_eq!(w.attempts().len(), 1);
}

/// **Falha não é queda.** O provedor está fora do ar: o programa termina com erro (nada foi
/// enviado). O supervisor não retoma o que o programa já decidiu (saída 1, uma tentativa).
#[test]
fn a_program_failure_is_not_a_crash() {
    let w = World::start("sup-fail");
    let o = w.supervise("obra").env("NOTES_URL", "http://127.0.0.1:9").output().unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert_eq!(w.attempts().len(), 1);
    assert!(w.notes("obra").is_empty());
}

/// **Um executor travado é uma queda.** O provedor aceita a conexão e nunca responde. Cada
/// tentativa passa do prazo, é morta e retomada; no limite, o supervisor desiste (saída 5), em
/// tempo limitado, sem nota duplicada.
#[cfg(unix)]
#[test]
fn a_hung_executor_is_killed_and_the_supervisor_stays_bounded() {
    let w = World::start("sup-hang");
    let hole = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = hole.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for s in hole.incoming().flatten() {
            held.push(s);
        }
    });
    let t0 = std::time::Instant::now();
    let o = w
        .supervise("obra")
        .env("NOTES_URL", format!("http://{addr}"))
        .args(["--timeout", "60", "--attempt-timeout", "0.5", "--max-resumes", "2"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(5), "{}", text(&o));
    assert!(t0.elapsed().as_secs_f64() < 15.0, "o supervisor não ficou limitado: {:?}", t0.elapsed());
    let a = w.attempts();
    let outcomes: Vec<&str> =
        a.iter().filter_map(|e| e["mode"].as_str().map(|_| e["outcome"].as_str().unwrap())).collect();
    assert_eq!(outcomes, ["timeout", "timeout", "timeout"], "{}", text(&o));
}
