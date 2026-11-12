//! **v1.0-RC3: autoridade durável e integridade temporal.**
//!
//! Quatro contraexemplos do RC2, com processos reais (`athc`, `ath-world`, SQLite, `SIGKILL`).
//! No RC2 a autorização era verdadeira no instante da concessão, mas a sua **história** não era
//! verificável:
//!
//! 1. **retrocesso do registro de consumo**: apagar a última linha (ou o arquivo) devolvia
//!    concessões já gastas; uma nova execução repetia efeitos já feitos;
//! 2. **recuperação que reintegra**: com a linha do consumo apagada, `athc recover` concluía que
//!    a operação "não saiu" e liberava a concessão de um efeito que já tinha acontecido;
//! 3. **fotografia antiga**: uma fotografia confiável autêntica valia para sempre, em qualquer
//!    execução;
//! 4. **outro programa**: uma concessão valia em qualquer programa com a mesma autoridade e o
//!    mesmo identificador de execução.

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

#[cfg(unix)]
fn killed(o: &Output) -> bool {
    use std::os::unix::process::ExitStatusExt;
    o.status.signal() == Some(9)
}

fn jsonl(p: &Path) -> Vec<Json> {
    match std::fs::read_to_string(p) {
        Ok(t) => t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Um serviço `ath-world` próprio (o banco) num diretório próprio.
struct World {
    dir: PathBuf,
    db: PathBuf,
    addr: String,
}

impl World {
    fn start(tag: &str) -> World {
        let dir = std::env::temp_dir().join(format!("ath-rc3-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("bank.sqlite");
        let c = ath_world::open(&db).unwrap();
        let seed: Json = serde_json::from_str(&std::fs::read_to_string(fx("seed.json")).unwrap()).unwrap();
        ath_world::seed(&c, &seed).unwrap();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        std::thread::spawn(move || ath_world::serve(l, c));
        World { dir, db, addr }
    }

    fn state(&self) -> Json {
        ath_world::state(&ath_world::open(&self.db).unwrap()).unwrap()
    }

    fn count(&self, k: &str) -> u64 {
        self.state()["counts"][k].as_u64().unwrap()
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

/// Uma execução do programa `prog` com tudo assinado pela autoridade de demonstração, no
/// formato do RC2: concessões, fotografia confiável e pedido.
struct Run<'w> {
    w: &'w World,
    prog: PathBuf,
    run: String,
    grants: PathBuf,
    trusted: PathBuf,
    args_sig: PathBuf,
}

impl<'w> Run<'w> {
    fn signed(w: &'w World, prog: &Path, run: &str) -> Run<'w> {
        let r = Run {
            w,
            prog: prog.to_path_buf(),
            run: run.into(),
            grants: w.p(&format!("{run}.grants.json")),
            trusted: w.p(&format!("{run}.trusted.json")),
            args_sig: w.p(&format!("{run}.args.sig")),
        };
        w.ok(w
            .athc()
            .args(["grant", "--run", run, "--origin", "confirmation:teste-rc3", "--out"])
            .arg(&r.grants)
            .arg("--from")
            .arg(fx("intent.json"))
            .arg("--sign")
            .arg(demo_key()));
        std::fs::copy(fx("trusted.json"), &r.trusted).unwrap();
        w.ok(w.athc().args(["sign", "--key"]).arg(demo_key()).arg("--trusted").arg(&r.trusted));
        w.ok(w
            .athc()
            .args(["sign", "--key"])
            .arg(demo_key())
            .args(["--run", run, "--arg", REQUEST, "--out"])
            .arg(&r.args_sig));
        r
    }

    fn ledger(&self) -> PathBuf {
        PathBuf::from(format!("{}.spent", self.grants.display()))
    }

    fn cmd(&self, wal: &str) -> Command {
        let mut c = self.w.athc();
        c.arg("run")
            .arg(&self.prog)
            .args(["--provider", "scripted", "--models"])
            .arg(fx("models-legit.json"))
            .args(["--host", &format!("tcp://{}", self.w.addr), "--trusted"])
            .arg(&self.trusted)
            .args(["--run-id", &self.run, "--timeout", "5", "--grants"])
            .arg(&self.grants)
            .arg("--wal")
            .arg(self.w.p(&format!("{wal}.wal")))
            .arg("--journal")
            .arg(self.w.p(&format!("{wal}.journal.jsonl")))
            .arg("--args-sig")
            .arg(&self.args_sig)
            .args(["--arg", REQUEST]);
        c
    }

    fn recover(&self, wal: &str) -> Output {
        self.w
            .athc()
            .arg("recover")
            .arg(&self.prog)
            .arg("--wal")
            .arg(self.w.p(&format!("{wal}.wal")))
            .args(["--host", &format!("tcp://{}", self.w.addr), "--timeout", "5"])
            .output()
            .unwrap()
    }
}

fn rc2_prog() -> PathBuf {
    root().join("examples/rc2/treasury.ath")
}

/// Tira as últimas `n` linhas do registro de consumo (o retrocesso que um backup restaurado,
/// ou alguém com acesso ao disco, produz).
fn truncate_ledger(ledger: &Path, n: usize) {
    let t = std::fs::read_to_string(ledger).unwrap();
    let lines: Vec<&str> = t.lines().collect();
    let keep = lines.len() - n;
    std::fs::write(ledger, lines[..keep].iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
}

/// **Contraexemplo 1.** Uma execução completa gasta as três concessões. O registro de consumo
/// retrocede (a última linha some, ou o arquivo inteiro). Uma nova execução, com as mesmas
/// concessões, não pode repetir efeito algum.
#[test]
fn a_rolled_back_ledger_does_not_reinstate_spent_grants() {
    for (tag, whole) in [("tail", false), ("file", true)] {
        let w = World::start(&format!("rollback-{tag}"));
        let r = Run::signed(&w, &rc2_prog(), "r");
        w.ok(&mut r.cmd("first"));
        assert_eq!((w.count("pays_posted"), w.count("scheduled")), (2, 1));
        if whole {
            std::fs::remove_file(r.ledger()).unwrap();
        } else {
            truncate_ledger(&r.ledger(), 1);
        }
        let o = r.cmd("second").output().unwrap();
        assert_eq!(w.count("scheduled"), 1, "{tag}: o agendamento foi repetido depois do retrocesso\n{}", text(&o));
        assert_eq!(w.state()["accounts"]["TREASURY"], "9804.50", "{tag}");
    }
}

/// **Contraexemplo 2.** `SIGKILL` depois do envio do agendamento; a linha do consumo some. A
/// recuperação não pode concluir que a operação "não saiu" e liberar a concessão: ela saiu.
#[cfg(unix)]
#[test]
fn recovery_does_not_reinstate_a_grant_whose_effect_left() {
    let w = World::start("recover-rollback");
    let r = Run::signed(&w, &rc2_prog(), "r");
    let o = r.cmd("first").args(["--kill-at", "sent:3"]).output().unwrap();
    assert!(killed(&o), "{}", text(&o));
    assert_eq!(w.count("scheduled"), 1);
    truncate_ledger(&r.ledger(), 1);
    let rec = r.recover("first");
    let released = jsonl(&w.p("first.wal")).iter().any(|e| e["detail"]["evidence"] == "recovery:unspent");
    assert!(!released, "a recuperação liberou a concessão de um efeito que saiu\n{}", text(&rec));
}

/// **Contraexemplo 3.** A fotografia confiável assinada para uma execução não vale na seguinte:
/// a autoridade pode ter mudado o cadastro desde então.
#[test]
fn an_old_trusted_snapshot_is_not_accepted_in_a_later_run() {
    let w = World::start("stale");
    let old = Run::signed(&w, &rc2_prog(), "ontem");
    let mut new = Run::signed(&w, &rc2_prog(), "hoje");
    // a execução de hoje apresenta a fotografia (autêntica) de ontem
    new.trusted = old.trusted.clone();
    let o = new.cmd("hoje").output().unwrap();
    assert_eq!(w.count("pays_posted"), 0, "a fotografia de outra execução foi aceita\n{}", text(&o));
}

/// **Contraexemplo 4.** Uma concessão assinada para um programa não vale em outro programa da
/// mesma autoridade, ainda que o identificador de execução coincida.
#[test]
fn a_grant_does_not_authorize_another_program() {
    let w = World::start("program");
    let granted = Run::signed(&w, &rc2_prog(), "r");
    // outro programa (outro Codex), mesma autoridade: o teto de orçamento é diferente
    let other = w.p("other.ath");
    let src = std::fs::read_to_string(rc2_prog()).unwrap();
    assert!(src.contains("usd <= 0.10"));
    std::fs::write(&other, src.replace("usd <= 0.10", "usd <= 0.11")).unwrap();
    let mut r = Run::signed(&w, &other, "r");
    r.grants = granted.grants.clone();
    let _ = &granted.prog;
    let o = r.cmd("other").output().unwrap();
    assert_eq!(w.count("pays_posted"), 0, "a concessão de um programa foi aceita por outro\n{}", text(&o));
}
