//! **O cliente MCP do ARX (v1.0-RC9).**
//!
//! Um `bind` com `mcp = …` liga as ferramentas declaradas do programa às ferramentas de um
//! servidor MCP, iniciado pelo executor como um processo filho e falado por stdio (JSON-RPC, uma
//! mensagem por linha). Este módulo é a conexão (as duas eras do protocolo) e a **conferência na
//! ligação**: antes de qualquer efeito, o que o servidor anuncia de cada ferramenta ligada é
//! conferido contra o que o programa declara dela.
//!
//! **O que o servidor diz de si só pode restringir.** Descrições, `instructions` e títulos não
//! são lidos por nada (não há caminho deles até um prompt). Esquemas são conferidos contra a
//! declaração: uma ferramenta que não aceita o que o programa envia, ou que não devolve o tipo
//! que o programa espera, é recusada (`R0646`). Anotações e rótulos (`readOnlyHint`,
//! `destructiveHint`, `_meta.ifc`) são afirmações não confiáveis: uma que admite perigo maior do
//! que o declarado recusa a ligação ou o valor (`R0647`, `R0649`); uma que promete segurança é
//! ignorada (a classe do efeito é a que o programa declara).
//!
//! | era | aperto de mão | cada pedido |
//! |---|---|---|
//! | moderna (`2026-07-28`) | `server/discover` (opcional; usado como sonda) | `_meta` com versão, cliente e capacidades |
//! | legada (`2025-11-25`, `2025-06-18`, `2025-03-26`) | `initialize` + `notifications/initialized` | sem `_meta` |
//!
//! **O servidor não alcança o modelo nem o usuário.** Um pedido do servidor ao cliente
//! (`sampling/createMessage`, `elicitation/create`, `roots/list`, qualquer outro) é respondido
//! com erro e contado; um resultado `input_required` não é atendido.

use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const MODERN: &str = "2026-07-28";
/// o maior tamanho de uma mensagem do servidor, em bytes
const LINE: usize = 8 << 20;
pub const LEGACY: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

/// Como um pedido ao servidor não chegou a um resultado.
#[derive(Debug)]
pub enum Fail {
    /// nenhuma resposta no prazo
    Timeout,
    /// o processo terminou (ou fechou a saída) antes de responder
    Died,
    /// o pedido não pôde ser escrito: nada foi enviado
    NotSent,
    /// resposta de erro JSON-RPC
    Rpc(i64, String),
    /// o servidor violou o protocolo (resposta a um pedido que não foi feito, linha que não é JSON)
    Violation(String),
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fail::Timeout => write!(f, "sem resposta no prazo"),
            Fail::Died => write!(f, "o processo do servidor terminou antes de responder"),
            Fail::NotSent => write!(f, "o pedido não chegou ao servidor"),
            Fail::Rpc(c, m) => write!(f, "erro {c} do protocolo: {m}"),
            Fail::Violation(m) => write!(f, "violação do protocolo: {m}"),
        }
    }
}

pub struct Conn {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
    stderr: Arc<Mutex<Vec<String>>>,
    next: u64,
    abandoned: HashSet<String>,
    pub modern: bool,
    pub version: String,
    pub server: String,
    pub tools: BTreeMap<String, Json>,
    /// os pedidos do servidor ao cliente, recusados (método)
    pub refused: Vec<String>,
}

impl Drop for Conn {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn meta() -> Json {
    json!({
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientInfo": { "name": "aletheia-arx", "version": env!("CARGO_PKG_VERSION") },
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

/// O caminho do programa: com `/`, como está; senão, procurado no `PATH` do executor (o processo
/// do servidor não recebe o `PATH` para isto: recebe o programa já resolvido).
fn resolve(prog: &str, path: Option<&str>) -> Option<std::path::PathBuf> {
    if prog.contains('/') {
        return Some(prog.into());
    }
    path?.split(':').map(|d| std::path::Path::new(d).join(prog)).find(|p| p.is_file())
}

impl Conn {
    /// Inicia o servidor e faz o aperto de mão; depois, lista as ferramentas. `env` é o ambiente
    /// inteiro do processo do servidor (ele não herda o do executor).
    pub fn open(
        argv: &[String],
        env: &[(String, String)],
        exec_path: Option<&str>,
        timeout: f64,
    ) -> Result<Conn, String> {
        let prog = argv.first().ok_or("o comando do servidor está vazio")?;
        let bin = resolve(prog, exec_path).ok_or_else(|| format!("`{prog}` não foi achado"))?;
        let mut child = Command::new(&bin)
            .args(&argv[1..])
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("`{}` não pôde ser iniciado: {e}", bin.display()))?;
        let stdin = child.stdin.take().ok_or("sem stdin")?;
        let stdout = child.stdout.take().ok_or("sem stdout")?;
        let stderr = child.stderr.take().ok_or("sem stderr")?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // uma linha tem no máximo `LINE` bytes: um servidor que manda mais é cortado (a linha
            // incompleta não é JSON, e o pedido em curso termina em violação)
            let mut r = BufReader::new(stdout);
            loop {
                let mut buf = Vec::new();
                match std::io::Read::take(&mut r, LINE as u64 + 1).read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let long = buf.len() > LINE;
                let line = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string();
                if tx.send(if long { "\u{0}".to_string() } else { line }).is_err() || long {
                    break;
                }
            }
        });
        let tail = Arc::new(Mutex::new(Vec::new()));
        let t2 = tail.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                let mut t = t2.lock().unwrap();
                t.push(line.chars().take(400).collect());
                if t.len() > 8 {
                    t.remove(0);
                }
            }
        });
        let mut c = Conn {
            child,
            stdin,
            rx,
            stderr: tail,
            next: 1,
            abandoned: HashSet::new(),
            modern: true,
            version: MODERN.into(),
            server: String::new(),
            tools: BTreeMap::new(),
            refused: Vec::new(),
        };
        let hs = timeout.clamp(0.5, 10.0);
        // A sonda moderna: `server/discover`. Erro, ou silêncio, é um servidor legado: o aperto de
        // mão `initialize`, no mesmo processo (a regra de compatibilidade do stdio).
        let probe = c.request("server/discover", json!({ "_meta": meta() }), hs);
        let modern = match probe {
            Ok(r) => r["supportedVersions"].as_array().is_some_and(|v| v.iter().any(|x| x == MODERN)).then_some(r),
            Err(Fail::Died) | Err(Fail::NotSent) => return Err(c.died("antes de responder à sonda `server/discover`")),
            Err(f @ Fail::Violation(_)) => return Err(format!("na sonda `server/discover`: {f}")),
            Err(_) => None,
        };
        match modern {
            Some(r) => {
                c.server = show_info(&r["_meta"]["io.modelcontextprotocol/serverInfo"]);
            }
            None => {
                c.modern = false;
                let r = c
                    .request(
                        "initialize",
                        json!({ "protocolVersion": LEGACY[0], "capabilities": {},
                                "clientInfo": { "name": "aletheia-arx", "version": env!("CARGO_PKG_VERSION") } }),
                        hs,
                    )
                    .map_err(|f| match f {
                        Fail::Died | Fail::NotSent => c.died("no aperto de mão `initialize`"),
                        other => format!("o aperto de mão `initialize` falhou: {other}"),
                    })?;
                let v = r["protocolVersion"].as_str().unwrap_or_default().to_string();
                if !LEGACY.contains(&v.as_str()) {
                    return Err(format!(
                        "o servidor só fala a versão `{v}` do protocolo, que este cliente não conhece"
                    ));
                }
                c.version = v;
                c.server = show_info(&r["serverInfo"]);
                c.notify("notifications/initialized", json!({}))?;
            }
        }
        // as ferramentas, página a página
        let mut cursor: Option<String> = None;
        for _ in 0..50 {
            let mut p = json!({});
            if let Some(cu) = &cursor {
                p["cursor"] = json!(cu);
            }
            let r = c.call_raw("tools/list", p, hs).map_err(|f| format!("`tools/list` falhou: {f}"))?;
            for t in r["tools"].as_array().into_iter().flatten() {
                if let Some(n) = t["name"].as_str() {
                    c.tools.insert(n.to_string(), t.clone());
                }
            }
            cursor = r["nextCursor"].as_str().map(String::from);
            if cursor.is_none() {
                break;
            }
        }
        Ok(c)
    }

    fn died(&mut self, when: &str) -> String {
        std::thread::sleep(Duration::from_millis(50));
        let tail = self.stderr.lock().unwrap().join(" | ");
        let code = self.child.try_wait().ok().flatten().map(|s| format!(" ({s})")).unwrap_or_default();
        if tail.is_empty() {
            format!("o servidor terminou {when}{code}")
        } else {
            format!("o servidor terminou {when}{code}: {}", tail.chars().take(300).collect::<String>())
        }
    }

    fn write(&mut self, m: &Json) -> Result<(), Fail> {
        writeln!(self.stdin, "{m}").and_then(|_| self.stdin.flush()).map_err(|_| Fail::NotSent)
    }

    fn notify(&mut self, method: &str, params: Json) -> Result<(), String> {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .map_err(|_| self.died(&format!("antes de `{method}`")))
    }

    /// Um pedido, com `_meta` na era moderna.
    pub fn call_raw(&mut self, method: &str, mut params: Json, timeout: f64) -> Result<Json, Fail> {
        if self.modern {
            params["_meta"] = meta();
        }
        self.request(method, params, timeout)
    }

    fn request(&mut self, method: &str, params: Json, timeout: f64) -> Result<Json, Fail> {
        let id = self.next;
        self.next += 1;
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.05));
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = match self.rx.recv_timeout(left) {
                Ok(l) => l,
                Err(RecvTimeoutError::Timeout) => {
                    self.abandoned.insert(id.to_string());
                    return Err(Fail::Timeout);
                }
                Err(RecvTimeoutError::Disconnected) => return Err(Fail::Died),
            };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(msg) = serde_json::from_str::<Json>(&line) else {
                return Err(Fail::Violation(if line == "\u{0}" {
                    format!("uma mensagem do servidor passou de {} MiB", LINE >> 20)
                } else {
                    "o servidor escreveu na saída uma linha que não é JSON-RPC".into()
                }));
            };
            if let Some(m) = msg["method"].as_str() {
                // um pedido do servidor ao cliente: recusado, sempre; uma notificação: ignorada
                if let Some(rid) = msg.get("id") {
                    self.refused.push(m.to_string());
                    let _ = self.write(&json!({ "jsonrpc": "2.0", "id": rid, "error": { "code": -32601,
                        "message": "o cliente Aletheia não atende pedidos do servidor (amostragem, elicitação, raízes)" } }));
                }
                continue;
            }
            let rid = match &msg["id"] {
                Json::Number(n) => n.to_string(),
                Json::String(s) => s.clone(),
                _ => String::new(),
            };
            if rid == id.to_string() {
                if let Some(e) = msg.get("error") {
                    return Err(Fail::Rpc(
                        e["code"].as_i64().unwrap_or(0),
                        e["message"].as_str().unwrap_or_default().to_string(),
                    ));
                }
                return Ok(msg["result"].clone());
            }
            if self.abandoned.remove(&rid) {
                continue;
            }
            return Err(Fail::Violation(format!("resposta a um pedido que não foi feito (id {})", msg["id"])));
        }
    }
}

fn show_info(i: &Json) -> String {
    match (i["name"].as_str(), i["version"].as_str()) {
        (Some(n), Some(v)) => format!("{n} {v}"),
        (Some(n), None) => n.to_string(),
        _ => "servidor sem identificação".into(),
    }
}

/// A impressão digital de uma definição de ferramenta (BLAKE3 do JSON canônico, 12 dígitos).
pub fn pin(def: &Json) -> String {
    let h = blake3::hash(def.to_string().as_bytes()).to_hex();
    h[..12].to_string()
}

/// Um segmento de caminho num esquema: um campo, ou os elementos de uma lista.
#[derive(Clone, Debug)]
pub enum Seg {
    Field(String),
    Items,
}

/// As variantes de um esquema: ele mesmo e as de `oneOf`, `anyOf`, `allOf`, recursivamente.
fn variants(s: &Json) -> Vec<&Json> {
    let mut out = vec![s];
    for k in ["oneOf", "anyOf", "allOf"] {
        for a in s[k].as_array().into_iter().flatten() {
            out.extend(variants(a));
        }
    }
    out
}

/// Um esquema que só proíbe (`{"not": …}`) não é um campo presente.
fn forbids(s: &Json) -> bool {
    s.as_object().is_some_and(|o| o.contains_key("not") && !o.contains_key("type") && !o.contains_key("properties"))
}

fn own_types(s: &Json, out: &mut BTreeSet<String>) {
    match &s["type"] {
        Json::String(t) => {
            out.insert(t.clone());
        }
        Json::Array(ts) => ts.iter().filter_map(|t| t.as_str()).for_each(|t| {
            out.insert(t.to_string());
        }),
        _ => {
            if s["properties"].is_object() {
                out.insert("object".into());
            }
            if s.get("items").is_some() {
                out.insert("array".into());
            }
            for v in s["enum"].as_array().into_iter().flatten() {
                out.insert(json_type(v).into());
            }
            if let Some(v) = s.get("const") {
                out.insert(json_type(v).into());
            }
        }
    }
}

fn json_type(v: &Json) -> &'static str {
    match v {
        Json::Null => "null",
        Json::Bool(_) => "boolean",
        Json::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Json::Number(_) => "number",
        Json::String(_) => "string",
        Json::Array(_) => "array",
        Json::Object(_) => "object",
    }
}

/// Os tipos JSON que o esquema admite no caminho. Vazio: o caminho não existe em variante alguma.
/// `"any"`: existe, e o esquema não diz o tipo.
pub fn types_at(s: &Json, path: &[Seg]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    match path.split_first() {
        None => {
            for v in variants(s) {
                own_types(v, &mut out);
            }
            if out.is_empty() {
                out.insert("any".into());
            }
        }
        Some((seg, rest)) => {
            for v in variants(s) {
                let next = match seg {
                    Seg::Field(f) => v["properties"].get(f),
                    Seg::Items => v.get("items"),
                };
                if let Some(n) = next.filter(|n| !forbids(n)) {
                    out.extend(types_at(n, rest));
                }
            }
        }
    }
    out
}

/// O tipo JSON que um tipo base da linguagem aceita.
fn accepts(base: &str) -> &'static [&'static str] {
    match base {
        "Text" => &["string"],
        "Int" => &["integer"],
        "Bool" => &["boolean"],
        "List" => &["array"],
        _ => &[],
    }
}

/// Os tipos definidos no conjunto que o tipo base não aceita (`null` e `any` ficam para o monitor).
pub fn incompatible(base: &str, ts: &BTreeSet<String>) -> Vec<String> {
    if base == "Unit" {
        return Vec::new();
    }
    let ok = accepts(base);
    ts.iter().filter(|t| *t != "null" && *t != "any" && !ok.contains(&t.as_str())).cloned().collect()
}

/// Acha no documento o caminho.
pub fn at<'a>(doc: &'a Json, path: &[String]) -> Option<&'a Json> {
    path.iter().try_fold(doc, |d, k| d.get(k))
}

/// Algum texto do valor (em qualquer profundidade) carrega o marcador?
pub fn carries(v: &Json, mark: &str) -> bool {
    match v {
        Json::String(s) => s.contains(mark),
        Json::Array(a) => a.iter().any(|x| carries(x, mark)),
        Json::Object(o) => o.values().any(|x| carries(x, mark)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Json {
        let p = format!("{}/../ath-world/fixtures/github-mcp/{name}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    /// Os esquemas reais do servidor do GitHub: os caminhos que a triagem lê têm os tipos que ela
    /// declara, e os que não existem não existem.
    #[test]
    fn the_paths_of_the_triage_in_the_real_schemas() {
        let f = |s: &str| Seg::Field(s.into());
        let comment = fixture("add_issue_comment");
        let out = &comment["outputSchema"];
        assert_eq!(types_at(out, &[f("html_url")]), BTreeSet::from(["string".to_string()]));
        // `id` é texto no servidor do GitHub: um programa que o declare `Int` é recusado
        assert_eq!(incompatible("Int", &types_at(out, &[f("id")])), vec!["string".to_string()]);
        assert!(types_at(out, &[f("number")]).is_empty());
        let read = fixture("issue_read");
        let out = &read["outputSchema"];
        assert_eq!(types_at(out, &[f("issue"), f("title")]), BTreeSet::from(["string".to_string()]));
        assert!(incompatible("Text", &types_at(out, &[f("comments"), Seg::Items, f("html_url")])).is_empty());
        assert!(types_at(out, &[f("comments"), Seg::Items, f("html_url")]).contains("string"));
        assert!(types_at(out, &[f("comments")]).contains("array"));
        let write = fixture("issue_write");
        assert!(types_at(&write["outputSchema"], &[f("issue"), f("url")]).contains("string"));
        let input = &comment["inputSchema"];
        assert!(types_at(input, &[f("issue_number")]).contains("number"));
        assert!(types_at(input, &[f("nada")]).is_empty());
    }
}
