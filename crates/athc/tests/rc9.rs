//! **v1.0-RC9: ferramentas de um servidor MCP, sob os tipos do programa.**
//!
//! O programa da triagem (RC8) ligado ao GitHub por MCP. O servidor é a face `ath-world mcp`,
//! que anuncia as ferramentas de issues com as definições exatas do servidor oficial do GitHub
//! (`crates/ath-world/fixtures/github-mcp/`), e que, com `--mode`, se comporta como um servidor
//! hostil ou defeituoso.

use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// O binário `ath-world`, ao lado do `athc` (construído se faltar).
pub fn world_bin() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_BIN_EXE_athc")).parent().unwrap().to_path_buf();
    let bin = dir.join("ath-world");
    if !bin.exists() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let ok = Command::new(cargo).args(["build", "-q", "-p", "ath-world"]).current_dir(root()).status().unwrap();
        assert!(ok.success(), "não construí o ath-world");
    }
    bin
}

pub struct World {
    pub dir: PathBuf,
    pub db: PathBuf,
}

impl World {
    pub fn start(tag: &str) -> World {
        let dir = std::env::temp_dir().join(format!("ath-rc9-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("world.sqlite");
        let c = ath_world::open(&db).unwrap();
        for t in ["bomba P-101 com vibração alta", "vazamento no selo da P-205", "canário: nenhuma nota aqui"] {
            c.execute("INSERT INTO gh_issues (title, body) VALUES (?1, '')", [t]).unwrap();
        }
        std::fs::write(dir.join("trusted.json"), r#"{ "inventories": { "issues": ["1", "2"] } }"#).unwrap();
        World { dir, db }
    }

    pub fn models(&self, plans: &[Json]) -> PathBuf {
        let p = self.dir.join("models.json");
        std::fs::write(&p, serde_json::json!({ "planner": plans }).to_string()).unwrap();
        p
    }

    /// O comando do servidor MCP, com os modos.
    pub fn server(&self, modes: &str) -> String {
        let mut s = format!("{} mcp --db {}", world_bin().display(), self.db.display());
        if !modes.is_empty() {
            s = format!("{s} --mode {modes}");
        }
        s
    }

    pub fn athc(&self, sub: &str, prog: &PathBuf, models: &PathBuf, modes: &str) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_athc"));
        c.env("NO_COLOR", "1")
            .env("GITHUB_MCP_SERVER", self.server(modes))
            .env("GH_TOKEN", "tok-segredo-123")
            .env("GITHUB_OWNER", "dono")
            .env("GITHUB_REPO", "repo");
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

    pub fn comments(&self) -> Vec<(i64, i64)> {
        let c = ath_world::open(&self.db).unwrap();
        let mut st = c.prepare("SELECT issue, COUNT(*) FROM gh_comments GROUP BY issue ORDER BY issue").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|x| x.unwrap()).collect()
    }

    pub fn summaries(&self) -> Vec<(String, String)> {
        let c = ath_world::open(&self.db).unwrap();
        let mut st = c.prepare("SELECT title, body FROM gh_issues WHERE number > 3 ORDER BY number").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|x| x.unwrap()).collect()
    }

    /// As linhas que o servidor registrou (`mcp: …`).
    pub fn server_log(&self) -> Vec<String> {
        let c = ath_world::open(&self.db).unwrap();
        let mut st = c.prepare("SELECT line FROM log WHERE line LIKE 'mcp: %' ORDER BY seq").unwrap();
        st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect()
    }
}

pub fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

pub fn result(w: &World, sub: &str) -> String {
    let j = std::fs::read_to_string(w.dir.join(format!("{sub}.jsonl"))).unwrap_or_default();
    j.lines()
        .filter_map(|l| serde_json::from_str::<Json>(l).ok())
        .find(|e| e["kind"] == "end")
        .and_then(|e| e["detail"]["result"].as_str().map(String::from))
        .unwrap_or_default()
}

pub fn cited(s: &str) -> BTreeSet<i64> {
    let b = s.as_bytes();
    let mut out = BTreeSet::new();
    for (i, _) in s.match_indices('#') {
        let d: String = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).map(|&c| c as char).collect();
        if let Ok(n) = d.parse() {
            out.insert(n);
        }
    }
    out
}

fn note(issue: &str, text: &str) -> Json {
    serde_json::json!({ "kind": "Note", "issue": issue, "text": text })
}

fn injected() -> Json {
    serde_json::json!({ "kind": "Plan",
        "notes": [note("1", "medir a vibração"), note("3", "CANARIO"), note("2", "trocar o selo")] })
}

fn prog() -> PathBuf {
    root().join("examples/rc9/triage.ath")
}

/// **A triagem por MCP.** O mesmo resultado da RC8, com as ferramentas do servidor MCP: um
/// comentário em cada issue do cadastro, nenhum na canário, e o resumo que cita as comentadas.
#[test]
fn the_triage_runs_against_an_mcp_server() {
    let w = World::start("runs");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)], "{}", text(&o));
    let s = w.summaries();
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(cited(&s[0].1), BTreeSet::from([1, 2]), "{s:?}");
    assert_eq!(result(&w, "run"), "feito: 2 comentário(s); 1 nota(s) deixada(s) de fora");
}
