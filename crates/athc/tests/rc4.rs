//! **v1.0-RC4: autoridade temporal e ciclo de vida verificado do efeito.**
//!
//! Quatro contraexemplos da RC3, com processos reais (`athc`, `ath-world`, SQLite, `SIGKILL`):
//!
//! 1. **relógio local**: a validade era conferida contra o relógio do executor; atrasá-lo
//!    revalidava concessões expiradas;
//! 2. **desfecho apagado**: a âncora cobria o consumo, não o desfecho; apagar o `close` do WAL
//!    passava sem aviso e a recuperação voltava ao provedor;
//! 3. **linha forjada**: uma linha bem encadeada acrescentada ao registro era ancorada na
//!    abertura seguinte;
//! 4. **não utilização provada, concessão perdida**: uma queda com o consumo gravado e a âncora
//!    ainda atrás prova que nada saiu, e mesmo assim a concessão ficava consumida.
//!
//! Os quatro falham no commit que os introduziu, contra o programa da RC3
//! (`docs/evidence/rc4-contraexemplos-na-rc3.txt`). Aqui rodam contra `examples/rc4/treasury.ath`
//! (autoridade temporal: `ops = [..]`, `ledger = anchored`, `clock`, `outcomes`) e passam. O
//! programa da RC3 continua com a semântica da RC3 (`tests/rc3.rs`, inalterado).
//!
//! Dois serviços por teste, em bancos separados: o provedor (`bank`, que assina os recibos) e a
//! âncora (que é também a testemunha de tempo, `notary`).

use serde_json::Value as Json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const REQUEST: &str = "\"Pague 120.00 a ACME (NF 4471) e 75.50 a BETA; agende 250.00 para BETA em 2026-12-01.\"";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}
fn fx(name: &str) -> PathBuf {
    root().join("examples/rc1/fixtures").join(name)
}
fn demo_key() -> PathBuf {
    root().join("examples/rc2/fixtures/DEMO-rfc8032.key")
}
fn witness_key(name: &str) -> [u8; 32] {
    let p = root().join(format!("examples/rc4/fixtures/DEMO-{name}.key"));
    arx::sign::parse_secret(&std::fs::read_to_string(p).unwrap()).unwrap()
}
fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}
#[cfg(unix)]
fn killed(o: &Output) -> bool {
    use std::os::unix::process::ExitStatusExt;
    o.status.signal() == Some(9)
}
fn jsonl(p: &Path) -> Vec<Json> {
    std::fs::read_to_string(p)
        .map(|t| t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
        .unwrap_or_default()
}

fn prog() -> PathBuf {
    root().join("examples/rc4/treasury.ath")
}

/// A biblioteca que desloca o relógio de parede do processo (`tests/clockshim.c`).
fn clock_shim() -> PathBuf {
    let so = std::env::temp_dir().join(format!("ath-clockshim-{}.so", std::process::id()));
    if !so.exists() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/clockshim.c");
        let st = Command::new("cc").args(["-shared", "-fPIC", "-o"]).arg(&so).arg(src).arg("-ldl").status().unwrap();
        assert!(st.success(), "cc não compilou o deslocador de relógio");
    }
    so
}

/// A decisão de `Aletheia.Lifecycle.expected` para o ponto de queda, lida do fonte Lean: o
/// modelo formal é o oráculo do teste, e o Lean prova que a tabela é a função `decide`.
fn lean_decision(point: &str) -> String {
    let src = std::fs::read_to_string(root().join("lean/Aletheia/Lifecycle.lean")).unwrap();
    let table = src.split("def expected : List (Point × Decision) :=").nth(1).expect("Lifecycle.lean: `expected`");
    let table = &table[..table.find(']').expect("fim de `expected`")];
    let pairs: Vec<(String, String)> = table
        .split('(')
        .skip(1)
        .map(|p| {
            let mut it = p.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty());
            (it.next().unwrap().to_string(), it.next().unwrap().to_string())
        })
        .collect();
    assert_eq!(pairs.len(), 7, "Lifecycle.lean: `expected` tem sete pontos de queda");
    pairs.into_iter().find(|(p, _)| p == point).unwrap_or_else(|| panic!("ponto `{point}`")).1
}

struct World {
    dir: PathBuf,
    db: PathBuf,
    addr: String,
    anchor_db: PathBuf,
    anchor: String,
}

fn serve(db: &Path, key: [u8; 32]) -> String {
    let c = ath_world::open(db).unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    std::thread::spawn(move || ath_world::serve_with(l, c, Some(key)));
    addr
}

impl World {
    fn start(tag: &str) -> World {
        let dir = std::env::temp_dir().join(format!("ath-rc4-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (db, anchor_db) = (dir.join("bank.sqlite"), dir.join("anchor.sqlite"));
        let c = ath_world::open(&db).unwrap();
        let seed: Json = serde_json::from_str(&std::fs::read_to_string(fx("seed.json")).unwrap()).unwrap();
        ath_world::seed(&c, &seed).unwrap();
        drop(c);
        let (addr, anchor) = (serve(&db, witness_key("bank")), serve(&anchor_db, witness_key("notary")));
        World { dir, db, addr, anchor_db, anchor }
    }
    fn state(&self) -> Json {
        ath_world::state(&ath_world::open(&self.db).unwrap()).unwrap()
    }
    fn count(&self, k: &str) -> u64 {
        self.state()["counts"][k].as_u64().unwrap()
    }
    fn effects(&self) -> (u64, u64, u64) {
        (self.count("pays_posted"), self.count("scheduled"), self.count("receipts"))
    }
    /// Quantos registros de cada tipo o registro unificado tem.
    fn lifecycle(&self, kind: &str) -> u64 {
        ath_world::state(&ath_world::open(&self.anchor_db).unwrap()).unwrap()["lifecycle"][kind].as_u64().unwrap_or(0)
    }
    fn requests_for(&self, key: &str) -> usize {
        let c = ath_world::open(&self.db).unwrap();
        let mut st = c.prepare("SELECT line FROM log ORDER BY seq").unwrap();
        let v: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
        v.iter().filter(|l| l.contains(key)).count()
    }
    /// Uma falha na testemunha de tempo (a `nth`-ésima leitura).
    fn time_fault(&self, nth: i64, kind: &str, ms: i64) {
        ath_world::add_fault(&ath_world::open(&self.anchor_db).unwrap(), "time", Some(nth), kind, ms).unwrap();
    }
    fn fault(&self, tool: &str, kind: &str, ms: i64) {
        ath_world::add_fault(&ath_world::open(&self.db).unwrap(), tool, None, kind, ms).unwrap();
    }
    /// O escopo (e a configuração de confiança) aberto na âncora.
    fn scope(&self) -> (String, arx::lifecycle::Meta) {
        let c = ath_world::open(&self.anchor_db).unwrap();
        let (s, m): (String, String) =
            c.query_row("SELECT scope, meta FROM lc_scopes", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        (s, arx::lifecycle::Meta::from_json(&serde_json::from_str(&m).unwrap()).unwrap())
    }
    fn p(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
    fn athc(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_athc"));
        c.env("NO_COLOR", "1");
        c
    }
    fn ok(&self, c: &mut Command) -> Output {
        let o = c.output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
        o
    }
}

/// Uma execução com as três fontes assinadas pela autoridade para o programa, a execução e um
/// prazo.
struct Run<'w> {
    w: &'w World,
    run: String,
    grants: PathBuf,
    trusted: PathBuf,
    args_sig: PathBuf,
    models: &'static str,
}

impl<'w> Run<'w> {
    fn signed(w: &'w World, run: &str, intent: &str, issued: u64, ttl: u64) -> Run<'w> {
        let r = Run {
            w,
            run: run.into(),
            grants: w.p(&format!("{run}.grants.json")),
            trusted: w.p(&format!("{run}.trusted.json")),
            args_sig: w.p(&format!("{run}.args.sig")),
            models: if intent == "intent-wire.json" { "models-wire.json" } else { "models-legit.json" },
        };
        let stamp = |c: &mut Command, issued: u64, ttl: u64| {
            c.arg("--program").arg(prog()).args(["--issued", &issued.to_string(), "--ttl", &ttl.to_string()]);
        };
        let _ = std::fs::remove_file(&r.grants);
        let mut c = w.athc();
        c.args(["grant", "--run", run, "--origin", "confirmation:teste-rc4", "--out"]).arg(&r.grants);
        c.arg("--from").arg(fx(intent)).arg("--sign").arg(demo_key());
        stamp(&mut c, issued, ttl);
        w.ok(&mut c);
        // a fotografia e o pedido valem dez minutos: os testes de prazo mexem só nas concessões
        std::fs::copy(fx("trusted.json"), &r.trusted).unwrap();
        let mut c = w.athc();
        c.args(["sign", "--key"]).arg(demo_key()).arg("--trusted").arg(&r.trusted).args(["--run", run]);
        stamp(&mut c, now(), 600);
        w.ok(&mut c);
        let mut c = w.athc();
        c.args(["sign", "--key"]).arg(demo_key()).args(["--run", run, "--arg", REQUEST, "--out"]).arg(&r.args_sig);
        stamp(&mut c, now(), 600);
        w.ok(&mut c);
        r
    }
    fn fresh(w: &'w World, run: &str) -> Run<'w> {
        Run::signed(w, run, "intent.json", now(), 600)
    }
    fn wal(&self, wal: &str) -> PathBuf {
        self.w.p(&format!("{wal}.wal"))
    }
    fn cmd(&self, wal: &str) -> Command {
        let mut c = self.w.athc();
        c.arg("run").arg(prog()).args(["--provider", "scripted", "--models"]).arg(fx(self.models));
        c.args(["--host", &format!("tcp://{}", self.w.addr), "--trusted"]).arg(&self.trusted);
        c.args(["--run-id", &self.run, "--timeout", "5", "--grants"]).arg(&self.grants);
        c.arg("--wal").arg(self.wal(wal));
        c.arg("--journal").arg(self.w.p(&format!("{wal}.journal.jsonl")));
        c.arg("--args-sig").arg(&self.args_sig).args(["--arg", REQUEST]);
        c.args(["--anchor", &format!("tcp://{}", self.w.anchor), "--clock", &format!("tcp://{}", self.w.anchor)]);
        c
    }
    fn recover(&self, wal: &str) -> Output {
        let mut c = self.w.athc();
        c.arg("recover").arg(prog()).arg("--wal").arg(self.wal(wal));
        c.args(["--host", &format!("tcp://{}", self.w.addr), "--timeout", "5"]);
        c.output().unwrap()
    }
    /// A execução é recusada citando `code` e `why`, sem efeito no provedor e sem autorização de
    /// envio no registro unificado.
    fn refused(&self, c: &mut Command, code: &str, why: &str) -> Output {
        let o = self.blocked(c, code, why);
        assert!(!o.status.success(), "esperado {code}: {}", text(&o));
        o
    }

    /// Como `refused`, quando o erro acontece dentro da saga do programa: o tratador `on failure`
    /// o captura e a execução termina normalmente, com o código no resultado.
    fn blocked(&self, c: &mut Command, code: &str, why: &str) -> Output {
        let before = self.w.state();
        let o = c.output().unwrap();
        assert!(text(&o).contains(code) && text(&o).contains(why), "esperado {code} ({why}): {}", text(&o));
        assert_eq!(self.w.state()["counts"], before["counts"], "{code}: o provedor aplicou um efeito");
        assert_eq!(self.w.lifecycle("send"), 0, "{code}: houve autorização de envio");
        o
    }
}

/// O desfecho da chave no WAL depois da recuperação: (estado, evidência).
fn outcome(wal: &[Json], key: &str) -> Option<(String, String)> {
    wal.iter().rev().find(|e| (e["kind"] == "close" || e["kind"] == "escalate") && e["detail"]["key"] == key).map(|e| {
        let d = &e["detail"];
        if e["kind"] == "escalate" {
            ("escalated".to_string(), d["reason"].as_str().unwrap_or_default().to_string())
        } else {
            (d["status"].as_str().unwrap().to_string(), d["evidence"].as_str().unwrap().to_string())
        }
    })
}

// ---------------------------------------------------------------- o ciclo de vida

/// **Todo efeito autorizado tem a história inteira ancorada**: a concessão assinada para este
/// programa, a reserva com o tempo atestado, a autorização de envio revalidada, e o desfecho com
/// o recibo assinado pelo provedor para aquela chave, aquela ferramenta e aqueles argumentos. O
/// WAL cita o desfecho ancorado, e o executor reaceita o registro inteiro do zero.
#[test]
fn every_authorized_effect_has_an_anchored_lifecycle() {
    let w = World::start("lifecycle");
    let r = Run::fresh(&w, "r");
    w.ok(&mut r.cmd("r"));
    assert_eq!(w.effects(), (2, 1, 1));
    assert_eq!((w.lifecycle("reserve"), w.lifecycle("send"), w.lifecycle("outcome")), (3, 3, 3));
    let (scope, meta) = w.scope();
    assert_eq!(scope, meta.scope(), "o escopo é o hash da configuração de confiança");
    let mut reg = arx::lifecycle::TcpRegistry::new(&w.anchor, 5.0).unwrap();
    use arx::lifecycle::Registry;
    let chain = reg.read(&scope).unwrap();
    let st = arx::lifecycle::verify_chain(&meta, &chain).unwrap();
    let wal = jsonl(&r.wal("r"));
    let bank = arx::sign::parse_public(&meta.outcomes).unwrap();
    for key in ["k1", "k2", "k3"] {
        let op = &st.ops[key];
        let (h, receipt) = op.outcome.as_ref().unwrap();
        assert!(receipt.verify(&bank) && receipt.wire == format!("r:{key}") && receipt.args == op.args_hash, "{key}");
        let close = wal.iter().find(|e| e["kind"] == "close" && e["detail"]["key"] == key).unwrap();
        assert_eq!(close["detail"]["outcome"], h.as_str(), "{key}: o WAL cita o desfecho ancorado");
        let intent = wal.iter().find(|e| e["kind"] == "intent" && e["detail"]["key"] == key).unwrap();
        assert_eq!(intent["hash"], op.intent.as_str(), "{key}: a reserva cita a intenção do WAL");
    }
    assert_eq!(st.taken.len(), 3, "as três concessões estão consumidas no registro ancorado");
}

/// **Sem o protocolo, nada.** Um programa temporal sem a âncora ou sem a testemunha de tempo
/// não executa (`R0639`).
#[test]
fn without_the_protocol_a_temporal_program_does_nothing() {
    let w = World::start("noprotocol");
    let r = Run::fresh(&w, "r");
    for drop in ["--anchor", "--clock"] {
        let args: Vec<String> = r.cmd(&drop[2..]).get_args().map(|a| a.to_string_lossy().to_string()).collect();
        let i = args.iter().position(|a| a == drop).unwrap();
        let mut c = w.athc();
        c.args(args.iter().enumerate().filter(|(j, _)| *j != i && *j != i + 1).map(|(_, a)| a));
        r.refused(&mut c, "R0639", "protocolo RC4");
    }
}

// ---------------------------------------------------------------- Missão I: tempo

/// **Contraexemplo 1, fechado: o relógio local não decide.** As concessões foram assinadas há
/// duas horas, com dez minutos de validade. Com o relógio do processo atrasado duas horas, a
/// execução continua recusada: o prazo é conferido no tempo atestado pela testemunha. E com o
/// relógio adiantado um dia, concessões frescas continuam valendo.
#[cfg(unix)]
#[test]
fn a_rewound_local_clock_does_not_revalidate_an_expired_grant() {
    let w = World::start("clock");
    let r = Run::signed(&w, "r", "intent.json", now() - 7200, 600);
    for offset in ["0", "-7200"] {
        let mut c = r.cmd(&format!("o{offset}"));
        c.env("LD_PRELOAD", clock_shim()).env("ATH_FAKE_CLOCK_OFFSET", offset);
        r.refused(&mut c, "G0001", "expirou");
    }
    assert_eq!(w.lifecycle("reserve"), 0);
    let f = Run::fresh(&w, "f");
    let mut c = f.cmd("adiantado");
    c.env("LD_PRELOAD", clock_shim()).env("ATH_FAKE_CLOCK_OFFSET", "86400");
    w.ok(&mut c);
    assert_eq!(w.effects(), (2, 1, 1), "o relógio adiantado não invalidou concessões frescas");
}

/// **A testemunha de tempo é verificada.** Uma resposta repetida (`T0001`), um instante que
/// volta (`T0002`), uma resposta atrasada além da tolerância (`T0003`), um instante congelado
/// enquanto o relógio monotônico anda (`T0004`) e a testemunha fora do ar (`T0005`): nenhum
/// efeito, nenhuma autorização de envio.
#[test]
fn the_time_witness_is_verified() {
    let cases: Vec<(&str, i64, &str, i64, &str, bool)> = vec![
        ("replay", 2, "replay", 0, "T0001", false),
        ("rewind", 3, "skew", -100, "T0002", false),
        ("late", 3, "hang", 3000, "T0003", false),
        ("frozen", 3, "freeze", 0, "T0004", true),
    ];
    for (tag, nth, kind, ms, code, slow) in cases {
        let w = World::start(&format!("time-{tag}"));
        let r = Run::fresh(&w, "r");
        w.time_fault(nth, kind, ms);
        if slow {
            // o relógio monotônico anda 4 s entre a leitura do início e a da fronteira
            w.fault("statement", "hang", 4000);
        }
        r.blocked(&mut r.cmd("r"), code, "");
    }
    let w = World::start("time-down");
    let r = Run::fresh(&w, "r");
    let mut c = r.cmd("r");
    c.args(["--clock", "tcp://127.0.0.1:1"]);
    r.refused(&mut c, "T0005", "testemunha");
}

/// **Uma execução que atravessa o prazo para na fronteira.** (a) As concessões valem cinco
/// segundos e a leitura do extrato leva quatro: no primeiro efeito o tempo atestado mostra a
/// concessão expirada (com a tolerância de 2 s): `R0638`, nada reservado. (b) O prazo passa
/// entre a reserva e a autorização de envio (a testemunha adianta 1000 s na quarta leitura): a
/// reserva é liberada (nada saiu) e a chamada é recusada.
#[test]
fn an_execution_that_crosses_the_deadline_stops_at_the_boundary() {
    let w = World::start("deadline-a");
    let r = Run::signed(&w, "r", "intent.json", now(), 5);
    w.fault("statement", "hang", 4000);
    let o = r.cmd("r").output().unwrap();
    // o contrato é avaliado no `refine`, antes de qualquer efeito: a concessão já aparece
    // expirada no tempo atestado (a última leitura mais o relógio monotônico desde ela)
    let j = jsonl(&w.p("r.journal.jsonl"));
    let refused =
        j.iter().find(|e| e["kind"] == "grants").map(|e| e["detail"]["refused"].to_string()).unwrap_or_default();
    assert!(refused.contains("expirou"), "{refused}\n{}", text(&o));
    assert_eq!((w.effects(), w.lifecycle("reserve")), ((0, 0, 0), 0), "{}", text(&o));

    let w = World::start("deadline-b");
    let r = Run::fresh(&w, "r");
    w.time_fault(4, "skew", 1000);
    let o = r.cmd("r").output().unwrap();
    assert!(text(&o).contains("R0638") && text(&o).contains("entre a reserva e o envio"), "{}", text(&o));
    assert_eq!(w.effects(), (0, 0, 0));
    assert_eq!((w.lifecycle("reserve"), w.lifecycle("release"), w.lifecycle("send")), (1, 1, 0));
    let close = jsonl(&r.wal("r")).into_iter().find(|e| e["kind"] == "close").unwrap();
    assert_eq!(close["detail"]["evidence"], "released:R0638");
}

/// **A recuperação aplica a mesma disciplina.** `SIGKILL` depois de enviar o primeiro pagamento
/// (`idempotent`), sem desfecho; a recuperação só acontece depois do prazo. Reenviar poderia ser
/// o primeiro envio que chega, com a concessão expirada: a operação fica escalada (`R0638`), o
/// provedor não recebe nada novo, e a saga continua aberta.
#[cfg(unix)]
#[test]
fn recovery_does_not_resend_with_an_expired_authorization() {
    let w = World::start("recover-expired");
    let r = Run::signed(&w, "r", "intent.json", now(), 6);
    let o = r.cmd("r").args(["--kill-at", "sent:1"]).output().unwrap();
    assert!(killed(&o), "{}", text(&o));
    let before = w.requests_for("call pay r:k1");
    std::thread::sleep(std::time::Duration::from_secs(7));
    let rec = r.recover("r");
    assert_eq!(rec.status.code(), Some(3), "{}", text(&rec));
    // (a compensação da saga, `refund` com a mesma chave, não é autorizada por concessão: ela
    // desfaz; o que não pode sair é um novo `pay`)
    assert_eq!(w.requests_for("call pay r:k1"), before, "a recuperação reenviou com a concessão expirada");
    let (status, why) = outcome(&jsonl(&r.wal("r")), "k1").unwrap();
    assert!(status == "escalated" && why.contains("R0638"), "{status}: {why}");
}

// ---------------------------------------------------------------- Missão II: ciclo de vida

/// **Contraexemplo 2, fechado: um desfecho confirmado não some.** O WAL perde o `close` do
/// agendamento, ou tudo depois do registro `run`. A recuperação restaura cada desfecho do
/// registro ancorado, citando o mesmo recibo, sem consultar o provedor.
#[test]
fn a_confirmed_outcome_does_not_vanish_with_the_wal() {
    for cut in ["k3", "all"] {
        let w = World::start(&format!("outcome-{cut}"));
        let r = Run::fresh(&w, "r");
        w.ok(&mut r.cmd("r"));
        let real = jsonl(&r.wal("r"));
        let pos = match cut {
            "k3" => real.iter().position(|e| e["kind"] == "close" && e["detail"]["key"] == "k3").unwrap(),
            _ => 1,
        };
        let j = arx::journal::Journal::from_entries(real[..pos].to_vec());
        std::fs::write(r.wal("r"), j.to_jsonl()).unwrap();
        let before = (w.requests_for("r:k1"), w.requests_for("r:k2"), w.requests_for("r:k3"));
        let rec = r.recover("r");
        assert_eq!(rec.status.code(), Some(0), "{cut}: {}", text(&rec));
        assert_eq!(
            (w.requests_for("r:k1"), w.requests_for("r:k2"), w.requests_for("r:k3")),
            before,
            "{cut}: o provedor foi consultado"
        );
        let wal = jsonl(&r.wal("r"));
        for key in if cut == "k3" { vec!["k3"] } else { vec!["k1", "k2", "k3"] } {
            let orig = real.iter().find(|e| e["kind"] == "close" && e["detail"]["key"] == key).unwrap();
            let back = wal.iter().rev().find(|e| e["kind"] == "close" && e["detail"]["key"] == key).unwrap();
            assert_eq!(back["detail"]["evidence"], "anchor:outcome", "{cut}/{key}");
            assert_eq!(back["detail"]["outcome"], orig["detail"]["outcome"], "{cut}/{key}: o mesmo recibo");
        }
        assert_eq!(w.effects(), (2, 1, 1), "{cut}");
    }
}

/// **Um desfecho local adulterado ou fabricado é recusado** (`W0008`): o `close` do agendamento
/// trocado para "falhou", ou um `close` que cita um desfecho que a âncora não tem. A recuperação
/// para antes de qualquer envio e não escreve no WAL.
#[test]
fn a_tampered_wal_outcome_is_refused() {
    let w = World::start("tampered");
    let r = Run::fresh(&w, "r");
    w.ok(&mut r.cmd("r"));
    let real = jsonl(&r.wal("r"));
    let pos = real.iter().position(|e| e["kind"] == "close" && e["detail"]["key"] == "k3").unwrap();
    let orig = real[pos]["detail"].clone();
    let mut swapped = orig.clone();
    swapped["status"] = serde_json::json!("failed");
    let mut fabricated = orig.clone();
    fabricated["outcome"] = serde_json::json!("e".repeat(64));
    for (tag, forged) in [("status", swapped), ("hash", fabricated)] {
        let mut j = arx::journal::Journal::from_entries(real[..pos].to_vec());
        j.push(0.0, "close", forged);
        std::fs::write(r.wal("r"), j.to_jsonl()).unwrap();
        let before = std::fs::read(r.wal("r")).unwrap();
        let rec = r.recover("r");
        assert_eq!(rec.status.code(), Some(2), "{tag}: {}", text(&rec));
        assert!(text(&rec).contains("W0008"), "{tag}: {}", text(&rec));
        assert_eq!(std::fs::read(r.wal("r")).unwrap(), before, "{tag}: a recuperação escreveu");
        assert_eq!(w.requests_for("status schedule"), 0, "{tag}");
    }
}

/// **Contraexemplo 3, fechado: a âncora não aceita o que a evidência não sustenta.** Registros
/// com hashes e encadeamento corretos, mandados direto ao serviço da âncora: um desfecho com o
/// recibo assinado por outra chave, um desfecho com o recibo de outra operação, uma reserva com
/// uma concessão assinada por quem não é a autoridade, uma reserva com um tempo atestado
/// reaproveitado de outra posição do registro, e uma liberação depois do envio. Todos
/// recusados; o registro não cresce; o executor reaceita o registro inteiro.
#[test]
fn a_fabricated_record_is_never_anchored() {
    use arx::lifecycle::{args_hash, AppendError, Receipt, Registry};
    let w = World::start("fabricated");
    let r = Run::fresh(&w, "r");
    let o = r.cmd("r").args(["--kill-at", "sent:3"]).output().unwrap();
    #[cfg(unix)]
    assert!(killed(&o), "{}", text(&o));
    let (scope, meta) = w.scope();
    let mut reg = arx::lifecycle::TcpRegistry::new(&w.anchor, 5.0).unwrap();
    let chain = reg.read(&scope).unwrap();
    let st = arx::lifecycle::verify_chain(&meta, &chain).unwrap();
    let n = chain.len() as u64;
    let k3 = &st.ops["k3"];
    let auth = arx::sign::parse_secret(&std::fs::read_to_string(demo_key()).unwrap()).unwrap();
    let wrong_signer = Receipt::sign(&auth, "r:k3", "schedule", &k3.args_hash, "done", "schedule #1");
    let other_op = Receipt::sign(&witness_key("bank"), "r:k9", "schedule", &k3.args_hash, "done", "schedule #1");
    let mut g =
        arx::grant::Grant::new("r", "pay", &["MALLORY".to_string(), "9000.00".to_string()], "confirmation:x", 0);
    let other = arx::sign::parse_secret(&"42".repeat(32)).unwrap();
    let stamp = arx::sign::Stamp::sign(&other, "grant2", &g.id, &meta.program, "r", now(), now() + 60);
    (g.program, g.issued, g.expires, g.sig) =
        (Some(stamp.program), Some(stamp.issued), Some(stamp.expires), Some(stamp.sig));
    let old_time = chain.iter().find(|e| e.kind == "send").unwrap().body["time"].clone();
    let args = vec![serde_json::json!("MALLORY"), serde_json::json!("9000.00")];
    let reserve = |grant: Json, time: Json| {
        serde_json::json!({ "key": "k9", "tool": "pay", "args": args, "args_hash": args_hash(&args), "intent": "i",
                            "grants": [grant], "time": time })
    };
    let real_grant = k3.grants[0].to_json();
    let cases: Vec<(&str, &str, Json, &str)> = vec![
        (
            "recibo de outra chave",
            "outcome",
            serde_json::json!({ "key": "k3", "receipt": wrong_signer.to_json() }),
            "FABRICADO",
        ),
        (
            "recibo de outra operação",
            "outcome",
            serde_json::json!({ "key": "k3", "receipt": other_op.to_json() }),
            "outra operação",
        ),
        ("concessão de quem não é a autoridade", "reserve", reserve(g.to_json(), old_time.clone()), ""),
        ("tempo reaproveitado", "reserve", reserve(real_grant, old_time), ""),
        ("liberação depois do envio", "release", serde_json::json!({ "key": "k3" }), "pode ter saído"),
    ];
    for (tag, kind, body, why) in cases {
        match reg.append(&scope, n, kind, body) {
            Err(AppendError::Refused(m)) => assert!(m.contains(why), "{tag}: {m}"),
            other => panic!("{tag}: a âncora aceitou: {other:?}"),
        }
    }
    let after = reg.read(&scope).unwrap();
    assert_eq!(after.len() as u64, n, "o registro cresceu");
    arx::lifecycle::verify_chain(&meta, &after).unwrap();
    // e a recuperação resolve o agendamento pela consulta, com o recibo verdadeiro
    let rec = r.recover("r");
    assert_eq!(rec.status.code(), Some(0), "{}", text(&rec));
    assert_eq!(outcome(&jsonl(&r.wal("r")), "k3"), Some(("done".into(), "recovery:status".into())));
}

/// **Contraexemplo 4, fechado: a não utilização provada libera a concessão.** `SIGKILL` com a
/// reserva ancorada e sem autorização de envio: nada saiu. A recuperação libera a reserva na
/// âncora sem consultar o provedor. No agendamento (a terceira operação), e no primeiro
/// pagamento: aí uma nova execução, com as mesmas concessões, faz tudo o que foi confirmado,
/// uma vez (com chaves novas: uma chave liberada nunca é reaproveitada).
#[cfg(unix)]
#[test]
fn a_provably_unsent_operation_releases_its_grant() {
    for n in [3u32, 1] {
        let w = World::start(&format!("release-{n}"));
        let r = Run::fresh(&w, "r");
        let o = r.cmd("first").args(["--kill-at", &format!("reserved:{n}")]).output().unwrap();
        assert!(killed(&o), "{n}: {}", text(&o));
        let key = format!("r:k{n}");
        assert_eq!(w.requests_for(&key), 0, "{n}");
        let rec = r.recover("first");
        assert_eq!(rec.status.code(), Some(0), "{n}: {}", text(&rec));
        assert_eq!(
            w.requests_for(&key),
            0,
            "{n}: a recuperação consultou o provedor sobre o que provadamente não saiu"
        );
        assert_eq!(
            outcome(&jsonl(&r.wal("first")), &format!("k{n}")),
            Some(("failed".into(), "anchor:unsent".into())),
            "{n}"
        );
        assert_eq!(w.lifecycle("release"), 1, "{n}");
        if n == 1 {
            let o = r.cmd("second").output().unwrap();
            assert_eq!(w.effects(), (2, 1, 1), "a concessão liberada não voltou a valer\n{}", text(&o));
            assert_eq!(w.lifecycle("reserve"), 4, "a reserva liberada mais as três da nova execução");
        }
    }
}

/// **A tabela do Lean é o oráculo da recuperação.** `SIGKILL` em cada um dos sete pontos da
/// fronteira do agendamento (`queryable`); a decisão do `athc recover`, num processo novo, é a
/// que `Aletheia.Lifecycle.expected` dá para o ponto (e que o Lean prova ser `decide`).
#[cfg(unix)]
#[test]
fn recovery_decides_as_the_lean_table() {
    for point in ["authorized", "logged", "reserved", "intent", "sent", "confirmed", "closed"] {
        let w = World::start(&format!("table-{point}"));
        let r = Run::fresh(&w, "r");
        let o = r.cmd("r").args(["--kill-at", &format!("{point}:3")]).output().unwrap();
        assert!(killed(&o), "{point}: {}", text(&o));
        let status_before = w.requests_for("status schedule");
        let rec = r.recover("r");
        assert_eq!(rec.status.code(), Some(0), "{point}: {}", text(&rec));
        let wal = jsonl(&r.wal("r"));
        let got = outcome(&wal, "k3");
        let queried = w.requests_for("status schedule") > status_before;
        match lean_decision(point).as_str() {
            "unreserved" => {
                let expect = (point == "logged").then(|| ("failed".to_string(), "anchor:unreserved".to_string()));
                assert_eq!(got, expect, "{point}");
                assert!(!queried && w.requests_for("r:k3") == 0, "{point}");
            }
            "release" => {
                assert_eq!(got, Some(("failed".into(), "anchor:unsent".into())), "{point}");
                assert!(!queried && w.requests_for("r:k3") == 0, "{point}");
            }
            "byClass" => {
                let applied = if point == "sent" { "done" } else { "failed" };
                assert_eq!(got, Some((applied.into(), "recovery:status".into())), "{point}");
                assert!(queried, "{point}: o provedor consultável não foi consultado");
            }
            "restore" => {
                let e = got.unwrap();
                assert_eq!(e.0, "done", "{point}");
                assert_eq!(e.1, if point == "confirmed" { "anchor:outcome" } else { "reply" }, "{point}");
                assert!(!queried, "{point}: consultou o provedor por um desfecho ancorado");
            }
            d => panic!("decisão desconhecida: {d}"),
        }
        let sched = w.count("scheduled");
        assert!(sched <= 1, "{point}: {sched} agendamentos");
    }
}

/// **O limite do provedor opaco é preservado, não resolvido.** A remessa (`opaque`) com
/// autorização de envio e sem desfecho fica escalada: não é reenviada e não é dada como feita
/// nem como não feita. A mesma remessa interrompida antes da autorização de envio é liberada:
/// ali a ausência de `send` no registro ancorado prova que nada saiu.
#[cfg(unix)]
#[test]
fn an_opaque_effect_after_possible_send_stays_unknown() {
    for (point, code, expect) in [("sent", 3, "escalated"), ("reserved", 0, "failed")] {
        let w = World::start(&format!("opaque-{point}"));
        let r = Run::signed(&w, "r", "intent-wire.json", now(), 600);
        let o = r.cmd("r").args(["--kill-at", &format!("{point}:2")]).output().unwrap();
        assert!(killed(&o), "{point}: {}", text(&o));
        let wires = w.count("wires");
        let rec = r.recover("r");
        assert_eq!(rec.status.code(), Some(code), "{point}: {}", text(&rec));
        let (status, why) = outcome(&jsonl(&r.wal("r")), "k2").unwrap();
        assert_eq!(status, expect, "{point}: {why}");
        if point == "sent" {
            assert!(why.contains("opaco"), "{why}");
        }
        assert_eq!(w.count("wires"), wires, "{point}: a recuperação reenviou a remessa");
    }
}

/// **A corrida, no registro unificado.** Quatro processos da mesma execução, com as mesmas
/// concessões e WALs diferentes: o compare-and-swap da âncora deixa uma reserva por concessão;
/// o provedor termina no estado de uma execução; os perdedores são recusados (`R0632`).
#[test]
fn concurrent_executors_share_one_anchored_lifecycle() {
    let w = World::start("race");
    let r = Run::fresh(&w, "r");
    for _ in 0..4 {
        w.fault("statement", "hang", 700);
    }
    let procs: Vec<_> = (0..4)
        .map(|i| r.cmd(&format!("p{i}")).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap())
        .collect();
    let all: String =
        procs.into_iter().map(|p| text(&p.wait_with_output().unwrap())).collect::<Vec<_>>().join("\n---\n");
    assert_eq!((w.lifecycle("reserve"), w.lifecycle("send"), w.lifecycle("outcome")), (3, 3, 3), "{all}");
    assert_eq!(w.effects(), (2, 1, 1), "{all}");
    assert!(all.matches("R0632").count() >= 3, "{all}");
}

// ---------------------------------------------------------------- Missão III: a linguagem

/// **A garantia de cada autoridade é explícita.** `athc check` diz o nível de cada uma; com
/// `--strict-authority`, as formas anteriores (só identidade, textual) são recusadas (`E0430`) e
/// a temporal passa. A compatibilidade não se confunde com a garantia mais forte.
#[test]
fn weaker_authorities_are_identified_explicitly() {
    let w = World::start("levels");
    for (p, level) in [("rc2", "identidade (RC2)"), ("rc3", "textual (RC3)"), ("rc4", "temporal (RC4)")] {
        let path = root().join(format!("examples/{p}/treasury.ath"));
        let o = w.athc().arg("check").arg(&path).output().unwrap();
        assert!(o.status.success() && text(&o).contains(&format!("garantia {level}")), "{p}: {}", text(&o));
        let o = w.athc().args(["check", "--strict-authority"]).arg(&path).output().unwrap();
        assert_eq!(o.status.success(), p == "rc4", "{p}: {}", text(&o));
        assert_eq!(text(&o).contains("E0430"), p != "rc4", "{p}");
    }
}

/// **A autoridade tipada é resolvida e canônica.** Operação inexistente (`E0427`), repetida
/// (`E0428`), testemunha não declarada ou sem tolerância (`E0429`), escopo que não cobre um
/// contrato (`E0427`), forma mista (erro de sintaxe). E a ordem das operações não muda a
/// identidade do programa; o conteúdo, sim.
#[test]
fn the_typed_authority_is_resolved_and_canonical() {
    let w = World::start("typed");
    let src = std::fs::read_to_string(prog()).unwrap();
    let check = |s: &str| {
        let p = w.p("m.ath");
        std::fs::write(&p, s).unwrap();
        w.athc().arg("check").arg(&p).output().unwrap()
    };
    let root_of = |s: &str| -> String {
        let o = check(s);
        assert!(o.status.success(), "{}", text(&o));
        let t = text(&o);
        let i = t.find("raiz #").unwrap();
        t[i + 6..i + 18].to_string()
    };
    for (mutate, code) in [
        ("ops = [pay, schedule, wire, pya]", "E0427"),
        ("ops = [pay, schedule, pay, wire]", "E0428"),
        ("ops = [pay, schedule]", "E0427"),
    ] {
        let o = check(&src.replace("ops = [pay, schedule, wire]", mutate));
        assert!(!o.status.success() && text(&o).contains(code), "{mutate}: {}", text(&o));
    }
    let o = check(&src.replace("clock = notary", "clock = notario"));
    assert!(text(&o).contains("E0429"), "{}", text(&o));
    let o = check(&src.replace("  tolerance = 2\n", ""));
    assert!(text(&o).contains("E0429") && text(&o).contains("tolerance"), "{}", text(&o));
    let o = check(&src.replace("ledger = anchored", "ledger = \"anchored\""));
    assert!(!o.status.success() && text(&o).contains("mistura"), "{}", text(&o));
    let base = root_of(&src);
    assert_eq!(root_of(&src.replace("ops = [pay, schedule, wire]", "ops = [wire, pay, schedule]")), base);
    assert_ne!(root_of(&src.replace("max_age = 900", "max_age = 901")), base);
    assert_ne!(root_of(&src.replace("tolerance = 2", "tolerance = 3")), base);
}
