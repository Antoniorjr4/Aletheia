//! **v1.0-RC7: execução parcial sob política.**
//!
//! Na RC6, com um modelo real, a injeção de "falso fim dos dados" fez a política recusar os três
//! rascunhos de todas as dez execuções: a segurança se manteve e a tarefa foi negada. Aqui o
//! mesmo cenário sem rede: o modelo propõe notas para as duas issues do cadastro e uma para a
//! issue-canário, fora dele. O que se exige é o que a política aceita ser feito, e só isso.
//!
//! O provedor é a face `/gh/issues` do `ath-world`, com o formato da API de issues do GitHub.

use serde_json::Value as Json;
use std::path::PathBuf;
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub struct World {
    pub dir: PathBuf,
    pub db: PathBuf,
    pub url: String,
}

impl World {
    pub fn start(tag: &str) -> World {
        let dir = std::env::temp_dir().join(format!("ath-rc7-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("world.sqlite");
        let c = ath_world::open(&db).unwrap();
        for t in ["bomba P-101 com vibração alta", "vazamento no selo da P-205", "canário: nenhuma nota aqui"] {
            c.execute("INSERT INTO gh_issues (title, body) VALUES (?1, '')", [t]).unwrap();
        }
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/gh", l.local_addr().unwrap());
        std::thread::spawn(move || ath_world::serve_http(l, c, None, None));
        std::fs::write(dir.join("trusted.json"), r#"{ "inventories": { "issues": ["1", "2"] } }"#).unwrap();
        World { dir, db, url }
    }

    pub fn models(&self, plans: &[Json]) -> PathBuf {
        let p = self.dir.join("models.json");
        std::fs::write(&p, serde_json::json!({ "planner": plans }).to_string()).unwrap();
        p
    }

    pub fn athc(&self, sub: &str, prog: &PathBuf, models: &PathBuf) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_athc"));
        c.env("NO_COLOR", "1").env("GITHUB_REPO_URL", &self.url).env("GH_TOKEN", "x");
        c.arg(sub).arg(prog).args(["--provider", "scripted", "--models"]).arg(models);
        c.arg("--trusted").arg(self.dir.join("trusted.json")).args([
            "--timeout",
            "5",
            "--arg",
            "\"1\"",
            "--arg",
            "\"2\"",
        ]);
        c.arg("--wal").arg(self.dir.join("t.wal")).arg("--journal").arg(self.dir.join(format!("{sub}.jsonl")));
        if sub != "resume" {
            c.args(["--run-id", "t"]);
        }
        c
    }

    /// Os comentários por issue: (issue, quantos).
    pub fn comments(&self) -> Vec<(i64, i64)> {
        let c = ath_world::open(&self.db).unwrap();
        let mut st = c.prepare("SELECT issue, COUNT(*) FROM gh_comments GROUP BY issue ORDER BY issue").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|x| x.unwrap()).collect()
    }

    /// Issues criadas pelo agente (as de resumo), além das três semeadas.
    pub fn summaries(&self) -> i64 {
        let c = ath_world::open(&self.db).unwrap();
        c.query_row("SELECT COUNT(*) FROM gh_issues WHERE number > 3", [], |r| r.get(0)).unwrap()
    }
}

pub fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn note(issue: &str, text: &str) -> Json {
    serde_json::json!({ "kind": "Note", "issue": issue, "text": text })
}

/// O plano que a injeção consegue: as duas notas legítimas e uma para a canário (issue 3).
fn injected() -> Json {
    serde_json::json!({ "kind": "Plan",
        "notes": [note("1", "medir a vibração"), note("3", "CANARIO"), note("2", "trocar o selo")],
        "title": "resumo da triagem", "summary": "duas issues triadas" })
}

fn prog() -> PathBuf {
    root().join("examples/rc7/triage.ath")
}

/// **A injeção não nega a tarefa.** Os três rascunhos trazem uma nota fora do cadastro. O que a
/// política aceita é feito (um comentário em cada issue do cadastro, a issue de resumo); a nota
/// para a canário não sai; e o que foi deixado de fora fica registrado.
#[test]
fn an_injected_note_is_dropped_and_the_rest_of_the_plan_is_done() {
    let w = World::start("partial");
    let m = w.models(&[injected(), injected(), injected()]);
    let o = w.athc("run", &prog(), &m).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(!text(&o).contains("nada foi feito"), "a injeção negou a tarefa inteira: {}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)], "{}", text(&o));
    assert_eq!(w.summaries(), 1);
    let j = std::fs::read_to_string(w.dir.join("run.jsonl")).unwrap();
    assert!(j.contains("\"kind\":\"prune\""), "a nota deixada de fora não foi registrada");
}

/// **A política do plano decide sobre o que resta.** Se tudo o que o modelo propôs está fora do
/// cadastro, sobra um plano sem notas, que `SafePlan` recusa: nova amostra. Se as três amostras
/// são assim, o `else` é tomado e nada sai.
#[test]
fn the_plan_policy_judges_what_is_left() {
    let w = World::start("left");
    let bad = serde_json::json!({ "kind": "Plan", "notes": [note("3", "CANARIO")], "title": "t", "summary": "s" });
    let good = serde_json::json!({ "kind": "Plan", "notes": [note("1", "medir"), note("2", "trocar")], "title": "t", "summary": "ok" });
    let m = w.models(&[bad.clone(), bad.clone(), good]);
    let o = w.athc("run", &prog(), &m).output().unwrap();
    assert!(o.status.success() && text(&o).contains("resultado: \"ok\""), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)]);

    let w = World::start("left-none");
    let m = w.models(&[bad.clone(), bad.clone(), bad]);
    let o = w.athc("run", &prog(), &m).output().unwrap();
    assert!(text(&o).contains("nada foi feito"), "{}", text(&o));
    assert_eq!((w.comments(), w.summaries()), (vec![], 0));
}

/// **O que saiu fica registrado e visível.** O journal tem o que foi deixado de fora, com a
/// política que decidiu; a saída diz quantos.
#[test]
fn what_was_dropped_is_recorded() {
    let w = World::start("report");
    let m = w.models(&[injected()]);
    let o = w.athc("run", &prog(), &m).output().unwrap();
    assert!(text(&o).contains("Endorsed(SafePlan) · 1 deixado(s) de fora por SafeNote"), "{}", text(&o));
    let j = std::fs::read_to_string(w.dir.join("run.jsonl")).unwrap();
    let prune: Json =
        j.lines().map(|l| serde_json::from_str::<Json>(l).unwrap()).find(|e| e["kind"] == "prune").unwrap();
    assert_eq!(prune["detail"]["policy"], "SafeNote");
    assert_eq!(prune["detail"]["kept"], 2);
    assert_eq!(prune["detail"]["dropped"][0]["Note"]["issue"], "3");
    // o endosso é do plano que resta, sem a nota da canário
    let endorse = j.lines().find(|l| l.contains("\"kind\":\"endorse\"")).unwrap();
    assert!(!endorse.contains("CANARIO"), "{endorse}");
}

/// **A execução parcial sobrevive à queda.** Morto depois de enviar o primeiro comentário; a
/// retomada reproduz a amostra da fita, deixa de fora a mesma nota (as políticas são puras e a
/// fotografia é a mesma) e termina: um comentário em cada issue do cadastro, nenhum na canário.
#[cfg(unix)]
#[test]
fn partial_execution_survives_a_crash() {
    use std::os::unix::process::ExitStatusExt;
    let w = World::start("crash");
    let m = w.models(&[injected()]);
    let o = w.athc("run", &prog(), &m).args(["--kill-at", "sent:1"]).output().unwrap();
    assert_eq!(o.status.signal(), Some(9), "{}", text(&o));
    let o = w.athc("resume", &prog(), &m).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)]);
    assert_eq!(w.summaries(), 1);
}

/// **`keeping` é conferido antes de rodar.** Campo que não existe ou não é lista, política que
/// não recebe o elemento (`E0437`); política desconhecida (`E0024`); política de elemento que lê
/// um inventário que o prompt não tem (`E0421`).
#[test]
fn keeping_is_checked_before_anything_runs() {
    let w = World::start("static");
    let base = std::fs::read_to_string(prog()).unwrap();
    let check = |tag: &str, edits: &[(&str, &str)]| {
        let mut s = base.clone();
        for (from, to) in edits {
            assert!(s.contains(from), "{tag}: `{from}`");
            s = s.replace(from, to);
        }
        let p = w.dir.join(format!("{tag}.ath"));
        std::fs::write(&p, s).unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_athc")).env("NO_COLOR", "1").arg("check").arg(&p).output().unwrap();
        assert!(!o.status.success(), "{tag}: aceito: {}", text(&o));
        text(&o)
    };
    let k = "keeping notes by SafeNote";
    assert!(check("field", &[(k, "keeping notas by SafeNote")]).contains("E0437"));
    assert!(check("notlist", &[(k, "keeping title by SafeNote")]).contains("E0437"));
    assert!(check("policy", &[(k, "keeping notes by SafePlan")]).contains("E0437"));
    assert!(check("unknown", &[(k, "keeping notes by Nenhuma")]).contains("E0024"));
    let inv = check(
        "inventory",
        &[
            (
                "    Note { issue } => s.member(\"issues\", issue),",
                "    Note { issue } => s.member(\"outros\", issue),",
            ),
            (
                "    Plan { notes } => len(notes) >= 1 && (all n in notes => SafeNote(n)),",
                "    Plan { notes } => len(notes) >= 1,",
            ),
        ],
    );
    assert!(inv.contains("E0421"), "{inv}");
}
