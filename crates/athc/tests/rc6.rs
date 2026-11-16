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
        if sub == "run" {
            c.args(["--run-id", "n"]);
        }
        c
    }

    /// As notas de um tópico, como o provedor as guardou: (id, texto).
    fn notes(&self, topic: &str) -> Vec<(i64, String)> {
        let c = ath_world::open(&self.db).unwrap();
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
