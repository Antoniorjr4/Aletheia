//! **v1.0-RC2: a fronteira entre autorização, persistência e efeito.**
//!
//! Os dois contraexemplos do RC1, com processos reais (`athc`, `ath-world`, SQLite, `SIGKILL`):
//!
//! 1. **a janela**: o RC1 consumia a concessão (`<concessões>.spent`) antes de persistir a
//!    intenção no WAL. Uma queda entre os dois deixava uma concessão gasta sem obrigação
//!    recuperável: a autorização do principal sumia sem efeito e sem registro;
//! 2. **a corrida**: o RC1 lia o registro de consumo uma vez, na carga. Dois processos com o
//!    mesmo arquivo de concessões viam a mesma concessão livre e os dois a gastavam.
//!
//! Os dois testes que os reproduzem falham no commit que os introduziu (a saída está em
//! `docs/evidence/rc2-contraexemplos-no-rc1.txt`) e passam com o protocolo do RC2: intenção →
//! consumo → envio, sob a trava do registro de consumo (`lean/Aletheia/Txn.lean`).

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

    /// Quantas requisições o serviço recebeu para a chave de fio (nenhuma: a operação não saiu).
    fn requests_for(&self, key: &str) -> usize {
        let c = ath_world::open(&self.db).unwrap();
        let mut st = c.prepare("SELECT line FROM log ORDER BY seq").unwrap();
        let v: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
        v.iter().filter(|l| l.contains(key)).count()
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

/// O invariante da fronteira: toda concessão consumida tem uma intenção no WAL que a cita.
fn no_stranded_grant(w: &World, wal: &str, g: &Path, ctx: &str) {
    let cited = intended(&w.wal(wal));
    for id in spent(g) {
        assert!(cited.contains(&id), "{ctx}: a concessão {} foi consumida e nenhuma intenção do WAL a cita", &id[..12]);
    }
}

/// O desfecho que a recuperação deu à chave, com a evidência.
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

/// **A janela.** `SIGKILL` na fronteira entre a autorização e a persistência, nos dois pontos
/// (`authorized`: o contrato passou e nada está em disco; `logged`: a intenção está no WAL e o
/// consumo ainda não está no registro), em cada uma das três operações autorizadas por
/// concessão (dois pagamentos numa saga, `idempotent`; um agendamento, `queryable`).
///
/// Depois da queda e depois da recuperação: nenhuma concessão consumida sem intenção que a
/// cite; a operação interrompida não chegou ao serviço e não é enviada pela recuperação; a sua
/// concessão continua livre, e isso está escrito no WAL. Recuperar de novo não muda nada.
#[cfg(unix)]
#[test]
fn sigkill_between_authorization_and_intent_never_strands_a_grant() {
    for point in ["authorized", "logged"] {
        for n in 1..=3u32 {
            let ctx = format!("{point}:{n}");
            let w = World::start(&format!("window-{point}-{n}"));
            let g = w.grant("r", "intent.json");
            let o = w.run_cmd("r", "r", "models-legit.json", &g).args(["--kill-at", &ctx]).output().unwrap();
            assert!(killed(&o), "{ctx}: {}", text(&o));
            no_stranded_grant(&w, "r", &g, &ctx);
            // a operação interrompida: n-1 concessões consumidas, e o serviço nunca a viu
            let key = format!("k{n}");
            assert_eq!(spent(&g).len() as u32, n - 1, "{ctx}");
            assert_eq!(w.requests_for(&format!("r:{key}")), 0, "{ctx}: a operação saiu do processo");
            let has_intent = w.wal("r").iter().any(|e| e["kind"] == "intent" && e["detail"]["key"] == key.as_str());
            assert_eq!(has_intent, point == "logged", "{ctx}");

            // a recuperação, em outro processo: a queda não deixou trava órfã
            let r = w.recover("r", &[]);
            assert_eq!(r.status.code(), Some(0), "{ctx}: {}", text(&r));
            let wal = w.wal("r");
            no_stranded_grant(&w, "r", &g, &ctx);
            assert_eq!(w.requests_for(&format!("r:{key}")), 0, "{ctx}: a recuperação enviou a operação");
            if point == "logged" {
                // a intenção sem consumo é fechada com a evidência, e a concessão liberada é nomeada
                assert_eq!(outcome(&wal, &key), Some(("failed".into(), "recovery:unspent".into())), "{ctx}");
                let rel = wal.iter().find(|e| e["detail"]["evidence"] == "recovery:unspent").unwrap();
                assert_eq!(rel["detail"]["released"].as_array().unwrap().len(), 1, "{ctx}");
                assert!(text(&r).contains("continua(m) livre(s)"), "{ctx}: {}", text(&r));
            }
            assert_eq!(spent(&g).len() as u32, n - 1, "{ctx}: a recuperação consumiu ou devolveu concessão");
            // a saga interrompida foi estornada: nenhum pagamento fica pela metade
            let s = w.state();
            // (a saga dos pagamentos já tinha terminado quando a queda é no agendamento)
            assert_eq!(s["accounts"]["TREASURY"], if n < 3 { "10000.00" } else { "9804.50" }, "{ctx}");
            assert_eq!(w.count("scheduled"), 0, "{ctx}");
            // recuperar de novo: nada a fazer, nada enviado, WAL idêntico
            let before = std::fs::read(w.p("r.wal")).unwrap();
            let r2 = w.recover("r", &[]);
            assert_eq!(r2.status.code(), Some(0), "{ctx}: {}", text(&r2));
            assert_eq!(std::fs::read(w.p("r.wal")).unwrap(), before, "{ctx}: a segunda recuperação escreveu");
        }
    }
}

/// **A autorização não se perde.** Depois de uma queda na fronteira da primeira operação, as
/// três concessões continuam livres: uma nova execução (WAL novo, mesmas concessões) faz o que
/// o principal confirmou, exatamente uma vez. No RC1, a concessão do primeiro pagamento ficava
/// gasta e a nova execução era recusada.
#[cfg(unix)]
#[test]
fn an_authorization_survives_a_crash_before_its_effect() {
    for point in ["authorized", "logged"] {
        let w = World::start(&format!("survives-{point}"));
        let g = w.grant("r", "intent.json");
        let o = w
            .run_cmd("r", "first", "models-legit.json", &g)
            .args(["--kill-at", &format!("{point}:1")])
            .output()
            .unwrap();
        assert!(killed(&o), "{}", text(&o));
        assert_eq!(w.recover("first", &[]).status.code(), Some(0));
        assert!(spent(&g).is_empty(), "{point}");
        let o = w.run_cmd("r", "second", "models-legit.json", &g).output().unwrap();
        assert!(o.status.success(), "{point}: {}", text(&o));
        let s = w.state();
        assert_eq!(s["accounts"]["ACME"], "120.00", "{point}");
        assert_eq!(s["accounts"]["BETA"], "75.50", "{point}");
        assert_eq!(s["accounts"]["TREASURY"], "9804.50", "{point}");
        assert_eq!((w.count("pays_posted"), w.count("scheduled"), w.count("receipts")), (2, 1, 1), "{point}");
        assert_eq!(spent(&g).len(), 3, "{point}");
        no_stranded_grant(&w, "second", &g, point);
    }
}

/// **Provedor opaco, antes do envio.** No RC1, uma remessa interrompida antes do envio era
/// indistinguível, pelo WAL, de uma interrompida depois: as duas eram escaladas. Com a
/// intenção antes do consumo, a queda em `logged` prova que a remessa não saiu (o consumo vem
/// antes do envio e não está lá): é fechada como não enviada, sem intervenção. A queda em
/// `intent` (intenção e consumo persistidos) continua escalada: ali não há como saber.
#[cfg(unix)]
#[test]
fn an_opaque_operation_that_provably_never_left_is_not_escalated() {
    let w = World::start("opaque");
    for (point, code, status) in [("logged", 0, "failed"), ("intent", 3, "escalated")] {
        let run = format!("wire-{point}");
        let g = w.grant(&run, "intent-wire.json");
        let o =
            w.run_cmd(&run, &run, "models-wire.json", &g).args(["--kill-at", &format!("{point}:2")]).output().unwrap();
        assert!(killed(&o), "{point}: {}", text(&o));
        let r = w.recover(&run, &[]);
        assert_eq!(r.status.code(), Some(code), "{point}: {}", text(&r));
        assert_eq!(outcome(&w.wal(&run), "k2").map(|x| x.0).as_deref(), Some(status), "{point}");
        assert_eq!(w.count("wires"), 0, "{point}: a remessa nunca é enviada pela recuperação");
        no_stranded_grant(&w, &run, &g, point);
    }
}

/// **Sem o registro de consumo não se adivinha.** A intenção cita uma concessão; se o registro
/// some, a recuperação não tem como saber se a operação saiu: para com `W0007`, não envia nada
/// e não escreve no WAL.
#[cfg(unix)]
#[test]
fn a_missing_spend_ledger_stops_recovery() {
    let w = World::start("noledger");
    let g = w.grant("r", "intent.json");
    let o = w.run_cmd("r", "r", "models-legit.json", &g).args(["--kill-at", "intent:1"]).output().unwrap();
    assert!(killed(&o), "{}", text(&o));
    let ledger = PathBuf::from(format!("{}.spent", g.display()));
    let saved = std::fs::read(&ledger).unwrap();
    std::fs::remove_file(&ledger).unwrap();
    let before = std::fs::read(w.p("r.wal")).unwrap();
    let r = w.recover("r", &[]);
    assert_eq!(r.status.code(), Some(2), "{}", text(&r));
    assert!(text(&r).contains("W0007"), "{}", text(&r));
    assert_eq!(std::fs::read(w.p("r.wal")).unwrap(), before);
    assert_eq!(w.requests_for("r:k1"), 0);
    // com o registro de volta, a recuperação decide: o consumo está lá, o provedor é
    // `idempotent`, a operação é reenviada com a mesma chave e a saga interrompida é estornada
    std::fs::write(&ledger, saved).unwrap();
    let r = w.recover("r", &[]);
    assert_eq!(r.status.code(), Some(0), "{}", text(&r));
    assert_eq!(outcome(&w.wal("r"), "k1"), Some(("done".into(), "recovery:resend".into())));
    assert_eq!(w.state()["accounts"]["TREASURY"], "10000.00");
}

/// **A recuperação é determinística.** Duas cópias do mesmo estado depois da queda (WAL e
/// registro de consumo), recuperadas por processos independentes contra serviços independentes
/// no mesmo estado, tomam as mesmas decisões, na mesma ordem.
#[cfg(unix)]
#[test]
fn recovery_is_a_function_of_the_durable_state() {
    for kill in ["logged:2", "intent:2", "sent:3"] {
        let decisions = |tag: &str| {
            let w = World::start(&format!("det-{}-{tag}", kill.replace(':', "-")));
            let g = w.grant("r", "intent.json");
            let o = w.run_cmd("r", "r", "models-legit.json", &g).args(["--kill-at", kill]).output().unwrap();
            assert!(killed(&o), "{kill}: {}", text(&o));
            let n = w.wal("r").len();
            let r = w.recover("r", &[]);
            assert!(r.status.code() == Some(0), "{kill}: {}", text(&r));
            let d: Vec<String> = w.wal("r")[n..]
                .iter()
                .map(|e| {
                    let d = &e["detail"];
                    format!("{} {} {} {} {}", e["kind"], d["key"], d["status"], d["evidence"], d["saga"])
                })
                .collect();
            (d, w.state()["accounts"].clone(), w.state()["counts"].clone())
        };
        assert_eq!(decisions("a"), decisions("b"), "{kill}");
    }
}

/// **A corrida.** Quatro processos independentes da mesma execução, com o mesmo arquivo de
/// concessões e WALs diferentes, largam juntos (o serviço segura a resposta da primeira leitura
/// para que todos já tenham carregado as concessões). O agendamento passa por um provedor
/// `queryable`, que não deduplica: no RC1, cada processo gastava a mesma concessão e o serviço
/// registrava um agendamento por processo.
///
/// Com a seção crítica sob a trava do registro: cada concessão é consumida uma vez, o serviço
/// termina no estado de **uma** execução, e os perdedores são recusados (`R0632`) sem efeito.
#[test]
fn two_processes_cannot_spend_the_same_grant() {
    for round in 0..3 {
        let w = World::start(&format!("race-{round}"));
        let g = w.grant("r", "intent.json");
        const N: usize = 4;
        for _ in 0..N {
            w.fault("statement", "hang", 700);
        }
        let procs: Vec<_> = (0..N)
            .map(|i| {
                w.run_cmd("r", &format!("p{i}"), "models-legit.json", &g)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        let outs: Vec<Output> = procs.into_iter().map(|p| p.wait_with_output().unwrap()).collect();
        let all: String = outs.iter().map(text).collect::<Vec<_>>().join("\n---\n");
        let ids = spent(&g);
        let mut uniq = ids.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(ids.len(), uniq.len(), "uma concessão foi consumida duas vezes: {ids:?}\n{all}");
        assert_eq!(ids.len(), 3, "{all}");
        let s = w.state();
        assert_eq!(w.count("scheduled"), 1, "um agendamento autorizado, {} registrados\n{all}", w.count("scheduled"));
        assert_eq!(s["accounts"]["ACME"], "120.00", "{all}");
        assert_eq!(s["accounts"]["BETA"], "75.50", "{all}");
        assert_eq!(s["accounts"]["TREASURY"], "9804.50", "{all}");
        assert_eq!((w.count("pays_posted"), w.count("receipts")), (2, 1), "{all}");
        // cada concessão foi paga por exatamente uma intenção, de um dos WALs
        let mut cited: Vec<String> = (0..N).flat_map(|i| intended(&w.wal(&format!("p{i}")))).collect();
        cited.sort();
        assert_eq!(cited, uniq, "{all}");
        assert!(all.matches("R0632").count() >= N - 1, "{all}");
    }
}

/// **Um executor por WAL.** Enquanto uma execução está viva (o serviço segura a resposta), um
/// segundo `athc run` e um `athc recover` no mesmo WAL são recusados com `W0006`, sem enviar
/// nada. Depois que ela termina, a trava some com o processo.
#[test]
fn one_executor_per_wal() {
    let w = World::start("wal-lock");
    let g = w.grant("r", "intent.json");
    w.fault("statement", "hang", 2500);
    let alive =
        w.run_cmd("r", "r", "models-legit.json", &g).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    // espera a primeira execução criar o WAL (ela já detém a trava quando o escreve)
    for _ in 0..200 {
        if w.p("r.wal").exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let second = w.run_cmd("r", "r", "models-legit.json", &g).output().unwrap();
    assert_eq!(second.status.code(), Some(2), "{}", text(&second));
    assert!(text(&second).contains("W0006"), "{}", text(&second));
    let rec = w.recover("r", &[]);
    assert_eq!(rec.status.code(), Some(2), "{}", text(&rec));
    assert!(text(&rec).contains("W0006"), "{}", text(&rec));
    let first = alive.wait_with_output().unwrap();
    assert!(first.status.success(), "{}", text(&first));
    assert_eq!((w.count("pays_posted"), w.count("scheduled"), w.count("receipts")), (2, 1, 1));
    // a execução terminou: a trava não ficou, e não há o que recuperar
    let rec = w.recover("r", &[]);
    assert_eq!(rec.status.code(), Some(0), "{}", text(&rec));
}

// ------------------------------------------------------------------ autenticidade

/// Uma execução de `examples/rc2/treasury.ath`, que declara a autoridade `treasury`. Cada
/// fonte de autoridade pode ser assinada com a chave da autoridade (a de demonstração, da RFC
/// 8032), com outra chave, ou não ser assinada.
struct Signed<'w> {
    w: &'w World,
    run: String,
    grants: PathBuf,
    trusted: PathBuf,
    args_sig: Option<String>,
}

fn demo_key() -> PathBuf {
    root().join("examples/rc2/fixtures/DEMO-rfc8032.key")
}

impl<'w> Signed<'w> {
    /// Tudo assinado pela autoridade do programa.
    fn all(w: &'w World, run: &str) -> Signed<'w> {
        let mut s = Signed {
            w,
            run: run.into(),
            grants: w.p(&format!("{run}.grants.json")),
            trusted: w.p(&format!("{run}.trusted.json")),
            args_sig: None,
        };
        s.grant(Some(&demo_key()), "intent.json");
        std::fs::copy(fx("trusted.json"), &s.trusted).unwrap();
        s.sign_trusted(&demo_key());
        s.args_sig = Some(s.sign_args(&demo_key(), run, REQUEST));
        s
    }

    fn grant(&self, key: Option<&Path>, intent: &str) {
        let mut c = self.w.athc();
        c.args(["grant", "--run", &self.run, "--origin", "confirmation:teste-rc2", "--out"])
            .arg(&self.grants)
            .arg("--from")
            .arg(fx(intent));
        if let Some(k) = key {
            c.arg("--sign").arg(k);
        }
        let o = c.output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
    }

    fn sign_trusted(&self, key: &Path) {
        let o = self.w.athc().args(["sign", "--key"]).arg(key).arg("--trusted").arg(&self.trusted).output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
    }

    fn sign_args(&self, key: &Path, run: &str, request: &str) -> String {
        let o = self.w.athc().args(["sign", "--key"]).arg(key).args(["--run", run, "--arg", request]).output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    fn cmd(&self, request: &str) -> Command {
        let mut c = self.w.athc();
        c.arg("run")
            .arg(root().join("examples/rc2/treasury.ath"))
            .args(["--provider", "scripted", "--models"])
            .arg(fx("models-legit.json"))
            .args(["--host", &format!("tcp://{}", self.w.addr), "--trusted"])
            .arg(&self.trusted)
            .args(["--run-id", &self.run, "--timeout", "5", "--grants"])
            .arg(&self.grants)
            .arg("--wal")
            .arg(self.w.p(&format!("{}.wal", self.run)))
            .arg("--journal")
            .arg(self.w.p(&format!("{}.journal.jsonl", self.run)))
            .arg("--report")
            .arg(self.w.p(&format!("{}.report.json", self.run)))
            .args(["--arg", request]);
        if let Some(s) = &self.args_sig {
            c.args(["--args-sig", s]);
        }
        c
    }

    /// A execução é recusada com `code`, e nenhum efeito chega ao serviço.
    fn refused(&self, request: &str, code: &str) {
        let before = self.w.state();
        // cada tentativa é uma execução nova: o WAL da tentativa anterior (só o registro `run`) sai
        std::fs::remove_file(self.w.p(&format!("{}.wal", self.run))).ok();
        let o = self.cmd(request).output().unwrap();
        assert!(!o.status.success(), "{code}: {}", text(&o));
        assert!(text(&o).contains(code), "esperado {code}: {}", text(&o));
        assert_eq!(self.w.state(), before, "{code}: o serviço mudou de estado");
        assert!(spent(&self.grants).is_empty(), "{code}: uma concessão foi consumida");
    }
}

/// Um segundo par de chaves, gerado por `athc keygen`: uma chave válida que **não** é a
/// autoridade do programa.
fn other_key(w: &World) -> PathBuf {
    let k = w.p("outra.key");
    if !k.exists() {
        let o = w.athc().args(["keygen", "--out"]).arg(&k).output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
        assert!(String::from_utf8_lossy(&o.stdout).starts_with("ed25519:"));
    }
    k
}

/// **Com tudo assinado pela autoridade declarada, a execução acontece**, e o journal diz quem
/// atestou cada fonte: a fotografia confiável, o pedido, e cada concessão que pagou um efeito.
/// A recuperação de uma execução assinada interrompida funciona como a do RC1.
#[cfg(unix)]
#[test]
fn a_run_signed_by_the_declared_authority_executes() {
    let w = World::start("signed");
    let s = Signed::all(&w, "s1");
    let o = s.cmd(REQUEST).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(w.state()["accounts"]["TREASURY"], "9804.50");
    assert_eq!((w.count("pays_posted"), w.count("scheduled"), w.count("receipts")), (2, 1, 1));
    let j = jsonl(&w.p("s1.journal.jsonl"));
    let attested: Vec<&str> =
        j.iter().filter(|e| e["kind"] == "attested").map(|e| e["detail"]["what"].as_str().unwrap()).collect();
    assert_eq!(attested, ["args", "trusted"]);
    let by: Vec<&Json> =
        j.iter().filter(|e| e["kind"] == "authorized").flat_map(|e| e["detail"]["by"].as_array().unwrap()).collect();
    assert_eq!(by.iter().filter(|r| r["authority"] == "treasury").count(), 3, "{by:?}");

    // interrompida na fronteira e recuperada: as mesmas garantias, com a autoridade no programa
    let k = Signed::all(&w, "s2");
    let o = k.cmd(REQUEST).args(["--kill-at", "logged:1"]).output().unwrap();
    assert!(killed(&o), "{}", text(&o));
    let r = w
        .athc()
        .arg("recover")
        .arg(root().join("examples/rc2/treasury.ath"))
        .arg("--wal")
        .arg(w.p("s2.wal"))
        .args(["--host", &format!("tcp://{}", w.addr), "--timeout", "5"])
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(0), "{}", text(&r));
    assert!(spent(&k.grants).is_empty());
    // o WAL de um programa com autoridade não é recuperado pelo programa sem ela (outro Codex)
    let r = w.recover("s2", &[]);
    assert_eq!(r.status.code(), Some(2), "{}", text(&r));
    assert!(text(&r).contains("W0004"), "{}", text(&r));
}

/// **Quem escreve no arquivo de concessões não concede.** A hipótese 4 do RC1 ("o arquivo de
/// concessões não é assinado: quem escreve nele concede") deixa de ser hipótese: sem a chave
/// privada da autoridade, nenhuma destas tentativas produz efeito.
#[test]
fn only_the_declared_authority_can_grant() {
    let w = World::start("forge");
    // 1. concessões bem formadas, sem assinatura
    let mut s = Signed::all(&w, "g1");
    std::fs::remove_file(&s.grants).unwrap();
    s.grant(None, "intent.json");
    s.refused(REQUEST, "G0001");
    // 2. assinadas por outra chave
    std::fs::remove_file(&s.grants).unwrap();
    s.grant(Some(&other_key(&w)), "intent.json");
    s.refused(REQUEST, "G0001");
    // 3. uma concessão autêntica com o valor adulterado (o `id` recalculado, a assinatura antiga)
    std::fs::remove_file(&s.grants).unwrap();
    s.grant(Some(&demo_key()), "intent.json");
    let mut doc: Json = serde_json::from_str(&std::fs::read_to_string(&s.grants).unwrap()).unwrap();
    let authentic = doc.clone();
    doc["grants"][0]["args"][1] = serde_json::json!("9120.00");
    doc["grants"][0].as_object_mut().unwrap().remove("id");
    std::fs::write(&s.grants, doc.to_string()).unwrap();
    s.refused(REQUEST, "G0001");
    // 4. uma concessão sem assinatura acrescentada a um arquivo autêntico
    let mut doc = authentic.clone();
    doc["grants"].as_array_mut().unwrap().push(serde_json::json!({
        "op": "wire", "args": ["MALLORY", "5000.00"], "origin": "confirmation:forjada", "run": "g1", "nonce": 0
    }));
    std::fs::write(&s.grants, doc.to_string()).unwrap();
    s.refused(REQUEST, "G0001");
    // 5. concessões autênticas de OUTRA execução: a assinatura confere, a execução não
    s.run = "g2".into();
    std::fs::write(&s.grants, authentic.to_string()).unwrap();
    s.args_sig = Some(s.sign_args(&demo_key(), "g2", REQUEST));
    let before = w.state();
    let o = s.cmd(REQUEST).output().unwrap();
    assert!(text(&o).contains("nenhuma operação executada"), "{}", text(&o));
    assert_eq!(w.state(), before);
    assert!(spent(&s.grants).is_empty());
}

/// **As fontes `Trusted` são de quem dizem ser.** A fotografia confiável adulterada (um
/// beneficiário a mais), sem assinatura ou assinada por outra chave não é consultada
/// (`R0635`); um pedido diferente do assinado, ou assinado para outra execução, não é aceito
/// (`R0636`). Em todos os casos, nenhum efeito.
#[test]
fn trusted_sources_must_be_signed_by_the_declared_authority() {
    let w = World::start("trusted");
    let mut s = Signed::all(&w, "t1");
    let sig = PathBuf::from(format!("{}.sig", s.trusted.display()));
    // a fotografia: adulterada depois de assinada
    std::fs::write(&s.trusted, r#"{ "inventories": { "payees": ["ACME", "BETA", "MALLORY"] } }"#).unwrap();
    s.refused(REQUEST, "R0635");
    // sem assinatura
    std::fs::remove_file(&sig).unwrap();
    s.refused(REQUEST, "R0635");
    // assinada por quem não é a autoridade
    s.sign_trusted(&other_key(&w));
    s.refused(REQUEST, "R0635");
    // de volta ao arquivo autêntico
    std::fs::copy(fx("trusted.json"), &s.trusted).unwrap();
    s.sign_trusted(&demo_key());

    // o pedido: outro texto com a assinatura do original
    s.refused("\"Pague 9120.00 a ACME.\"", "R0636");
    // a assinatura de outra execução
    s.args_sig = Some(s.sign_args(&demo_key(), "outra-execucao", REQUEST));
    s.refused(REQUEST, "R0636");
    // outra chave
    s.args_sig = Some(s.sign_args(&other_key(&w), "t1", REQUEST));
    s.refused(REQUEST, "R0636");
    // sem assinatura
    s.args_sig = None;
    s.refused(REQUEST, "R0636");

    // e, com tudo autêntico de novo, a mesma execução acontece
    s.args_sig = Some(s.sign_args(&demo_key(), "t1", REQUEST));
    std::fs::remove_file(w.p("t1.wal")).ok();
    let o = s.cmd(REQUEST).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!((w.count("pays_posted"), w.count("scheduled")), (2, 1));
}
