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
//! O programa da tesouraria é o da RC4 (`examples/rc4/treasury.ath`), sem alteração. Os testes
//! da ligação nativa usam `examples/rc5/treasury.ath`: o mesmo programa, com as ferramentas
//! ligadas por `bind` à API REST do provedor (`ath-world serve --http`), sem código de cola.
#![allow(dead_code)]

use serde_json::Value as Json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

thread_local! {
    /// o programa dos testes desta thread (o da RC4, ou o da RC5 com `bind`)
    static PROG: std::cell::RefCell<&'static str> = const { std::cell::RefCell::new("examples/rc4/treasury.ath") };
}

fn prog() -> PathBuf {
    root().join(PROG.with(|p| *p.borrow()))
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
            // (o ponto `reserved` só existe nas operações autorizadas por concessão: sem ele a
            // execução termina, e retomar uma execução terminada não pode fazer nada)
            assert!(killed(&o) || (point == "reserved" && o.status.success()), "{tag}: {}", text(&o));
            let o = r.resume("r").output().unwrap();
            if (point, n) == ("intent", 3) {
                // o agendamento (`queryable`, com concessão) caiu entre a autorização de envio
                // e o envio: o provedor responde que não aplicou e a autorização daquela
                // tentativa está consumida. A retomada não a recria: a chamada falha, com os
                // pagamentos (já confirmados) no lugar e nada duplicado.
                assert!(!o.status.success() && text(&o).contains("R0600"), "{tag}: {}", text(&o));
                assert_eq!((w.count("pays_posted"), w.count("scheduled"), w.count("receipts")), (2, 0, 0), "{tag}");
                continue;
            }
            assert!(o.status.success() && text(&o).contains(ANSWER), "{tag}: {}", text(&o));
            let s = w.state();
            assert_eq!(s["accounts"], want["accounts"], "{tag}: saldos");
            assert_eq!(s["counts"], want["counts"], "{tag}: efeitos");
        }
    }
}

/// O que a retomada informa: (entradas vindas da fita, chamadas ao vivo).
fn resumed(o: &Output) -> (u32, u32) {
    let t = text(o);
    let l = t.lines().find(|l| l.contains("retomada:") && l.contains("ao vivo")).unwrap_or_else(|| panic!("{t}"));
    let n: Vec<u32> = l.split(|c: char| !c.is_ascii_digit()).filter_map(|x| x.parse().ok()).collect();
    (n[0], n[1])
}

fn report(p: &Path) -> Json {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// **A retomada sobrevive a outra queda.** O processo que retoma também é morto, duas vezes,
/// em pontos diferentes; a terceira retomada termina. O estado é o da execução sem queda.
#[cfg(unix)]
#[test]
fn resume_survives_being_killed_again() {
    let want = reference("again-ref");
    let w = World::start("again");
    let r = Run::fresh(&w, "r");
    let o = r.cmd("r").args(["--kill-at", "sent:1"]).output().unwrap();
    assert!(killed(&o), "{}", text(&o));
    // na retomada, a primeira operação ao vivo é o segundo pagamento; depois, o agendamento
    for point in ["intent:1", "confirmed:1"] {
        let o = r.resume("r").args(["--kill-at", point]).output().unwrap();
        assert!(killed(&o), "{point}: {}", text(&o));
    }
    let o = r.resume("r").output().unwrap();
    assert!(o.status.success() && text(&o).contains(ANSWER), "{}", text(&o));
    let s = w.state();
    assert_eq!((&s["accounts"], &s["counts"]), (&want["accounts"], &want["counts"]));
    // e retomar a execução terminada não chama ninguém: tudo vem da fita
    let calls = w.requests_for("call ");
    let o = r.resume("r").output().unwrap();
    assert!(o.status.success() && text(&o).contains(ANSWER), "{}", text(&o));
    assert_eq!(resumed(&o).1, 0, "{}", text(&o));
    assert_eq!(w.requests_for("call "), calls, "a retomada de uma execução terminada chamou o provedor");
}

/// **O orçamento atravessa a queda.** O que a execução original gastou (tokens, dólares) é
/// reposto no medidor pela re-execução: o total da execução retomada é o da execução sem
/// queda, e o modelo não é chamado de novo.
#[cfg(unix)]
#[test]
fn the_budget_and_the_model_calls_survive_the_crash() {
    let w0 = World::start("budget-ref");
    let r0 = Run::fresh(&w0, "r");
    w0.ok(r0.cmd("r").arg("--report").arg(w0.p("ref.json")));
    let want = report(&w0.p("ref.json"));
    let w = World::start("budget");
    let r = Run::fresh(&w, "r");
    let o = r.cmd("r").args(["--kill-at", "sent:3"]).output().unwrap();
    assert!(killed(&o), "{}", text(&o));
    // a retomada usa um modelo que responderia outra coisa: ele não é consultado
    let mut c = r.resume("r");
    let o = w.ok(c.arg("--models").arg(fx("models-wire.json")).arg("--report").arg(w.p("res.json")));
    let got = report(&w.p("res.json"));
    assert_eq!(got["spent"]["tok"], want["spent"]["tok"], "{}", text(&o));
    assert_eq!(got["spent"]["usd"], want["spent"]["usd"]);
    assert_eq!(got["result"], want["result"]);
    assert_eq!(w.count("scheduled"), 1);
    assert_eq!(w.count("wires"), 0, "o modelo da retomada foi consultado e a remessa saiu");
}

/// **A retomada recusa outra história.** Outros argumentos ou outra fotografia confiável não
/// são a execução original (`R0652`); uma execução que `athc recover` encerrou, compensando a
/// saga, não é retomável (`R0653`). Em todos os casos o provedor não recebe nada.
#[cfg(unix)]
#[test]
fn resume_refuses_what_is_not_the_original_execution() {
    let w = World::start("refuse");
    let r = Run::fresh(&w, "r");
    let o = r.cmd("r").args(["--kill-at", "sent:2"]).output().unwrap();
    assert!(killed(&o), "{}", text(&o));
    // outra fotografia confiável (assinada pela mesma autoridade, para a mesma execução)
    let other = w.p("other.trusted.json");
    std::fs::write(&other, r#"{ "inventories": { "payees": ["ACME"] } }"#).unwrap();
    let mut c = w.athc();
    c.args(["sign", "--key"]).arg(demo_key()).arg("--trusted").arg(&other).args(["--run", "r"]);
    c.arg("--program").arg(prog()).args(["--issued", &now().to_string(), "--ttl", "600"]);
    w.ok(&mut c);
    let calls = w.requests_for("call ");
    let mut c = r.resume("r");
    let o = c.arg("--trusted").arg(&other).output().unwrap();
    assert!(!o.status.success() && text(&o).contains("R0652"), "{}", text(&o));
    // (a fase 1 fecha a obrigação em aberto do segundo pagamento pelo protocolo; mais nada)
    assert!(w.requests_for("call ") <= calls + 1);
    assert_eq!(w.count("scheduled"), 0);
    // `athc recover` encerra a execução: a saga interrompida é compensada
    let rec = r.recover("r");
    assert!(rec.status.code().is_some(), "{}", text(&rec));
    let o = r.resume("r").output().unwrap();
    assert!(!o.status.success() && text(&o).contains("R0653"), "{}", text(&o));
    assert_eq!(w.count("scheduled"), 0);
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

// ---------------------------------------------------------------- Missão II: `bind`

const TOKEN: &str = "tok-5f1c9a7e2b4d4c8f9a6e-NAO-E-UM-SEGREDO-REAL";

/// A face HTTP do provedor de `w`, sobre o mesmo banco, exigindo `TOKEN`.
fn http(w: &World) -> String {
    let c = ath_world::open(&w.db).unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    std::thread::spawn(move || ath_world::serve_http(l, c, Some(witness_key("bank")), Some(TOKEN.to_string())));
    addr
}

impl<'w> Run<'w> {
    /// `athc run|resume` do programa ligado: sem `--host`; o provedor é o do `bind`.
    fn bound(&self, sub: &str, api: &str, wal: &str) -> Command {
        let mut c = self.w.athc();
        c.env("BANK_URL", format!("http://{api}")).env("BANK_TOKEN", TOKEN);
        c.arg(sub).arg(prog()).args(["--provider", "scripted", "--models"]).arg(fx(self.models));
        c.arg("--trusted").arg(&self.trusted).args(["--timeout", "5", "--grants"]).arg(&self.grants);
        if sub == "run" {
            c.args(["--run-id", &self.run]);
        }
        c.arg("--wal").arg(self.wal(wal));
        c.arg("--journal").arg(self.w.p(&format!("{wal}.{sub}.jsonl")));
        c.arg("--report").arg(self.w.p(&format!("{wal}.{sub}.report.json")));
        c.arg("--args-sig").arg(&self.args_sig).args(["--arg", REQUEST]);
        c.args(["--anchor", &format!("tcp://{}", self.w.anchor), "--clock", &format!("tcp://{}", self.w.anchor)]);
        c
    }
}

/// Todo arquivo que a execução deixou no diretório do teste, concatenado.
fn everything_on_disk(dir: &Path) -> String {
    let mut all = String::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().is_file() {
            all.push_str(&String::from_utf8_lossy(&std::fs::read(e.path()).unwrap()));
        }
    }
    all
}

/// **O agente inteiro na linguagem.** O programa da tesouraria, com as ferramentas ligadas por
/// `bind` a uma API REST que exige um token: a execução passa por HTTP (o protocolo de linha
/// não é usado), com as mesmas garantias da RC4 (concessões, tempo atestado, ciclo de vida
/// ancorado, recibos assinados). O token não aparece na saída nem em arquivo algum.
#[test]
fn a_program_reaches_a_real_http_api_with_no_glue_and_the_secret_stays_out() {
    PROG.with(|p| *p.borrow_mut() = "examples/rc5/treasury.ath");
    let w = World::start("bind");
    let api = http(&w);
    let r = Run::fresh(&w, "r");
    let o = w.ok(&mut r.bound("run", &api, "r"));
    assert!(text(&o).contains(ANSWER) && text(&o).contains("ligada(s) por HTTP"), "{}", text(&o));
    assert_eq!(w.effects(), (2, 1, 1));
    assert_eq!((w.lifecycle("send"), w.lifecycle("outcome")), (3, 3), "o ciclo de vida ancorado vale por HTTP");
    let disk = everything_on_disk(&w.dir);
    assert!(!text(&o).contains(TOKEN) && !disk.contains(TOKEN), "o segredo vazou");
    assert!(disk.contains("BANK_TOKEN") || !disk.contains("secret"), "só o nome do segredo é citado");

    // sem o segredo no ambiente, ou sem a ligação: nada executa
    let before = w.state();
    let r2 = Run::fresh(&w, "r2");
    let o = r2.bound("run", &api, "r2").env_remove("BANK_TOKEN").output().unwrap();
    assert!(!o.status.success() && text(&o).contains("R0642"), "{}", text(&o));
    let o = r2.bound("run", &api, "r2b").arg("--no-bind").output().unwrap();
    assert!(text(&o).contains("R0641"), "{}", text(&o));
    // com o segredo errado, o provedor recusa (401): a recusa é definitiva, nada é aplicado
    let o = r2.bound("run", &api, "r2c").env("BANK_TOKEN", "errado").output().unwrap();
    assert!(text(&o).contains("HTTP 401"), "{}", text(&o));
    assert_eq!(w.state()["counts"], before["counts"]);
}

/// **A retomada vale pela ligação.** A execução ligada por HTTP é morta depois de enviar o
/// segundo pagamento e antes de enviar o agendamento; `athc resume` a termina pela mesma API.
#[cfg(unix)]
#[test]
fn a_bound_agent_resumes_over_http() {
    PROG.with(|p| *p.borrow_mut() = "examples/rc5/treasury.ath");
    for point in ["sent:2", "reserved:3", "confirmed:3"] {
        let w = World::start(&format!("bind-resume-{}", point.replace(':', "-")));
        let api = http(&w);
        let r = Run::fresh(&w, "r");
        let o = r.bound("run", &api, "r").args(["--kill-at", point]).output().unwrap();
        assert!(killed(&o), "{point}: {}", text(&o));
        let o = r.bound("resume", &api, "r").output().unwrap();
        assert!(o.status.success() && text(&o).contains(ANSWER), "{point}: {}", text(&o));
        assert_eq!(w.effects(), (2, 1, 1), "{point}");
        assert!(!everything_on_disk(&w.dir).contains(TOKEN), "{point}: o segredo vazou");
    }
}

const WHOAMI: &str = r#"module rc5.whoami

tool whoami(c: Cap[Bank.Read]) -> Text[..200] ^ Untrusted
  ! { Bank.Read } reversible latency 5.0

bind api {
  url    = env("BANK_URL")
  auth   = secret("BANK_TOKEN")
  whoami = "GET /v1/whoami -> id"
}

agent Probe(bank: Cap[Bank.Read])
  budget [tok <= 10, usd <= 0.01, wall <= 30.0]
{
  on ask(account: Text[..16] ^ Trusted) -> Text ! { Bank.Read } {
    let who = whoami(bank)
    "lido"
  }
}
"#;

/// **Um provedor que devolve o segredo não o entrega ao programa.** `GET /v1/whoami` responde
/// com o cabeçalho `Authorization` que recebeu. O valor que entra no programa (e no journal)
/// tem `‹secret:BANK_TOKEN›` no lugar do token.
#[test]
fn a_reflected_secret_is_redacted_before_it_enters_the_program() {
    let w = World::start("whoami");
    let api = http(&w);
    let prog = w.p("whoami.ath");
    std::fs::write(&prog, WHOAMI).unwrap();
    let mut c = w.athc();
    c.env("BANK_URL", format!("http://{api}")).env("BANK_TOKEN", TOKEN);
    c.arg("run").arg(&prog).args(["--arg", "\"TREASURY\"", "--timeout", "5"]).arg("--journal").arg(w.p("who.jsonl"));
    let o = w.ok(&mut c);
    let j = std::fs::read_to_string(w.p("who.jsonl")).unwrap();
    assert!(j.contains("Bearer ‹secret:BANK_TOKEN›"), "{j}");
    assert!(!j.contains(TOKEN) && !text(&o).contains(TOKEN), "o segredo refletido entrou no programa");
}

/// **A ligação é conferida antes de rodar.** Rota para o que não é ferramenta (`E0431`),
/// ferramenta ligada duas vezes (`E0432`), segredo em claro para outro host (`E0433`), operação
/// com chave num `GET` (`E0434`); e um segredo literal não é sequer sintaxe.
#[test]
fn a_binding_is_checked_before_anything_runs() {
    let dir = std::env::temp_dir().join(format!("ath-rc5-{}-static", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let base = std::fs::read_to_string(root().join("examples/rc5/treasury.ath")).unwrap();
    let check = |tag: &str, from: &str, to: &str| {
        assert!(base.contains(from), "{tag}: `{from}`");
        let p = dir.join(format!("{tag}.ath"));
        std::fs::write(&p, base.replace(from, to)).unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_athc")).env("NO_COLOR", "1").arg("check").arg(&p).output().unwrap();
        assert!(!o.status.success(), "{tag}: aceito: {}", text(&o));
        text(&o)
    };
    let route = "  wire      = \"POST /v1/wires -> id\"\n";
    assert!(check("unknown", route, "  wirre     = \"POST /v1/wires -> id\"\n").contains("E0431"));
    assert!(check("twice", route, &format!("{route}}}\n\nbind other {{\n  url = \"https://b.example\"\n{route}"))
        .contains("E0432"));
    assert!(check("clear", "env(\"BANK_URL\")", "\"http://bank.example\"").contains("E0433"));
    assert!(check("get", "\"POST /v1/payments -> id\"", "\"GET /v1/payments -> id\"").contains("E0434"));
    let lit = check("literal", "secret(\"BANK_TOKEN\")", "\"tok-123\"");
    assert!(lit.contains("nunca um literal do programa"), "{lit}");
    assert!(check("route", "\"POST /v1/wires -> id\"", "\"DELETE http://evil/x\"").contains("rota inválida"));
}
