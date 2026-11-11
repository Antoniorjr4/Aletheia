//! **v1.0-RC2: a fronteira entre autorização, persistência e efeito.**
//!
//! Os dois contraexemplos do RC1, com processos reais (`athc`, `ath-world`, SQLite, `SIGKILL`):
//!
//! 1. **a janela**: o RC1 consumia a concessão (`<concessões>.spent`) antes de persistir a
//!    intenção no WAL. Uma queda entre os dois deixava uma concessão gasta sem obrigação
//!    recuperável: a autorização do principal sumia sem efeito e sem registro;
//! 2. **a corrida**: o RC1 lia o registro de consumo uma vez, na carga. Dois processos com o
//!    mesmo arquivo de concessões viam a mesma concessão livre e os dois a gastavam.

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

struct World {
    dir: PathBuf,
    db: PathBuf,
    addr: String,
}

impl World {
    fn start(tag: &str) -> World {
        let dir = std::env::temp_dir().join(format!("ath-rc2-{}-{tag}", std::process::id()));
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

    fn fault(&self, tool: &str, kind: &str, ms: i64) {
        ath_world::add_fault(&ath_world::open(&self.db).unwrap(), tool, None, kind, ms).unwrap();
    }

    fn p(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn athc(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_athc"));
        c.env("NO_COLOR", "1");
        c
    }

    fn grant(&self, run: &str, intent: &str) -> PathBuf {
        let out = self.p(&format!("{run}.grants.json"));
        let o = self
            .athc()
            .args(["grant", "--run", run, "--origin", "confirmation:teste-rc2", "--out"])
            .arg(&out)
            .arg("--from")
            .arg(fx(intent))
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        out
    }

    /// `athc run` da tesouraria; `wal` é o nome do WAL (dois processos da mesma execução usam
    /// WALs diferentes).
    fn run_cmd(&self, run: &str, wal: &str, models: &str, grants: &Path) -> Command {
        let mut c = self.athc();
        c.arg("run")
            .arg(root().join("examples/rc1/treasury.ath"))
            .args(["--provider", "scripted", "--models"])
            .arg(fx(models))
            .args(["--host", &format!("tcp://{}", self.addr), "--trusted"])
            .arg(fx("trusted.json"))
            .args(["--run-id", run, "--timeout", "5", "--grants"])
            .arg(grants)
            .arg("--wal")
            .arg(self.p(&format!("{wal}.wal")))
            .arg("--journal")
            .arg(self.p(&format!("{wal}.journal.jsonl")))
            .arg("--report")
            .arg(self.p(&format!("{wal}.report.json")))
            .args(["--arg", REQUEST]);
        c
    }

    fn recover(&self, wal: &str, extra: &[&str]) -> Output {
        self.athc()
            .arg("recover")
            .arg(root().join("examples/rc1/treasury.ath"))
            .arg("--wal")
            .arg(self.p(&format!("{wal}.wal")))
            .args(["--host", &format!("tcp://{}", self.addr), "--timeout", "5", "--report"])
            .arg(self.p(&format!("{wal}.recover.json")))
            .args(extra)
            .output()
            .unwrap()
    }

    fn wal(&self, wal: &str) -> Vec<Json> {
        jsonl(&self.p(&format!("{wal}.wal")))
    }
}

fn jsonl(p: &Path) -> Vec<Json> {
    match std::fs::read_to_string(p) {
        Ok(t) => t.lines().map(|l| serde_json::from_str(l).unwrap()).collect(),
        Err(_) => Vec::new(),
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

/// As concessões consumidas segundo o registro de consumo (formatos do RC1 e do RC2).
fn spent(grants: &Path) -> Vec<String> {
    let mut v = Vec::new();
    for e in jsonl(&PathBuf::from(format!("{}.spent", grants.display()))) {
        if let Some(id) = e["grant"].as_str() {
            v.push(id.to_string());
        }
        v.extend(e["grants"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)));
    }
    v
}

/// As concessões que alguma intenção do WAL cita.
fn intended(wal: &[Json]) -> Vec<String> {
    wal.iter()
        .filter(|e| e["kind"] == "intent")
        .flat_map(|e| e["detail"]["grants"].as_array().cloned().unwrap_or_default())
        .filter_map(|x| x.as_str().map(String::from))
        .collect()
}

/// **A janela.** `SIGKILL` na fronteira entre a autorização e a persistência da intenção, em
/// cada uma das três operações autorizadas por concessão. O invariante: toda concessão
/// consumida tem uma intenção no WAL que a cita. Sem ele, a recuperação não tem como saber que
/// a autorização foi gasta, e o principal perde a operação que confirmou.
#[cfg(unix)]
#[test]
fn sigkill_between_authorization_and_intent_never_strands_a_grant() {
    for n in 1..=3 {
        let w = World::start(&format!("window-{n}"));
        let g = w.grant("r", "intent.json");
        let o = w
            .run_cmd("r", "r", "models-legit.json", &g)
            .args(["--kill-at", &format!("authorized:{n}")])
            .output()
            .unwrap();
        assert!(killed(&o), "operação {n}: {}", text(&o));
        let cited = intended(&w.wal("r"));
        for id in spent(&g) {
            assert!(
                cited.contains(&id),
                "operação {n}: a concessão {} foi consumida e nenhuma intenção do WAL a cita",
                &id[..12]
            );
        }
    }
}

/// **A corrida.** Dois processos independentes da mesma execução, com o mesmo arquivo de
/// concessões e WALs diferentes, largam juntos (o serviço segura a resposta da primeira leitura
/// para que os dois já tenham carregado as concessões). O agendamento passa por um provedor
/// `queryable`, que não deduplica: se os dois processos gastarem a mesma concessão, o serviço
/// registra dois agendamentos para uma autorização.
#[test]
fn two_processes_cannot_spend_the_same_grant() {
    let w = World::start("race");
    let g = w.grant("r", "intent.json");
    w.fault("statement", "hang", 600);
    w.fault("statement", "hang", 600);
    let spawn = |wal: &str| {
        w.run_cmd("r", wal, "models-legit.json", &g).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
    };
    let (a, b) = (spawn("a"), spawn("b"));
    let (oa, ob) = (a.wait_with_output().unwrap(), b.wait_with_output().unwrap());
    let ids = spent(&g);
    let mut uniq = ids.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(ids.len(), uniq.len(), "uma concessão foi consumida duas vezes: {ids:?}\n{}\n{}", text(&oa), text(&ob));
    assert_eq!(w.count("scheduled"), 1, "um agendamento autorizado, {} registrados", w.count("scheduled"));
}
