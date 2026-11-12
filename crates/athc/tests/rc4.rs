//! **v1.0-RC4: autoridade temporal e ciclo de vida verificado do efeito.**
//!
//! Quatro contraexemplos da RC3, com processos reais:
//!
//! 1. **relógio local**: a validade era conferida contra o relógio do executor; atrasá-lo
//!    revalidava concessões expiradas;
//! 2. **desfecho apagado**: a âncora cobria o consumo, não o desfecho; apagar o `close` do WAL
//!    passava sem aviso e a recuperação voltava a consultar o provedor;
//! 3. **linha forjada**: uma linha bem encadeada acrescentada ao registro era ancorada na
//!    abertura seguinte (indistinguível de uma queda entre o consumo e a âncora);
//! 4. **não utilização provada, concessão perdida**: uma queda com o consumo gravado e a âncora
//!    ainda atrás prova que nada saiu, e mesmo assim a concessão ficava consumida.

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
fn lines(p: &Path) -> Vec<String> {
    std::fs::read_to_string(p).unwrap_or_default().lines().map(String::from).collect()
}
fn write_lines(p: &Path, ls: &[String]) {
    std::fs::write(p, ls.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
}

/// O programa sob teste.
fn prog() -> PathBuf {
    root().join("examples/rc3/treasury.ath")
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

/// O provedor (o banco) e a âncora, em bancos separados, e um diretório de trabalho.
struct World {
    dir: PathBuf,
    db: PathBuf,
    addr: String,
    anchor_db: PathBuf,
    anchor: String,
}

fn serve(db: &Path) -> String {
    let c = ath_world::open(db).unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    std::thread::spawn(move || ath_world::serve(l, c));
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
        let (addr, anchor) = (serve(&db), serve(&anchor_db));
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
    fn anchored(&self) -> u64 {
        ath_world::state(&ath_world::open(&self.anchor_db).unwrap()).unwrap()["anchored"].as_u64().unwrap()
    }
    /// Quantas requisições o provedor recebeu que citam `key`.
    fn requests_for(&self, key: &str) -> usize {
        let c = ath_world::open(&self.db).unwrap();
        let mut st = c.prepare("SELECT line FROM log ORDER BY seq").unwrap();
        let v: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
        v.iter().filter(|l| l.contains(key)).count()
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
/// prazo (`issued`, `ttl`), e o registro ancorado.
struct Run<'w> {
    w: &'w World,
    run: String,
    grants: PathBuf,
    trusted: PathBuf,
    args_sig: PathBuf,
}

impl<'w> Run<'w> {
    fn signed(w: &'w World, run: &str, issued: u64, ttl: u64) -> Run<'w> {
        let r = Run {
            w,
            run: run.into(),
            grants: w.p(&format!("{run}.grants.json")),
            trusted: w.p(&format!("{run}.trusted.json")),
            args_sig: w.p(&format!("{run}.args.sig")),
        };
        let stamp = |c: &mut Command| {
            c.arg("--program").arg(prog()).args(["--issued", &issued.to_string(), "--ttl", &ttl.to_string()]);
        };
        let _ = std::fs::remove_file(&r.grants);
        let mut c = w.athc();
        c.args(["grant", "--run", run, "--origin", "confirmation:teste-rc4", "--out"]).arg(&r.grants);
        c.arg("--from").arg(fx("intent.json")).arg("--sign").arg(demo_key());
        stamp(&mut c);
        w.ok(&mut c);
        std::fs::copy(fx("trusted.json"), &r.trusted).unwrap();
        let mut c = w.athc();
        c.args(["sign", "--key"]).arg(demo_key()).arg("--trusted").arg(&r.trusted).args(["--run", run]);
        stamp(&mut c);
        w.ok(&mut c);
        let mut c = w.athc();
        c.args(["sign", "--key"]).arg(demo_key()).args(["--run", run, "--arg", REQUEST, "--out"]).arg(&r.args_sig);
        stamp(&mut c);
        w.ok(&mut c);
        r
    }
    fn fresh(w: &'w World, run: &str) -> Run<'w> {
        Run::signed(w, run, now(), 600)
    }
    fn ledger(&self) -> PathBuf {
        PathBuf::from(format!("{}.spent", self.grants.display()))
    }
    fn cmd(&self, wal: &str) -> Command {
        let mut c = self.w.athc();
        c.arg("run").arg(prog()).args(["--provider", "scripted", "--models"]).arg(fx("models-legit.json"));
        c.args(["--host", &format!("tcp://{}", self.w.addr), "--trusted"]).arg(&self.trusted);
        c.args(["--run-id", &self.run, "--timeout", "5", "--grants"]).arg(&self.grants);
        c.arg("--wal").arg(self.w.p(&format!("{wal}.wal")));
        c.arg("--journal").arg(self.w.p(&format!("{wal}.journal.jsonl")));
        c.arg("--args-sig").arg(&self.args_sig).args(["--arg", REQUEST]);
        c.args(["--anchor", &format!("tcp://{}", self.w.anchor)]);
        c
    }
    fn recover(&self, wal: &str) -> Output {
        let mut c = self.w.athc();
        c.arg("recover").arg(prog()).arg("--wal").arg(self.w.p(&format!("{wal}.wal")));
        c.args(["--host", &format!("tcp://{}", self.w.addr), "--timeout", "5"]);
        c.output().unwrap()
    }
}

/// **Contraexemplo 1: o relógio local.** As três fontes foram assinadas há duas horas, com dez
/// minutos de validade: estão expiradas. O executor roda com o relógio de parede atrasado duas
/// horas. Nenhum efeito pode sair.
#[cfg(unix)]
#[test]
fn a_rewound_local_clock_does_not_revalidate_an_expired_grant() {
    let w = World::start("clock");
    let r = Run::signed(&w, "r", now() - 7200, 600);
    // sem manipulação: recusado, como na RC3
    let o = r.cmd("honesto").output().unwrap();
    assert!(!o.status.success() && text(&o).contains("expirou"), "{}", text(&o));
    // com o relógio do processo atrasado duas horas
    let o = r.cmd("atrasado").env("LD_PRELOAD", clock_shim()).env("ATH_FAKE_CLOCK_OFFSET", "-7200").output().unwrap();
    assert_eq!(w.effects(), (0, 0, 0), "o relógio atrasado revalidou concessões expiradas\n{}", text(&o));
}

/// **Contraexemplo 2: o desfecho apagado.** Uma execução completa; o WAL retrocede até antes do
/// desfecho do agendamento. O desfecho já tinha sido confirmado: a recuperação não pode tratá-lo
/// como desconhecido e voltar ao provedor para descobrir.
#[test]
fn a_confirmed_outcome_does_not_vanish_with_the_wal() {
    let w = World::start("outcome");
    let r = Run::fresh(&w, "r");
    w.ok(&mut r.cmd("r"));
    let wal = lines(&w.p("r.wal"));
    let cut = wal.iter().position(|l| l.contains("\"kind\":\"close\"") && l.contains("\"key\":\"k3\"")).unwrap();
    write_lines(&w.p("r.wal"), &wal[..cut]);
    let before = w.requests_for("r:k3");
    let rec = r.recover("r");
    assert_eq!(
        w.requests_for("r:k3"),
        before,
        "a recuperação foi ao provedor por um desfecho já confirmado\n{}",
        text(&rec)
    );
}

/// **Contraexemplo 3: a linha forjada.** Quem escreve no disco acrescenta ao registro uma linha
/// bem encadeada que "consome" uma concessão que ninguém usou. O histórico ancorado não pode
/// crescer com algo que nenhum executor autorizado produziu.
#[test]
fn a_forged_ledger_line_is_never_anchored() {
    let w = World::start("forged");
    let r = Run::fresh(&w, "r");
    w.ok(&mut r.cmd("r"));
    assert_eq!(w.anchored(), 3);
    let real = lines(&r.ledger());
    let mut e: Json = serde_json::from_str(real.last().unwrap()).unwrap();
    let prev = e["hash"].clone();
    e.as_object_mut().unwrap().remove("hash");
    e["seq"] = serde_json::json!(real.len());
    e["prev"] = prev;
    e["grants"] = serde_json::json!(["f".repeat(64)]);
    e["hash"] = serde_json::json!(blake3::hash(e.to_string().as_bytes()).to_hex().to_string());
    let mut forged = real.clone();
    forged.push(e.to_string());
    write_lines(&r.ledger(), &forged);
    let o = r.cmd("depois").output().unwrap();
    assert_eq!(w.anchored(), 3, "a linha forjada foi ancorada\n{}", text(&o));
}

/// **Contraexemplo 4: não utilização provada.** `SIGKILL` com o consumo do agendamento gravado e
/// a âncora ainda atrás: o envio só acontece depois da âncora, logo nada saiu. A concessão do
/// principal não pode se perder: uma nova execução faz o agendamento, uma vez.
#[cfg(unix)]
#[test]
fn a_provably_unsent_operation_releases_its_grant() {
    let w = World::start("release");
    let r = Run::fresh(&w, "r");
    let o = r.cmd("first").args(["--kill-at", "spent:3"]).output().unwrap();
    assert!(killed(&o), "{}", text(&o));
    assert_eq!(w.requests_for("r:k3"), 0);
    let rec = r.recover("first");
    assert_eq!(
        w.requests_for("r:k3"),
        0,
        "a recuperação consultou o provedor sobre o que provadamente não saiu\n{}",
        text(&rec)
    );
    let o = r.cmd("second").output().unwrap();
    assert_eq!(w.count("scheduled"), 1, "a concessão do agendamento não foi liberada\n{}", text(&o));
}
