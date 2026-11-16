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

fn journal(w: &World, sub: &str) -> Vec<Json> {
    std::fs::read_to_string(w.dir.join(format!("{sub}.jsonl")))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn prompt_hashes(w: &World, sub: &str) -> Vec<Json> {
    journal(w, sub).into_iter().filter(|e| e["kind"] == "infer").map(|e| e["detail"]["prompt_hash"].clone()).collect()
}

/// Uma variação do programa, executada contra o servidor (a conferência é na ligação).
fn variant(w: &World, tag: &str, edits: &[(&str, &str)]) -> PathBuf {
    let mut s = std::fs::read_to_string(prog()).unwrap();
    for (from, to) in edits {
        assert!(s.contains(from), "{tag}: `{from}`");
        s = s.replacen(from, to, 1);
    }
    let p = w.dir.join(format!("{tag}.ath"));
    std::fs::write(&p, s).unwrap();
    p
}

/// Nada saiu: nenhum comentário, nenhuma issue nova, nenhuma chamada de ferramenta no servidor.
fn nothing_happened(w: &World) {
    assert_eq!((w.comments(), w.summaries().len()), (vec![], 0));
    assert!(!w.server_log().iter().any(|l| l.contains("tools/call")), "{:?}", w.server_log());
}

/// **As duas eras do protocolo.** Um servidor que só conhece o aperto de mão `initialize`
/// (2025-11-25) não responde à sonda `server/discover`; o cliente cai para a era legada no mesmo
/// processo, e o resultado é o mesmo.
#[test]
fn a_legacy_server_works_the_same() {
    let w = World::start("legacy");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "legacy").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("protocolo 2025-11-25"), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)]);
    assert_eq!(cited(&w.summaries()[0].1), BTreeSet::from([1, 2]));
}

/// **O que o servidor diz de si não é instrução.** Descrições e `instructions` com uma injeção
/// não chegam a prompt algum: o prompt do modelo é o mesmo, byte a byte (o mesmo hash), e o
/// resultado também.
#[test]
fn what_the_server_says_about_itself_is_not_an_instruction() {
    let clean = World::start("clean");
    let o = clean.athc("run", &prog(), &clean.models(&[injected()]), "").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let w = World::start("poison");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "poison").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(prompt_hashes(&w, "run"), prompt_hashes(&clean, "run"));
    assert!(!prompt_hashes(&w, "run").is_empty());
    assert_eq!(w.comments(), clean.comments());
    let j = std::fs::read_to_string(w.dir.join("run.jsonl")).unwrap();
    assert!(!j.contains("ignore as instruções"), "a descrição envenenada entrou no journal");
}

/// **Uma anotação que admite perigo restringe.** O servidor diz que `issue_read` muda o mundo;
/// o programa a usa como leitura (`reversible`). A ligação é recusada antes de qualquer efeito.
#[test]
fn a_server_that_admits_danger_is_refused() {
    let w = World::start("lie-read");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "lie-read").output().unwrap();
    assert!(!o.status.success());
    assert!(text(&o).contains("R0647"), "{}", text(&o));
    nothing_happened(&w);
}

/// **Uma anotação que promete segurança não muda nada.** O servidor diz que `add_issue_comment`
/// é uma leitura idempotente; para o programa ela continua irrevogável, com chave e consulta. O
/// processo morre depois de aplicar o primeiro comentário: a consulta o acha, e ele não é
/// reenviado.
#[test]
fn a_server_that_promises_safety_changes_nothing() {
    let w = World::start("lie-write");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "lie-write,crash-after-apply").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)], "{}", text(&o));
}

/// **Deriva de esquema.** O servidor passou a declarar `html_url` inteiro; o programa espera
/// texto. Recusado na ligação, antes de qualquer efeito (`R0646`).
#[test]
fn a_tool_whose_schema_drifted_is_refused_before_any_effect() {
    let w = World::start("drift");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "drift").output().unwrap();
    assert!(!o.status.success());
    let t = text(&o);
    assert!(t.contains("R0646") && t.contains("html_url"), "{t}");
    nothing_happened(&w);
}

/// **A declaração do programa contra o esquema real.** Contra as definições do servidor do
/// GitHub: o `id` do comentário é texto (declarar `Int` é recusado); um argumento que o servidor
/// exige e não é enviado; um literal fora do `enum`; um caminho que não existe. Tudo antes de
/// qualquer efeito.
#[test]
fn the_declaration_is_checked_against_the_real_schemas() {
    type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);
    let cases: [Case; 4] = [
        (
            "id-int",
            &[
                ("k: own IdemKey) -> Text[..200]", "k: own IdemKey) -> Int[1..9000000000]"),
                ("body: body) -> html_url", "body: body) -> id"),
            ],
            "`id` é string",
        ),
        ("no-owner", &[("add_issue_comment(owner: @owner, ", "add_issue_comment(")], "exige `owner`"),
        ("enum", &[("issue_read(method: 'get',", "issue_read(method: 'obter',")], "fora do `enum`"),
        ("path", &[("-> issue.title", "-> issue.titulo")], "`issue.titulo` não existe"),
    ];
    for (tag, edits, why) in cases {
        let w = World::start(tag);
        let p = variant(&w, tag, edits);
        let o = w.athc("run", &p, &w.models(&[injected()]), "").output().unwrap();
        let t = text(&o);
        assert!(!o.status.success() && t.contains("R0646") && t.contains(why), "{tag}: {t}");
        nothing_happened(&w);
    }
}

/// **O servidor não alcança o modelo nem o usuário.** No meio de cada `tools/call`, o servidor
/// pede amostragem ao modelo, elicitação ao usuário e as raízes. Os três pedidos são recusados,
/// o modelo é consultado uma só vez (a do programa), e a tarefa termina.
#[test]
fn the_server_does_not_reach_the_model_or_the_user() {
    let w = World::start("ask");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "ask").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let asks: Vec<String> = w.server_log().into_iter().filter(|l| l.contains("pedido do servidor")).collect();
    assert_eq!(asks.len(), 3 * 5, "{asks:?}");
    assert!(asks.iter().all(|l| l.contains("→ erro -32601")), "{asks:?}");
    assert_eq!(prompt_hashes(&w, "run").len(), 1);
    assert!(text(&o).contains("15 pedido(s) do servidor ao cliente recusado(s)"), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)]);
}

/// **`isError` não quer dizer "não aconteceu".** O servidor aplica o comentário e responde
/// `isError` (como o do GitHub faz quando cria a issue e falha nos rótulos). Para uma operação
/// com chave, é resultado desconhecido: a consulta o acha, e ele não é reenviado.
#[test]
fn an_error_result_does_not_mean_not_applied() {
    let w = World::start("error");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "error-after-apply").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)], "{}", text(&o));
}

/// **A queda depois de aplicar.** O processo do servidor morre sem responder depois de criar o
/// comentário. O cliente o reinicia, refaz a conferência, e a consulta acha o comentário.
#[test]
fn a_server_crash_after_applying_is_reconciled() {
    let w = World::start("crash");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "crash-after-apply").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("iniciado 2 vez(es)"), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)]);
}

/// **Uma resposta a outro pedido é violação.** A conexão é descartada; a operação com chave fica
/// desconhecida e a consulta decide.
#[test]
fn a_reply_to_another_request_is_a_violation() {
    let w = World::start("wrong-id");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "wrong-id").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)], "{}", text(&o));
}

/// **`input_required` não é atendido.** O servidor quer perguntar algo ao usuário antes de
/// comentar. A linguagem não pergunta: a operação fica desconhecida, a consulta diz que não foi
/// aplicada, e a execução para dizendo isso (`R0611`). Nenhum comentário sai, e o servidor não
/// recebeu resposta à pergunta.
#[test]
fn input_required_is_not_answered() {
    let w = World::start("input");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "input-required").output().unwrap();
    assert!(!o.status.success());
    assert!(text(&o).contains("R0611") && text(&o).contains("não foi aplicada"), "{}", text(&o));
    assert_eq!((w.comments(), w.summaries().len()), (vec![], 0));
}

/// **O servidor recebe só o seu segredo.** O processo do servidor não herda o ambiente do
/// executor: das variáveis dele, só `PATH`, `HOME`, `SSL_CERT_FILE` (se definidas) e o token, sob
/// o nome que a ligação dá. E o
/// token que o servidor devolve num resultado é trocado por `‹secret:GH_TOKEN›` antes de entrar no
/// programa; ele não está no journal, no WAL nem na fita.
#[test]
fn the_server_gets_only_its_secret() {
    let w = World::start("env");
    let mut c = w.athc("run", &prog(), &w.models(&[injected()]), "env-dump");
    c.env("GROQ_API_KEY", "groq-segredo").env("OPENAI_API_KEY", "openai-segredo").env("ATH_CANARIO", "x");
    let o = c.output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let titles: Vec<String> = journal(&w, "run")
        .iter()
        .filter(|e| e["kind"] == "tool" && e["detail"]["tool"] == "issue_title")
        .map(|e| e["detail"]["result"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(titles.len(), 2);
    for t in &titles {
        let (names, token) = t.split_once(" token=").unwrap();
        let names: BTreeSet<&str> = names.split(',').collect();
        assert!(names.contains("GITHUB_PERSONAL_ACCESS_TOKEN"), "{t}");
        assert!(
            names.iter().all(|n| ["GITHUB_PERSONAL_ACCESS_TOKEN", "PATH", "HOME", "SSL_CERT_FILE"].contains(n)),
            "{t}"
        );
        assert_eq!(token, "‹secret:GH_TOKEN›");
    }
    for f in ["run.jsonl", "t.wal", "t.wal.tape"] {
        let s = std::fs::read_to_string(w.dir.join(f)).unwrap_or_default();
        assert!(!s.contains("tok-segredo-123"), "o token está em {f}");
    }
}

/// **Um rótulo do servidor só rebaixa.** O servidor rotula como não confiável o resultado de
/// `add_issue_comment`, que o programa declara confiável. O comentário foi feito (o efeito é
/// contado, não reenviado), e o valor não entra no programa: culpa do provedor (`R0649`).
#[test]
fn an_untrusted_label_on_a_trusted_result_is_blamed() {
    let w = World::start("label");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "untrusted-label").output().unwrap();
    assert!(!o.status.success());
    assert!(text(&o).contains("R0649"), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1)]);
    assert_eq!(w.summaries().len(), 0);
    // declarado `^ Untrusted`, o mesmo rótulo não é contradição: o programa já o trata assim
    let w = World::start("label-ok");
    let p =
        variant(&w, "untrusted", &[("k: own IdemKey) -> Text[..200]", "k: own IdemKey) -> Text[..200] ^ Untrusted")]);
    let o = w.athc("run", &p, &w.models(&[injected()]), "untrusted-label").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
}

/// **Sem resposta no prazo.** O primeiro comentário é aplicado e respondido depois do prazo:
/// resultado desconhecido, e a consulta o acha.
#[test]
fn a_timeout_is_an_unknown_result() {
    let w = World::start("slow");
    let mut c = w.athc("run", &prog(), &w.models(&[injected()]), "slow");
    let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    let i = args.iter().position(|a| a == "--timeout").unwrap();
    let mut c2 = Command::new(env!("CARGO_BIN_EXE_athc"));
    c2.envs(c.get_envs().filter_map(|(k, v)| v.map(|v| (k.to_owned(), v.to_owned()))));
    let mut args = args;
    args[i + 1] = "1".into();
    c2.args(&args);
    c = c2;
    let o = c.output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)], "{}", text(&o));
}

/// **O esquema de saída obriga.** O servidor anuncia esquema de saída e não manda
/// `structuredContent`: a leitura falha, e nada é feito.
#[test]
fn a_missing_structured_result_is_not_guessed() {
    let w = World::start("unstructured");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "no-structured").output().unwrap();
    assert!(!o.status.success());
    assert!(text(&o).contains("structuredContent"), "{}", text(&o));
    assert_eq!((w.comments(), w.summaries().len()), (vec![], 0));
}

/// **Uma mensagem grande demais** (9 MiB, acima do limite de 8 MiB) é cortada: a leitura falha
/// como violação, e nada é feito.
#[test]
fn an_oversized_message_is_a_violation() {
    let w = World::start("huge");
    let o = w.athc("run", &prog(), &w.models(&[injected()]), "huge").output().unwrap();
    assert!(!o.status.success());
    assert!(text(&o).contains("passou de 8 MiB"), "{}", text(&o));
    assert_eq!((w.comments(), w.summaries().len()), (vec![], 0));
}

/// **A queda do executor.** Morto depois de enviar o primeiro comentário; a retomada reinicia o
/// servidor, refaz a conferência, a consulta acha o comentário, e a triagem termina.
#[cfg(unix)]
#[test]
fn the_executor_crash_is_resumed() {
    use std::os::unix::process::ExitStatusExt;
    let w = World::start("resume");
    let m = w.models(&[injected()]);
    let o = w.athc("run", &prog(), &m, "").args(["--kill-at", "sent:1"]).output().unwrap();
    assert_eq!(o.status.signal(), Some(9), "{}", text(&o));
    let o = w.athc("resume", &prog(), &m, "").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(w.comments(), vec![(1, 1), (2, 1)]);
    assert_eq!(cited(&w.summaries()[0].1), BTreeSet::from([1, 2]));
}

/// **A ligação MCP é conferida antes de rodar.** `@arg` não declarado e parâmetro que não é da
/// ferramenta (`E0435`); parâmetro que não chega ao servidor e resultado sem caminho (`E0439`);
/// consulta que não lê uma lista (`E0436`); e a sintaxe (`url` e `mcp` juntos, segredo sem `as`).
#[test]
fn the_mcp_binding_is_checked_before_running() {
    let w = World::start("static");
    let check = |tag: &str, edits: &[(&str, &str)]| {
        let p = variant(&w, tag, edits);
        let o = Command::new(env!("CARGO_BIN_EXE_athc")).env("NO_COLOR", "1").arg("check").arg(&p).output().unwrap();
        assert!(!o.status.success(), "{tag}: aceito: {}", text(&o));
        text(&o)
    };
    assert!(check(
        "arg",
        &[(
            "repo: @repo, issue_number: int(issue)) -> issue.title",
            "repo: @repositorio, issue_number: int(issue)) -> issue.title"
        )]
    )
    .contains("E0435"));
    assert!(check(
        "param",
        &[("issue_number: int(issue), body: body) -> html_url", "issue_number: int(issue), body: corpo) -> html_url")]
    )
    .contains("E0435"));
    assert!(check(
        "unsent",
        &[("issue_number: int(issue), body: body) -> html_url", "issue_number: int(issue)) -> html_url")]
    )
    .contains("E0439"));
    assert!(check("noproj", &[("body: body) -> html_url", "body: body)")]).contains("E0439"));
    assert!(check("findlist", &[("-> comments[].html_url", "-> comments")]).contains("E0436"));
    let both = check(
        "both",
        &[(
            "  mcp  = env(\"GITHUB_MCP_SERVER\")",
            "  mcp  = env(\"GITHUB_MCP_SERVER\")\n  url  = \"https://api.github.com\"",
        )],
    );
    assert!(both.contains("não os dois"), "{both}");
    let noas = check("noas", &[(" as \"GITHUB_PERSONAL_ACCESS_TOKEN\"", "")]);
    assert!(noas.contains("ambiente do servidor"), "{noas}");
}
