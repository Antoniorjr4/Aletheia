//! **v1.0-RC5: o agente inteiro na linguagem — retomada verificada e fronteira nativa.**
//!
//! Dois limites da RC4, reproduzidos como testes que falham nela
//! (`docs/evidence/rc5-limites-na-rc4.txt`):
//!
//! 1. **a queda mata o agente**: `athc recover` fecha as obrigações em aberto, mas o programa
//!    não continua; o que faltava executar nunca acontece (limite declarado em `docs/RC4.md`);
//! 2. **o tipo do resultado não é conferido na fronteira**: o que o provedor devolve entra no
//!    programa com o tipo declarado da ferramenta, seja o que for.
//!
//! O programa da tesouraria é o da RC4 (`examples/rc4/treasury.ath`), sem alteração.
#![allow(dead_code)]

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
        let dir = std::env::temp_dir().join(format!("ath-rc5-{}-{tag}", std::process::id()));
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
        c.args(["grant", "--run", run, "--origin", "confirmation:teste-rc5", "--out"]).arg(&r.grants);
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

impl<'w> Run<'w> {
    /// `athc resume`: o mesmo programa, o mesmo WAL, as mesmas fontes assinadas.
    fn resume(&self, wal: &str) -> Command {
        let mut c = self.w.athc();
        c.arg("resume").arg(prog()).args(["--provider", "scripted", "--models"]).arg(fx(self.models));
        c.args(["--host", &format!("tcp://{}", self.w.addr), "--trusted"]).arg(&self.trusted);
        c.args(["--timeout", "5", "--grants"]).arg(&self.grants);
        c.arg("--wal").arg(self.wal(wal));
        c.arg("--journal").arg(self.w.p(&format!("{wal}.resume.jsonl")));
        c.arg("--args-sig").arg(&self.args_sig).args(["--arg", REQUEST]);
        c.args(["--anchor", &format!("tcp://{}", self.w.anchor), "--clock", &format!("tcp://{}", self.w.anchor)]);
        c
    }
}

const ANSWER: &str = "resultado: \"Paguei 120.00 a ACME e 75.50 a BETA; agendei 250";

/// O estado do provedor que o programa produz sem queda: a referência de toda retomada.
fn reference(tag: &str) -> Json {
    let w = World::start(tag);
    let r = Run::fresh(&w, "r");
    let o = w.ok(&mut r.cmd("r"));
    assert!(text(&o).contains(ANSWER), "{}", text(&o));
    let s = w.state();
    serde_json::json!({ "accounts": s["accounts"], "counts": s["counts"] })
}

// ---------------------------------------------------------------- Missão I: a retomada

/// **Limite 1, fechado: o agente morto retoma e termina, exatamente uma vez.** O processo é
/// morto (`SIGKILL`) em cada ponto de queda de cada operação com chave. `athc resume`, num
/// processo novo, leva o programa ao fim: o estado do provedor é o da execução sem queda,
/// cada operação foi aplicada uma só vez, e a resposta é a mesma.
#[cfg(unix)]
#[test]
fn a_killed_agent_resumes_and_finishes_exactly_once() {
    let want = reference("resume-ref");
    for point in ["authorized", "reserved", "intent", "sent", "confirmed", "closed"] {
        for n in 1..=4u32 {
            let tag = format!("resume-{point}-{n}");
            let w = World::start(&tag);
            let r = Run::fresh(&w, "r");
            let o = r.cmd("r").args(["--kill-at", &format!("{point}:{n}")]).output().unwrap();
            assert!(killed(&o), "{tag}: {}", text(&o));
            let o = r.resume("r").output().unwrap();
            assert!(o.status.success() && text(&o).contains(ANSWER), "{tag}: {}", text(&o));
            let s = w.state();
            assert_eq!(s["accounts"], want["accounts"], "{tag}: saldos");
            assert_eq!(s["counts"], want["counts"], "{tag}: efeitos");
            assert_eq!(s["conserved"], serde_json::json!(true), "{tag}");
        }
    }
}

// ---------------------------------------------------------------- Missão II: a fronteira

/// Um provedor que responde a toda chamada com `value` (o protocolo de linha do `ath-world`).
fn lying_provider(value: Json) -> String {
    use std::io::{BufRead, BufReader, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut line = String::new();
            let _ = BufReader::new(&s).read_line(&mut line);
            let _ = (&s).write_all(format!("{}\n", serde_json::json!({ "ok": true, "value": value })).as_bytes());
        }
    });
    addr
}

const PROBE: &str = r#"module rc5.probe

tool balance(c: Cap[Bank.Read], account: Text[..16]) -> Text[..24] ^ Untrusted
  ! { Bank.Read } reversible latency 5.0

agent Probe(bank: Cap[Bank.Read])
  budget [tok <= 10, usd <= 0.01, wall <= 30.0]
{
  on ask(account: Text[..16] ^ Trusted) -> Text ! { Bank.Read } {
    let b = balance(bank, account)
    "saldo lido"
  }
}
"#;

/// **Limite 2, fechado: o provedor não entrega ao programa um valor fora do tipo.** A
/// ferramenta declara `Text[..24]`; o provedor devolve 5000 caracteres, ou um objeto. O valor
/// não entra no programa: a execução para com a culpa atribuída ao provedor (`R0640`).
#[test]
fn a_provider_cannot_hand_the_program_an_ill_typed_value() {
    let dir = std::env::temp_dir().join(format!("ath-rc5-{}-probe", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let prog = dir.join("probe.ath");
    std::fs::write(&prog, PROBE).unwrap();
    let run = |value: Json| {
        let addr = lying_provider(value);
        let mut c = Command::new(env!("CARGO_BIN_EXE_athc"));
        c.env("NO_COLOR", "1").arg("run").arg(&prog).args(["--host", &format!("tcp://{addr}"), "--timeout", "5"]);
        c.args(["--arg", "\"TREASURY\""]).arg("--journal").arg(dir.join("j.jsonl"));
        c.output().unwrap()
    };
    // dentro do tipo: executa
    let o = run(serde_json::json!("1000.00"));
    assert!(o.status.success() && text(&o).contains("saldo lido"), "{}", text(&o));
    let bads = [
        ("5000 caracteres", serde_json::json!("9".repeat(5000))),
        ("objeto", serde_json::json!({ "saldo": 1 })),
        ("inteiro", serde_json::json!(7)),
    ];
    for (what, bad) in bads {
        let o = run(bad);
        let shown: String = text(&o).chars().take(600).collect();
        assert!(!o.status.success() && text(&o).contains("R0640"), "{what}: aceito sem conferência: {shown}");
        assert!(!text(&o).contains("saldo lido"), "o valor fora do tipo entrou no programa: {}", text(&o));
    }
}
