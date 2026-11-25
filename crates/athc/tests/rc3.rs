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
//!
//! Os quatro testes que os reproduzem falham no commit que os introduziu, contra o programa do
//! RC2 (`docs/evidence/rc3-contraexemplos-no-rc2.txt`). Aqui rodam contra
//! `examples/rc3/treasury.ath`, cuja autoridade declara escopo, validade e registro ancorado, e
//! passam. O programa do RC2 continua com a semântica do RC2 (`tests/rc2.rs`, inalterado).

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

/// Dois serviços `ath-world` próprios, em bancos separados: o provedor (o banco) e a âncora.
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
        let dir = std::env::temp_dir().join(format!("ath-rc3-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("bank.sqlite");
        let c = ath_world::open(&db).unwrap();
        let seed: Json = serde_json::from_str(&std::fs::read_to_string(fx("seed.json")).unwrap()).unwrap();
        ath_world::seed(&c, &seed).unwrap();
        drop(c);
        let anchor_db = dir.join("anchor.sqlite");
        let (addr, anchor) = (serve(&db), serve(&anchor_db));
        World { dir, db, addr, anchor_db, anchor }
    }

    /// A marca da âncora (quantos consumos), lida do banco dela.
    fn anchored(&self) -> u64 {
        let c = ath_world::open(&self.anchor_db).unwrap();
        c.query_row("SELECT COALESCE(MAX(seq), 0) FROM anchors", [], |r| r.get(0)).unwrap()
    }

    fn requests_for(&self, key: &str) -> usize {
        let c = ath_world::open(&self.db).unwrap();
        let mut st = c.prepare("SELECT line FROM log ORDER BY seq").unwrap();
        let v: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
        v.iter().filter(|l| l.contains(key)).count()
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

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

fn rc3_prog() -> PathBuf {
    root().join("examples/rc3/treasury.ath")
}

/// O carimbo de uma assinatura: para que programa, emitida quando, por quanto tempo.
#[derive(Clone)]
struct Stamp {
    program: PathBuf,
    issued: u64,
    ttl: u64,
}

impl Stamp {
    fn fresh(program: &Path) -> Stamp {
        Stamp { program: program.to_path_buf(), issued: now(), ttl: 600 }
    }

    fn args(&self, c: &mut Command) {
        c.arg("--program").arg(&self.program).args([
            "--issued",
            &self.issued.to_string(),
            "--ttl",
            &self.ttl.to_string(),
        ]);
    }
}

/// Uma execução do programa `prog` com as três fontes assinadas pela autoridade de demonstração
/// **para este programa, esta execução e um prazo**, e o registro de consumo ancorado.
struct Run<'w> {
    w: &'w World,
    prog: PathBuf,
    run: String,
    grants: PathBuf,
    trusted: PathBuf,
    args_sig: PathBuf,
    anchor: Option<String>,
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
            anchor: Some(format!("tcp://{}", w.anchor)),
        };
        let st = Stamp::fresh(prog);
        r.grant(&demo_key(), "intent.json", &st);
        std::fs::copy(fx("trusted.json"), &r.trusted).unwrap();
        r.sign_trusted(&demo_key(), run, Some(&st));
        r.sign_args(&demo_key(), run, Some(&st));
        r
    }

    fn grant(&self, key: &Path, intent: &str, st: &Stamp) {
        let _ = std::fs::remove_file(&self.grants);
        let mut c = self.w.athc();
        c.args(["grant", "--run", &self.run, "--origin", "confirmation:teste-rc3", "--out"])
            .arg(&self.grants)
            .arg("--from")
            .arg(fx(intent))
            .arg("--sign")
            .arg(key);
        st.args(&mut c);
        self.w.ok(&mut c);
    }

    /// Assina a fotografia para a execução `run`; sem carimbo, no formato do RC2.
    fn sign_trusted(&self, key: &Path, run: &str, st: Option<&Stamp>) {
        let mut c = self.w.athc();
        c.args(["sign", "--key"]).arg(key).arg("--trusted").arg(&self.trusted);
        if let Some(st) = st {
            c.args(["--run", run]);
            st.args(&mut c);
        }
        self.w.ok(&mut c);
    }

    fn sign_args(&self, key: &Path, run: &str, st: Option<&Stamp>) {
        let mut c = self.w.athc();
        c.args(["sign", "--key"]).arg(key).args(["--run", run, "--arg", REQUEST, "--out"]).arg(&self.args_sig);
        if let Some(st) = st {
            st.args(&mut c);
        }
        self.w.ok(&mut c);
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
        if let Some(a) = &self.anchor {
            c.args(["--anchor", a]);
        }
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

    /// A execução é recusada com `code` (e, se dado, o motivo), sem efeito no serviço, sem
    /// consumo e sem avanço da âncora.
    fn refused(&self, wal: &str, code: &str, why: &str) {
        let (before, mark) = (self.w.state(), self.w.anchored());
        let lines = std::fs::read_to_string(self.ledger()).unwrap_or_default();
        let _ = std::fs::remove_file(self.w.p(&format!("{wal}.wal")));
        let o = self.cmd(wal).output().unwrap();
        assert!(!o.status.success(), "esperado {code}: {}", text(&o));
        assert!(text(&o).contains(code) && text(&o).contains(why), "esperado {code} ({why}): {}", text(&o));
        assert_eq!(self.w.state(), before, "{code}: o serviço mudou de estado");
        assert_eq!(std::fs::read_to_string(self.ledger()).unwrap_or_default(), lines, "{code}: o registro mudou");
        assert_eq!(self.w.anchored(), mark, "{code}: a âncora avançou");
    }
}

/// Tira as últimas `n` linhas do registro de consumo (o retrocesso que um backup restaurado,
/// ou alguém com acesso ao disco, produz).
fn truncate_ledger(ledger: &Path, n: usize) {
    let t = std::fs::read_to_string(ledger).unwrap();
    let lines: Vec<&str> = t.lines().collect();
    let keep = lines.len() - n;
    std::fs::write(ledger, lines[..keep].iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
}

fn lines(p: &Path) -> Vec<String> {
    std::fs::read_to_string(p).unwrap().lines().map(String::from).collect()
}

fn write_lines(p: &Path, ls: &[String]) {
    std::fs::write(p, ls.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
}

/// **A história inteira, ligada por hashes.** Uma execução legítima: cada efeito tem a concessão
/// assinada para este programa, a intenção no WAL que a cita, o consumo no registro que cita a
/// intenção e o programa, a âncora na marca do registro, e o desfecho no WAL que cita o consumo.
#[test]
fn every_effect_has_a_verifiable_history() {
    let w = World::start("history");
    let r = Run::signed(&w, &rc3_prog(), "r");
    let o = w.ok(&mut r.cmd("r"));
    assert_eq!((w.count("pays_posted"), w.count("scheduled"), w.count("receipts")), (2, 1, 1), "{}", text(&o));
    let grants: Json = serde_json::from_str(&std::fs::read_to_string(&r.grants).unwrap()).unwrap();
    let program = grants["grants"][0]["program"].as_str().unwrap().to_string();
    let (wal, ledger) = (jsonl(&w.p("r.wal")), jsonl(&r.ledger()));
    assert_eq!(wal[0]["detail"]["program"], program.as_str(), "o WAL é deste programa");
    assert_eq!(ledger.len(), 3);
    assert_eq!(w.anchored(), 3, "a âncora está na marca do registro");
    let mut prev = "0".repeat(64);
    for (n, line) in ledger.iter().enumerate() {
        // o consumo: encadeado, deste programa e desta execução
        assert_eq!((line["seq"].as_u64(), line["prev"].as_str()), (Some(n as u64), Some(prev.as_str())));
        assert_eq!((line["program"].as_str(), line["run"].as_str()), (Some(program.as_str()), Some("r")));
        prev = line["hash"].as_str().unwrap().to_string();
        // a concessão: assinada, com carimbo, para este programa
        let id = line["grants"][0].as_str().unwrap();
        let g = grants["grants"].as_array().unwrap().iter().find(|g| g["id"] == id).expect("concessão do registro");
        assert!(g["sig"].is_string() && g["expires"].as_u64() > g["issued"].as_u64());
        // a intenção do WAL: cita a concessão, e o consumo cita o hash dela
        let intent =
            wal.iter().find(|e| e["kind"] == "intent" && e["hash"] == line["intent"]).expect("intenção do consumo");
        assert_eq!(intent["detail"]["grants"][0], id);
        assert_eq!(intent["detail"]["key"], line["key"]);
        // o desfecho do WAL: cita o consumo
        let close = wal.iter().find(|e| e["kind"] == "close" && e["detail"]["key"] == line["key"]).expect("desfecho");
        assert_eq!(close["detail"]["spend"], line["hash"], "o desfecho cita o consumo");
        assert_eq!(close["detail"]["status"], "done");
    }
    // o journal diz quem atestou cada fonte e até quando
    let j = jsonl(&w.p("r.journal.jsonl"));
    let att: Vec<&Json> = j.iter().filter(|e| e["kind"] == "attested").collect();
    assert_eq!(att.len(), 2);
    assert!(att
        .iter()
        .all(|e| e["detail"]["authority"] == "treasury" && e["detail"]["expires"].as_u64() > Some(now())));
}

/// **Contraexemplo 1, fechado.** Uma execução completa gasta as três concessões. O registro de
/// consumo retrocede de quatro maneiras. Uma nova execução é recusada com `L0002` antes de
/// qualquer efeito; com o registro verdadeiro de volta, as concessões continuam gastas.
#[test]
fn a_rolled_back_ledger_does_not_reinstate_spent_grants() {
    for tag in ["tail", "two", "file", "empty"] {
        let w = World::start(&format!("rollback-{tag}"));
        let r = Run::signed(&w, &rc3_prog(), "r");
        w.ok(&mut r.cmd("first"));
        assert_eq!((w.count("pays_posted"), w.count("scheduled")), (2, 1));
        let real = std::fs::read(r.ledger()).unwrap();
        match tag {
            "tail" => truncate_ledger(&r.ledger(), 1),
            "two" => truncate_ledger(&r.ledger(), 2),
            "file" => std::fs::remove_file(r.ledger()).unwrap(),
            _ => std::fs::write(r.ledger(), "").unwrap(),
        }
        let before = std::fs::read(r.ledger()).unwrap_or_default();
        let o = r.cmd("second").output().unwrap();
        assert!(!o.status.success() && text(&o).contains("L0002"), "{tag}: {}", text(&o));
        assert_eq!(
            (w.count("pays_posted"), w.count("scheduled")),
            (2, 1),
            "{tag}: um efeito foi repetido\n{}",
            text(&o)
        );
        assert_eq!(w.state()["accounts"]["TREASURY"], "9804.50", "{tag}");
        assert_eq!(std::fs::read(r.ledger()).unwrap_or_default(), before, "{tag}: o registro retrocedido foi escrito");
        assert_eq!(w.anchored(), 3, "{tag}");
        // o registro verdadeiro de volta: aceito, e as concessões continuam gastas
        std::fs::write(r.ledger(), &real).unwrap();
        let o = r.cmd("third").output().unwrap();
        assert!(text(&o).contains("nenhuma operação executada"), "{tag}: {}", text(&o));
        assert_eq!((w.count("pays_posted"), w.count("scheduled")), (2, 1), "{tag}");
    }
}

/// **Contraexemplo 2, fechado.** `SIGKILL` depois do envio do agendamento; a linha do consumo
/// some. A recuperação para com `L0002`, sem enviar e sem escrever no WAL: não conclui que a
/// operação "não saiu". Com o registro de volta, resolve por consulta, sem reenviar.
#[cfg(unix)]
#[test]
fn recovery_does_not_reinstate_a_grant_whose_effect_left() {
    let w = World::start("recover-rollback");
    let r = Run::signed(&w, &rc3_prog(), "r");
    let o = r.cmd("first").args(["--kill-at", "sent:3"]).output().unwrap();
    assert!(killed(&o), "{}", text(&o));
    assert_eq!(w.count("scheduled"), 1);
    let real = std::fs::read(r.ledger()).unwrap();
    truncate_ledger(&r.ledger(), 1);
    let wal = std::fs::read(w.p("first.wal")).unwrap();
    let rec = r.recover("first");
    assert_eq!(rec.status.code(), Some(2), "{}", text(&rec));
    assert!(text(&rec).contains("L0002"), "{}", text(&rec));
    assert_eq!(std::fs::read(w.p("first.wal")).unwrap(), wal, "a recuperação escreveu no WAL");
    let released = jsonl(&w.p("first.wal")).iter().any(|e| e["detail"]["evidence"] == "recovery:unspent");
    assert!(!released, "a recuperação liberou a concessão de um efeito que saiu\n{}", text(&rec));
    std::fs::write(r.ledger(), real).unwrap();
    let rec = r.recover("first");
    assert_eq!(rec.status.code(), Some(0), "{}", text(&rec));
    let close = jsonl(&w.p("first.wal"))
        .into_iter()
        .rev()
        .find(|e| e["kind"] == "close" && e["detail"]["key"] == "k3")
        .unwrap();
    assert_eq!(close["detail"]["evidence"], "recovery:status");
    assert_eq!(close["detail"]["spend"], jsonl(&r.ledger())[2]["hash"], "o desfecho recuperado cita o consumo");
    assert_eq!(w.count("scheduled"), 1, "nunca reenviado");
}

/// **Alterações no registro.** Trocar a concessão de uma linha, tirar uma linha do meio, trocar
/// duas de lugar ou reescrever o fim com outra linha (uma bifurcação bem encadeada): a cadeia
/// recusa as três primeiras (`L0001`) e a âncora recusa a última (`L0003`). Nenhum efeito.
#[test]
fn a_tampered_ledger_is_refused() {
    let w = World::start("tamper");
    let r = Run::signed(&w, &rc3_prog(), "r");
    w.ok(&mut r.cmd("first"));
    let real = lines(&r.ledger());
    let free = "0".repeat(64);
    let cases: Vec<(&str, &str, Vec<String>)> = vec![
        ("concessão trocada", "L0001", {
            let mut v = real.clone();
            let id = jsonl(&r.ledger())[1]["grants"][0].as_str().unwrap().to_string();
            v[1] = v[1].replace(&id, &free);
            v
        }),
        ("linha do meio removida", "L0001", vec![real[0].clone(), real[2].clone()]),
        ("linhas trocadas", "L0001", vec![real[1].clone(), real[0].clone(), real[2].clone()]),
        ("bifurcação", "L0003", {
            // a última linha reescrita para outra concessão, com `seq`, `prev` e `hash` corretos
            let mut e: Json = serde_json::from_str(&real[2]).unwrap();
            e.as_object_mut().unwrap().remove("hash");
            e["grants"] = serde_json::json!([free]);
            let h = blake3::hash(e.to_string().as_bytes()).to_hex().to_string();
            e["hash"] = serde_json::json!(h);
            vec![real[0].clone(), real[1].clone(), e.to_string()]
        }),
    ];
    for (tag, code, forged) in cases {
        write_lines(&r.ledger(), &forged);
        let o = r.cmd(&format!("t-{}", tag.len())).output().unwrap();
        assert!(!o.status.success() && text(&o).contains(code), "{tag}: esperado {code}\n{}", text(&o));
        assert_eq!((w.count("pays_posted"), w.count("scheduled")), (2, 1), "{tag}");
        assert_eq!(lines(&r.ledger()), forged, "{tag}: o registro adulterado foi escrito");
    }
}

/// **A queda entre o consumo e a âncora.** `SIGKILL` com o consumo gravado e a âncora ainda
/// atrás: nada chegou ao serviço. A recuperação adianta a âncora (o consumo é legítimo e é o
/// único à frente) e segue o protocolo do provedor; a concessão não volta a ficar livre, e o
/// registro e a âncora terminam na mesma marca.
#[cfg(unix)]
#[test]
fn a_crash_between_spend_and_anchor_is_rolled_forward() {
    for n in [1u32, 3] {
        let w = World::start(&format!("spent-{n}"));
        let r = Run::signed(&w, &rc3_prog(), "r");
        let o = r.cmd("r").args(["--kill-at", &format!("spent:{n}")]).output().unwrap();
        assert!(killed(&o), "{n}: {}", text(&o));
        assert_eq!(
            (lines(&r.ledger()).len() as u64, w.anchored()),
            (n as u64, n as u64 - 1),
            "{n}: registro um à frente"
        );
        assert_eq!(w.requests_for(&format!("r:k{n}")), 0, "{n}: a operação saiu antes de a âncora avançar");
        let rec = r.recover("r");
        assert_eq!(rec.status.code(), Some(0), "{n}: {}", text(&rec));
        assert_eq!((lines(&r.ledger()).len() as u64, w.anchored()), (n as u64, n as u64), "{n}: âncora adiantada");
        let close =
            jsonl(&w.p("r.wal")).into_iter().find(|e| e["kind"] == "close" && e["detail"]["key"] == format!("k{n}"));
        let ev = close.unwrap()["detail"]["evidence"].as_str().unwrap().to_string();
        assert_eq!(ev, if n == 1 { "recovery:resend" } else { "recovery:status" }, "{n}");
        // a concessão consumida não volta: uma nova execução não a encontra livre
        let o = r.cmd("again").output().unwrap();
        assert!(text(&o).contains("nenhuma operação executada"), "{n}: {}", text(&o));
        assert_eq!(lines(&r.ledger()).len() as u64, n as u64, "{n}");
    }
}

/// **Sem a âncora não se consome.** A autoridade declara `ledger = "anchored"`: sem `--anchor`
/// (`R0637`) ou com a âncora fora do ar (`L0005`), nenhum efeito; e uma execução ancorada não é
/// recuperada sem alcançar a âncora.
#[cfg(unix)]
#[test]
fn an_anchored_authority_does_not_spend_without_the_anchor() {
    let w = World::start("no-anchor");
    let mut r = Run::signed(&w, &rc3_prog(), "r");
    let real = r.anchor.take();
    r.refused("a", "R0637", "ancorado");
    r.anchor = Some("tcp://127.0.0.1:1".into());
    r.refused("b", "L0005", "âncora");
    r.anchor = real;
    let o = r.cmd("c").args(["--kill-at", "sent:1"]).output().unwrap();
    assert!(killed(&o), "{}", text(&o));
    let wal = std::fs::read(w.p("c.wal")).unwrap();
    let rec =
        r.w.athc()
            .arg("recover")
            .arg(&r.prog)
            .arg("--wal")
            .arg(w.p("c.wal"))
            .args(["--host", &format!("tcp://{}", w.addr), "--anchor", "tcp://127.0.0.1:1"])
            .output()
            .unwrap();
    assert_eq!(rec.status.code(), Some(2), "{}", text(&rec));
    assert!(text(&rec).contains("L0005"), "{}", text(&rec));
    assert_eq!(std::fs::read(w.p("c.wal")).unwrap(), wal);
    assert_eq!(r.recover("c").status.code(), Some(0));
}

/// **A corrida, com âncora.** Quatro processos da mesma execução: cada concessão é consumida uma
/// vez, o registro e a âncora terminam na mesma marca, e o serviço no estado de uma execução.
#[test]
fn concurrent_executors_share_one_anchored_history() {
    let w = World::start("race");
    let r = Run::signed(&w, &rc3_prog(), "r");
    for _ in 0..4 {
        ath_world::add_fault(&ath_world::open(&w.db).unwrap(), "statement", None, "hang", 700).unwrap();
    }
    let procs: Vec<_> = (0..4)
        .map(|i| {
            r.cmd(&format!("p{i}"))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let all: String =
        procs.into_iter().map(|p| text(&p.wait_with_output().unwrap())).collect::<Vec<_>>().join("\n---\n");
    assert_eq!((lines(&r.ledger()).len(), w.anchored()), (3, 3), "{all}");
    assert_eq!((w.count("pays_posted"), w.count("scheduled"), w.count("receipts")), (2, 1, 1), "{all}");
    assert_eq!(w.state()["accounts"]["TREASURY"], "9804.50", "{all}");
    w.ok(&mut r.cmd("depois"));
}

/// **Contraexemplo 3, fechado: frescor e escopo das fontes `Trusted`.** A fotografia e o pedido
/// só valem assinados para este programa, para esta execução, dentro do prazo, e com prazo que
/// não passe do `max_age` da autoridade. Em todos os casos recusados, nenhum efeito.
#[test]
fn trusted_sources_are_fresh_and_scoped() {
    let w = World::start("fresh");
    let old = Run::signed(&w, &rc3_prog(), "ontem");
    let r = Run::signed(&w, &rc3_prog(), "hoje");
    let good = (std::fs::read(format!("{}.sig", r.trusted.display())).unwrap(), std::fs::read(&r.args_sig).unwrap());
    let other = w.p("other.ath");
    std::fs::write(&other, std::fs::read_to_string(rc3_prog()).unwrap().replace("usd <= 0.10", "usd <= 0.11")).unwrap();
    let fresh = Stamp::fresh(&rc3_prog());
    let stamps: Vec<(&str, &str, Option<Stamp>)> = vec![
        ("expirada", "expirou", Some(Stamp { issued: now() - 7200, ..fresh.clone() })),
        ("do futuro", "ainda não é válida", Some(Stamp { issued: now() + 3600, ..fresh.clone() })),
        ("prazo maior que max_age", "max_age", Some(Stamp { ttl: 86_400, ..fresh.clone() })),
        ("outro programa", "outro programa", Some(Stamp::fresh(&other))),
        ("formato do RC2 (sem carimbo)", "exige assinatura ligada", None),
    ];
    // a fotografia
    for (tag, why, st) in &stamps {
        r.sign_trusted(&demo_key(), "hoje", st.as_ref());
        r.refused(&format!("f{}", tag.len()), "R0635", why);
    }
    r.sign_trusted(&demo_key(), "ontem", Some(&fresh));
    r.refused("f-run", "R0635", "outra execução");
    // a fotografia autêntica de ontem (o contraexemplo, literalmente)
    std::fs::copy(format!("{}.sig", old.trusted.display()), format!("{}.sig", r.trusted.display())).unwrap();
    r.refused("f-ontem", "R0635", "outra execução");
    std::fs::write(format!("{}.sig", r.trusted.display()), &good.0).unwrap();
    // o pedido
    for (tag, why, st) in &stamps {
        r.sign_args(&demo_key(), "hoje", st.as_ref());
        r.refused(&format!("a{}", tag.len()), "R0636", why);
    }
    std::fs::copy(&old.args_sig, &r.args_sig).unwrap();
    r.refused("a-ontem", "R0636", "sem a assinatura");
    std::fs::write(&r.args_sig, &good.1).unwrap();
    // com as duas fontes frescas de volta, a execução acontece
    w.ok(&mut r.cmd("ok"));
    assert_eq!((w.count("pays_posted"), w.count("scheduled")), (2, 1));
}

/// **Contraexemplo 4, fechado: a concessão é deste programa, tem prazo e tem escopo.**
#[test]
fn a_grant_is_bound_to_program_validity_and_scope() {
    let w = World::start("grant");
    let r = Run::signed(&w, &rc3_prog(), "r");
    let fresh = Stamp::fresh(&rc3_prog());
    // outro programa (outro Codex), mesma autoridade, mesma execução
    let other = w.p("other.ath");
    std::fs::write(&other, std::fs::read_to_string(rc3_prog()).unwrap().replace("usd <= 0.10", "usd <= 0.11")).unwrap();
    let for_rc3 = std::fs::read(&r.grants).unwrap();
    let o = Run::signed(&w, &other, "r");
    // (as fontes `Trusted` são as do outro programa; as concessões, as assinadas para o primeiro)
    std::fs::write(&o.grants, &for_rc3).unwrap();
    o.refused("other", "G0001", "outro programa");
    let r = Run::signed(&w, &rc3_prog(), "r");
    // expirada, e com prazo maior que o `max_age`
    r.grant(&demo_key(), "intent.json", &Stamp { issued: now() - 7200, ..fresh.clone() });
    r.refused("exp", "G0001", "expirou");
    r.grant(&demo_key(), "intent.json", &Stamp { ttl: 86_400, ..fresh.clone() });
    r.refused("age", "G0001", "max_age");
    // o carimbo adulterado depois de assinado (a validade esticada)
    r.grant(&demo_key(), "intent.json", &Stamp { issued: now() - 7200, ..fresh.clone() });
    let mut doc: Json = serde_json::from_str(&std::fs::read_to_string(&r.grants).unwrap()).unwrap();
    for g in doc["grants"].as_array_mut().unwrap() {
        g["expires"] = serde_json::json!(now() + 600);
    }
    std::fs::write(&r.grants, doc.to_string()).unwrap();
    r.refused("stretch", "G0001", "sem a assinatura");
    // escopo: duas autoridades; a que só concede pagamentos assina um agendamento
    let k2 = w.p("pagadora.key");
    let pk2 = String::from_utf8(w.ok(w.athc().args(["keygen", "--out"]).arg(&k2)).stdout).unwrap();
    let two = w.p("two.ath");
    let src = std::fs::read_to_string(rc3_prog()).unwrap().replace(
        "authority treasury {",
        &format!("authority pagadora {{\n  key = \"{}\"\n  ops = \"pay\"\n  max_age = 900\n  ledger = \"anchored\"\n}}\n\nauthority treasury {{", pk2.trim()),
    );
    std::fs::write(&two, src).unwrap();
    let t = Run::signed(&w, &two, "t");
    t.grant(&k2, "intent.json", &Stamp::fresh(&two));
    t.refused("scope", "G0001", "fora do escopo");
    // e, com as concessões frescas da autoridade certa, a execução acontece
    t.grant(&demo_key(), "intent.json", &Stamp::fresh(&two));
    w.ok(&mut t.cmd("ok"));
    assert_eq!((w.count("pays_posted"), w.count("scheduled")), (2, 1));
}
