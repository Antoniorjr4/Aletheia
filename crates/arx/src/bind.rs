//! **A fronteira nativa (v1.0-RC5): `bind`.**
//!
//! Um programa liga as suas ferramentas a uma API HTTP na própria linguagem:
//!
//! ```text
//! bind bank {
//!   url    = env("BANK_URL")
//!   auth   = secret("BANK_TOKEN")
//!   status = "GET /v1/operations"
//!   pay    = "POST /v1/payments -> id"
//! }
//! ```
//!
//! Não há código de cola: o corpo da requisição são os parâmetros **nomeados** da ferramenta
//! (capacidades e a chave não são dados do provedor), a chave `own IdemKey` vai no cabeçalho
//! `Idempotency-Key` com a identidade da execução, e a resposta volta pelo monitor de tipo.
//!
//! | o que o HTTP diz | o que a linguagem vê |
//! |---|---|
//! | a conexão não abriu | `Unreachable`: nada saiu do processo |
//! | 200, 201, 204 | o valor (`-> campo`, se a rota o nomeia), conferido contra o tipo |
//! | 202 e os outros 2xx | `Timeout`: o pedido foi aceito, o efeito não está concluído |
//! | 4xx (menos 408) | `Failed`: recusa do provedor, o efeito não aconteceu |
//! | 408, 5xx, prazo, resposta cortada | `Timeout`: o `TimeoutUnknown` da linguagem |
//!
//! **Segredos por referência.** `auth = secret("NOME")` nomeia uma variável do ambiente do
//! executor. O valor só existe aqui, no cabeçalho `Authorization` da requisição: não é
//! argumento, não vai ao journal, ao WAL, à fita nem ao prompt de modelo algum, e o programa
//! não tem expressão que o leia. Se o provedor o devolver numa resposta ou num erro, ele é
//! trocado por `‹secret:NOME›` antes de o valor entrar no programa. Sobre HTTP sem TLS, um
//! segredo só viaja para o próprio host (`127.0.0.1`, `localhost`): senão, `R0643`.

use crate::host::{Host, HostError, Status, TrustedSnapshot};
use crate::mcp::{self, Conn, Fail, Seg};
use crate::value::Value;
use ath_syntax::ast::{BindDecl, BindUrl, EffClass, Item, Label, McpArg, McpCall, Module, Route, TypeExpr};
use serde_json::{json, Map, Value as Json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

pub const R0641: &str = "o programa liga ferramentas a provedores reais (`bind`); esta execução não tem a ligação";

struct Bound {
    base: String,
    route: Route,
    /// os nomes dos parâmetros que são dados do provedor, na ordem dos argumentos
    params: Vec<Option<String>>,
    secret: Option<usize>,
    status: Option<Route>,
    /// v1.0-RC6: o parâmetro de texto que carrega o marcador da chave, e a consulta que o acha
    marker: Option<String>,
    find: Option<Route>,
    /// segundos que a listagem do provedor pode levar para refletir uma escrita
    settle: u32,
}

/// O marcador da chave no texto que o provedor guarda: um comentário HTML com a identidade da
/// execução e a chave. É por ele que a consulta (`.find`) reconhece a operação.
fn marker(wire: &str) -> String {
    format!("<!-- aletheia:{wire} -->")
}

/// O resultado de uma resposta: ela inteira, um campo, ou (`[].campo`) o campo de cada elemento.
fn extract(field: &Option<String>, doc: &Json) -> Json {
    match field.as_deref() {
        None => doc.clone(),
        Some(f) => match f.strip_prefix("[].") {
            Some(each) => match doc.as_array() {
                Some(a) => Json::Array(a.iter().map(|e| e.get(each).cloned().unwrap_or(Json::Null)).collect()),
                None => Json::Null,
            },
            None => doc.get(f).cloned().unwrap_or(Json::Null),
        },
    }
}

/// v1.0-RC9: uma ferramenta ligada a um servidor MCP.
struct McpTool {
    server: usize,
    call: McpCall,
    find: Option<McpCall>,
    /// os nomes dos parâmetros de dados, na ordem dos argumentos, e o tipo base de cada um
    params: Vec<Option<String>>,
    bases: BTreeMap<String, String>,
    marker: Option<String>,
    settle: u32,
    keyed: bool,
    reversible: bool,
    /// o tipo base do que a ferramenta devolve
    ret: String,
    /// o programa declara o resultado confiável (`^ Trusted`/`^ Endorsed`, ou sem rótulo)
    trusted: bool,
}

/// v1.0-RC9: um servidor MCP, o processo e o que foi conferido nele.
struct McpServer {
    bind: String,
    argv: Vec<String>,
    env: Vec<(String, String)>,
    args: BTreeMap<String, String>,
    conn: Option<Conn>,
    /// o que a última conexão disse de si (servidor, versão do protocolo)
    who: String,
    /// as impressões digitais das definições conferidas, por ferramenta do servidor
    pins: BTreeMap<String, String>,
    /// as definições mudaram entre duas conexões (a conferência foi refeita)
    changed: BTreeSet<String>,
    refused: Vec<String>,
    starts: u32,
}

pub struct BoundHost {
    inner: Box<dyn Host>,
    tools: BTreeMap<String, Bound>,
    mcp_tools: BTreeMap<String, McpTool>,
    servers: Vec<McpServer>,
    exec_path: Option<String>,
    timeout: f64,
    blame: Option<String>,
    /// (nome, valor): o valor nunca sai deste módulo, a não ser no cabeçalho da requisição
    secrets: Vec<(String, String)>,
    run: String,
    agent: ureq::Agent,
    last_raw: Option<Json>,
    receipts: BTreeMap<String, Json>,
}

/// Só o próprio host (`ath_syntax::ast::loopback_url`): na dúvida, é outro host.
fn loopback(url: &str) -> bool {
    ath_syntax::ast::loopback_url(url)
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl BoundHost {
    /// Liga as ferramentas dos `bind` do módulo. `env` lê o ambiente do executor. Falha, antes
    /// de qualquer execução, se uma variável falta (`R0642`) ou se um segredo viajaria em claro
    /// para outro host (`R0643`).
    pub fn attach(
        m: &Module,
        inner: Box<dyn Host>,
        run: &str,
        timeout: f64,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<BoundHost, String> {
        let mut tools = BTreeMap::new();
        let mut secrets: Vec<(String, String)> = Vec::new();
        let binds: Vec<&BindDecl> =
            m.items.iter().filter_map(|i| if let Item::Bind(b) = i { Some(b) } else { None }).collect();
        let mut local = true;
        let mut mcp_tools = BTreeMap::new();
        let mut servers: Vec<McpServer> = Vec::new();
        for b in binds {
            let name = &b.name.name;
            if b.mcp.is_some() {
                BoundHost::attach_mcp(m, b, env, &mut secrets, &mut servers, &mut mcp_tools)?;
                continue;
            }
            let base = match &b.url {
                BindUrl::Lit(u) => u.clone(),
                BindUrl::Env(v) => env(v).filter(|x| !x.is_empty()).ok_or_else(|| {
                    format!("R0642: `bind {name}`: a variável de ambiente `{v}` (url) não está definida; nada foi executado")
                })?,
            };
            let base = base.trim_end_matches('/').to_string();
            if ath_syntax::ast::url_authority(&base).is_none() {
                return Err(format!(
                    "R0642: `bind {name}`: a url não é http(s), não tem host ou carrega credenciais; nada foi executado"
                ));
            }
            local &= loopback(&base);
            let secret = match &b.auth {
                None => None,
                Some(v) => {
                    let val = env(v).filter(|x| !x.is_empty()).ok_or_else(|| {
                        format!("R0642: `bind {name}`: o segredo `{v}` não está no ambiente do executor; nada foi executado")
                    })?;
                    if base.starts_with("http://") && !loopback(&base) {
                        return Err(format!(
                            "R0643: `bind {name}`: o segredo `{v}` viajaria em claro (http sem TLS) para outro host; nada foi executado"
                        ));
                    }
                    secrets.push((v.clone(), val));
                    Some(secrets.len() - 1)
                }
            };
            for (tool, route) in &b.routes {
                let Some(decl) = m.items.iter().find_map(|i| match i {
                    Item::Tool(t) if t.name.name == tool.name => Some(t),
                    _ => None,
                }) else {
                    continue;
                };
                let params = decl
                    .params
                    .iter()
                    .map(|p| match p.ty.unlabeled() {
                        TypeExpr::Cap(..) => None,
                        t if t.base_name() == "IdemKey" => None,
                        _ => Some(p.name.name.clone()),
                    })
                    .collect();
                tools.insert(
                    tool.name.clone(),
                    Bound {
                        base: base.clone(),
                        route: route.clone(),
                        params,
                        secret,
                        status: b.status.clone(),
                        marker: b.keys.iter().find(|(k, _)| k.name == tool.name).map(|(_, p)| p.clone()),
                        find: b.finds.iter().find(|(k, _)| k.name == tool.name).map(|(_, r)| r.clone()),
                        settle: b.settles.iter().find(|(k, _)| k.name == tool.name).map_or(0, |(_, n)| *n),
                    },
                );
            }
        }
        let mut cfg = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs_f64(timeout.max(0.05))))
            .http_status_as_error(false);
        if local {
            cfg = cfg.proxy(None);
        }
        // **As raízes de confiança do TLS** (v1.0-RC6). Por padrão, as raízes públicas embutidas.
        // Se o ambiente do executor define `SSL_CERT_FILE` (a convenção de OpenSSL, curl, Python
        // e Go), as raízes são as desse arquivo e só elas: é como um executor atrás de um proxy
        // corporativo que re-termina o TLS, ou diante de uma API com CA própria, confia nela. É
        // configuração do operador, como a url e o segredo: o programa não a escolhe.
        if let Some(path) = env("SSL_CERT_FILE").filter(|p| !p.is_empty()) {
            use ureq::tls::{parse_pem, PemItem, RootCerts, TlsConfig};
            let pem = std::fs::read(&path)
                .map_err(|e| format!("R0644: `SSL_CERT_FILE` ({path}) não pôde ser lido: {e}; nada foi executado"))?;
            let certs: Vec<_> = parse_pem(&pem)
                .filter_map(|i| match i {
                    Ok(PemItem::Certificate(c)) => Some(c),
                    _ => None,
                })
                .collect();
            if certs.is_empty() {
                return Err(format!("R0644: `SSL_CERT_FILE` ({path}) não tem certificado algum; nada foi executado"));
            }
            cfg = cfg.tls_config(TlsConfig::builder().root_certs(RootCerts::new_with_certs(&certs)).build());
        }
        let mut h = BoundHost {
            inner,
            tools,
            mcp_tools,
            servers,
            exec_path: env("PATH"),
            timeout,
            blame: None,
            secrets,
            run: run.to_string(),
            agent: cfg.build().into(),
            last_raw: None,
            receipts: BTreeMap::new(),
        };
        // v1.0-RC9: cada servidor MCP é iniciado e conferido agora, antes de qualquer efeito
        for i in 0..h.servers.len() {
            h.connect(i).map_err(|e| h.scrub_text(&e))?;
        }
        Ok(h)
    }

    /// v1.0-RC9: um `bind` MCP: o comando, o ambiente do processo do servidor (só o segredo, sob
    /// o nome que a ligação dá, e `PATH`, `HOME` e `SSL_CERT_FILE` do executor), os argumentos
    /// de configuração e as ferramentas.
    fn attach_mcp(
        m: &Module,
        b: &BindDecl,
        env: &dyn Fn(&str) -> Option<String>,
        secrets: &mut Vec<(String, String)>,
        servers: &mut Vec<McpServer>,
        mcp_tools: &mut BTreeMap<String, McpTool>,
    ) -> Result<(), String> {
        let name = &b.name.name;
        let need = |v: &str, what: &str| {
            env(v).filter(|x| !x.is_empty()).ok_or_else(|| {
                format!(
                    "R0642: `bind {name}`: a variável de ambiente `{v}` ({what}) não está definida; nada foi executado"
                )
            })
        };
        let cmd = match b.mcp.as_ref().expect("bind mcp") {
            BindUrl::Lit(c) => c.clone(),
            BindUrl::Env(v) => need(v, "o comando do servidor MCP")?,
        };
        let argv: Vec<String> = cmd.split_whitespace().map(String::from).collect();
        let mut child_env: Vec<(String, String)> = Vec::new();
        for k in ["PATH", "HOME", "SSL_CERT_FILE"] {
            if let Some(v) = env(k).filter(|x| !x.is_empty()) {
                child_env.push((k.to_string(), v));
            }
        }
        if let (Some(v), Some(as_)) = (&b.auth, &b.secret_as) {
            let val = need(v, "o segredo")?;
            secrets.push((v.clone(), val.clone()));
            child_env.retain(|(k, _)| k != as_);
            child_env.push((as_.clone(), val));
        }
        let mut args = BTreeMap::new();
        for (n, v) in &b.args {
            let val = match v {
                BindUrl::Lit(x) => x.clone(),
                BindUrl::Env(e) => need(e, &format!("`arg.{}`", n.name))?,
            };
            args.insert(n.name.clone(), val);
        }
        let server = servers.len();
        servers.push(McpServer {
            bind: name.clone(),
            argv,
            env: child_env,
            args,
            conn: None,
            who: String::new(),
            pins: BTreeMap::new(),
            changed: BTreeSet::new(),
            refused: Vec::new(),
            starts: 0,
        });
        for (tool, call) in &b.calls {
            let Some(decl) = m.items.iter().find_map(|i| match i {
                Item::Tool(t) if t.name.name == tool.name => Some(t),
                _ => None,
            }) else {
                continue;
            };
            let params = decl
                .params
                .iter()
                .map(|p| match p.ty.unlabeled() {
                    TypeExpr::Cap(..) => None,
                    t if t.base_name() == "IdemKey" => None,
                    _ => Some(p.name.name.clone()),
                })
                .collect();
            let bases =
                decl.params.iter().map(|p| (p.name.name.clone(), p.ty.unlabeled().base_name().to_string())).collect();
            let label = match &decl.ret {
                TypeExpr::Labeled(_, l, _) => *l,
                _ => Label::Trusted,
            };
            mcp_tools.insert(
                tool.name.clone(),
                McpTool {
                    server,
                    call: call.clone(),
                    find: b.call_finds.iter().find(|(k, _)| k.name == tool.name).map(|(_, c)| c.clone()),
                    params,
                    bases,
                    marker: b.keys.iter().find(|(k, _)| k.name == tool.name).map(|(_, p)| p.clone()),
                    settle: b.settles.iter().find(|(k, _)| k.name == tool.name).map_or(0, |(_, n)| *n),
                    keyed: decl.params.iter().any(|p| p.ty.unlabeled().base_name() == "IdemKey"),
                    reversible: decl.class == EffClass::Reversible,
                    ret: decl.ret.unlabeled().base_name().to_string(),
                    trusted: label >= Label::Endorsed,
                },
            );
        }
        Ok(())
    }

    /// Inicia (ou reinicia) o servidor e confere nele cada ferramenta ligada. Erro: nada foi
    /// enviado a ferramenta alguma deste servidor nesta conexão.
    fn connect(&mut self, i: usize) -> Result<(), String> {
        let path = self.exec_path.clone();
        let timeout = self.timeout;
        let s = &mut self.servers[i];
        s.conn = None;
        s.starts += 1;
        let conn = Conn::open(&s.argv, &s.env, path.as_deref(), timeout).map_err(|e| {
            format!("R0645: `bind {}`: o servidor MCP não pôde ser ligado: {e}; nada foi executado", s.bind)
        })?;
        let bind = s.bind.clone();
        let mut pins = BTreeMap::new();
        for (tool, t) in self.mcp_tools.iter().filter(|(_, t)| t.server == i) {
            for (c, find) in std::iter::once((&t.call, false)).chain(t.find.as_ref().map(|f| (f, true))) {
                let def = conform(&conn, &bind, tool, t, c, find)?;
                pins.insert(c.tool.clone(), mcp::pin(def));
            }
        }
        let s = &mut self.servers[i];
        let mut conn = conn;
        s.refused.append(&mut conn.refused);
        if !s.pins.is_empty() {
            for (k, v) in &pins {
                if s.pins.get(k) != Some(v) {
                    s.changed.insert(k.clone());
                }
            }
        }
        s.pins = pins;
        s.who = format!("{}, protocolo {}", conn.server, conn.version);
        s.conn = Some(conn);
        Ok(())
    }

    /// Os argumentos de uma chamada MCP, a partir dos argumentos da ferramenta. `Err`: um
    /// `int(p)` sobre texto que não é inteiro; nada é enviado.
    fn mcp_args(
        &self,
        t: &McpTool,
        c: &McpCall,
        args: &[Value],
        key: Option<&str>,
    ) -> Result<Map<String, Json>, String> {
        let s = &self.servers[t.server];
        let val = |p: &str| -> Option<&Value> {
            t.params.iter().zip(args).find(|(n, _)| n.as_deref() == Some(p)).map(|(_, v)| v)
        };
        let mut out = Map::new();
        for (k, a) in &c.args {
            let j = match a {
                McpArg::Param(p) => {
                    let mut j = val(p).map(Value::to_json).unwrap_or(Json::Null);
                    if let (Some(m), Some(k)) = (&t.marker, key) {
                        if m == p {
                            j = json!(format!("{}\n\n{}", j.as_str().unwrap_or_default(), marker(k)));
                        }
                    }
                    j
                }
                McpArg::IntOf(p) => match val(p) {
                    Some(Value::Int(n)) => json!(n),
                    Some(Value::Text(x)) => match x.trim().parse::<i64>() {
                        Ok(n) => json!(n),
                        Err(_) => return Err(format!("`int({p})`: `{x}` não é um inteiro; nada foi enviado")),
                    },
                    _ => return Err(format!("`int({p})` sem valor; nada foi enviado")),
                },
                McpArg::Arg(x) => json!(s.args.get(x).cloned().unwrap_or_default()),
                McpArg::Str(x) => json!(x),
                McpArg::Num(n) => json!(n),
            };
            out.insert(k.clone(), j);
        }
        Ok(out)
    }

    /// O documento de um resultado: `structuredContent` se a ferramenta anuncia esquema de saída
    /// (e então ele é obrigatório), senão o texto do primeiro bloco, lido como JSON se for JSON.
    fn mcp_doc(conn: &Conn, tool: &str, r: &Json) -> Result<Json, String> {
        let schema = conn.tools.get(tool).is_some_and(|d| d["outputSchema"].is_object());
        if schema {
            return r
                .get("structuredContent")
                .cloned()
                .ok_or_else(|| format!("`{tool}` anuncia esquema de saída e não mandou `structuredContent`"));
        }
        let text = r["content"]
            .as_array()
            .and_then(|a| a.iter().find(|c| c["type"] == "text"))
            .and_then(|c| c["text"].as_str())
            .unwrap_or_default();
        Ok(serde_json::from_str(text).unwrap_or_else(|_| Json::String(text.to_string())))
    }

    /// Um `tools/call`, com a conexão (reiniciada e reconferida se caiu). `Err(None)`: nada foi
    /// enviado; `Err(Some(f))`: o pedido saiu e não houve resultado.
    fn mcp_send(
        &mut self,
        server: usize,
        name: &str,
        arguments: Map<String, Json>,
    ) -> Result<(Json, f64), Result<Fail, String>> {
        if self.servers[server].conn.is_none() {
            self.connect(server).map_err(Err)?;
        }
        let timeout = self.timeout;
        let s = &mut self.servers[server];
        let conn = s.conn.as_mut().expect("conexão");
        let t0 = Instant::now();
        let r = conn.call_raw("tools/call", json!({ "name": name, "arguments": arguments }), timeout);
        s.refused.append(&mut conn.refused);
        let lat = t0.elapsed().as_secs_f64();
        match r {
            Ok(v) => Ok((v, lat)),
            Err(Fail::NotSent) => {
                s.conn = None;
                Err(Err("o servidor não recebeu o pedido".into()))
            }
            Err(f) => {
                // depois de um prazo, de uma queda ou de uma violação, a conexão não serve mais
                if !matches!(f, Fail::Rpc(..)) {
                    s.conn = None;
                }
                Err(Ok(f))
            }
        }
    }

    /// Troca toda ocorrência de um segredo por `‹secret:NOME›`.
    fn scrub_text(&self, s: &str) -> String {
        let mut out = s.to_string();
        for (name, val) in &self.secrets {
            if out.contains(val.as_str()) {
                out = out.replace(val.as_str(), &format!("‹secret:{name}›"));
            }
        }
        out
    }

    fn scrub(&self, j: &Json) -> Json {
        match j {
            Json::String(s) => Json::String(self.scrub_text(s)),
            Json::Array(a) => Json::Array(a.iter().map(|x| self.scrub(x)).collect()),
            Json::Object(o) => Json::Object(o.iter().map(|(k, v)| (self.scrub_text(k), self.scrub(v))).collect()),
            other => other.clone(),
        }
    }

    /// Uma requisição: `Ok((status, corpo, segundos))`, ou o erro de transporte já classificado.
    fn http(
        &self,
        b: &Bound,
        route: &Route,
        fields: &Map<String, Json>,
        key: Option<&str>,
    ) -> Result<(u16, String, f64, bool), HostError> {
        // v1.0-RC6: `{parâmetro}` no caminho recebe o argumento, codificado como UM segmento
        // (uma `/` no valor não cria caminho); o host é sempre o da `url`. O que vai no
        // caminho não vai no corpo nem na query.
        let mut fields = fields.clone();
        let (segs, query) = route.path.split_once('?').unwrap_or((&route.path, ""));
        let path: Vec<String> = segs
            .split('/')
            .map(|seg| match seg.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
                Some(name) => {
                    let v = fields.remove(name).unwrap_or(Json::Null);
                    urlencode(&v.as_str().map(String::from).unwrap_or_else(|| v.to_string()))
                }
                None => seg.to_string(),
            })
            .collect();
        let mut url = format!("{}{}", b.base, path.join("/"));
        let fields = &fields;
        let get = route.method == "GET";
        if !query.is_empty() {
            url = format!("{url}?{query}");
        }
        if get && !fields.is_empty() {
            let q: Vec<String> = fields
                .iter()
                .map(|(k, v)| {
                    let v = v.as_str().map(String::from).unwrap_or_else(|| v.to_string());
                    format!("{}={}", urlencode(k), urlencode(&v))
                })
                .collect();
            url = format!("{url}{}{}", if query.is_empty() { "?" } else { "&" }, q.join("&"));
        }
        let auth = b.secret.map(|i| format!("Bearer {}", self.secrets[i].1));
        let t0 = Instant::now();
        let sent = if get {
            let mut c = self.agent.get(&url);
            if let Some(a) = &auth {
                c = c.header("Authorization", a);
            }
            if let Some(k) = key {
                c = c.header("Idempotency-Key", k);
            }
            c.call()
        } else {
            let mut c = match route.method.as_str() {
                "PUT" => self.agent.put(&url),
                "PATCH" => self.agent.patch(&url),
                _ => self.agent.post(&url),
            }
            .header("Content-Type", "application/json");
            if let Some(a) = &auth {
                c = c.header("Authorization", a);
            }
            if let Some(k) = key {
                c = c.header("Idempotency-Key", k);
            }
            c.send(Json::Object(fields.clone()).to_string())
        };
        let elapsed = t0.elapsed().as_secs_f64();
        match sent {
            Ok(mut resp) => {
                let status = resp.status().as_u16();
                // há mais páginas? (`Link: …; rel="next"`): quem não achou não pode concluir
                let more = resp
                    .headers()
                    .get("link")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|l| l.contains("rel=\"next\""));
                match resp.body_mut().read_to_string() {
                    Ok(body) => Ok((status, body, elapsed, more)),
                    // a resposta foi cortada: o efeito pode ter acontecido
                    Err(_) => Err(HostError::Timeout { elapsed }),
                }
            }
            // a conexão não abriu: nada saiu do processo
            Err(ureq::Error::ConnectionFailed | ureq::Error::HostNotFound) => {
                Err(HostError::Unreachable(format!("{} inacessível", self.scrub_text(&b.base))))
            }
            Err(ureq::Error::Io(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                Err(HostError::Unreachable(format!("{} inacessível", self.scrub_text(&b.base))))
            }
            // o TLS não foi estabelecido (certificado recusado, protocolo): a requisição, com o
            // segredo, nunca foi escrita na conexão. Nada saiu, e o motivo é dito.
            Err(e @ (ureq::Error::Tls(_) | ureq::Error::Rustls(_))) => {
                Err(HostError::Unreachable(format!("TLS com {} recusado: {}", self.scrub_text(&b.base), self.scrub_text(&e.to_string()))))
            }
            // qualquer outra falha depois de começar a enviar: resultado desconhecido
            // o certificado do servidor foi recusado: isso acontece no aperto de mão, antes de
            // qualquer dado da aplicação (a requisição, o segredo) ser escrito
            Err(ureq::Error::Io(e)) if format!("{e:?}").contains("InvalidCertificate") => {
                Err(HostError::Unreachable(format!(
                    "o certificado de {} foi recusado ({}); nada foi enviado. As raízes de confiança são as públicas, ou as de `SSL_CERT_FILE`",
                    self.scrub_text(&b.base),
                    self.scrub_text(&e.to_string())
                )))
            }
            Err(_) => Err(HostError::Timeout { elapsed }),
        }
    }

    /// v1.0-RC9: uma chamada a uma ferramenta ligada por MCP.
    ///
    /// | o que o servidor responde | o que a linguagem vê |
    /// |---|---|
    /// | o pedido não saiu (o servidor não inicia, não confere, não recebe) | `Unreachable` |
    /// | resultado, `isError: false` | o valor no caminho, conferido pelo monitor; um rótulo `_meta.ifc` não confiável num resultado declarado confiável é culpa do provedor (`R0649`) |
    /// | `isError: true`, `input_required`, erro JSON-RPC, violação | operação com chave: `Timeout` (pode ter acontecido; a consulta decide); leitura: `Failed` |
    /// | sem resposta no prazo, o processo morreu | `Timeout` |
    fn call_mcp(&mut self, tool: &str, args: &[Value]) -> Result<(Value, f64), HostError> {
        let t = &self.mcp_tools[tool];
        let (server, keyed, unit) = (t.server, t.keyed, t.ret == "Unit");
        let key =
            args.iter().find_map(|v| if let Value::Key(k) = v { Some(format!("{}:k{k}", self.run)) } else { None });
        let arguments = self.mcp_args(t, &t.call.clone(), args, key.as_deref()).map_err(HostError::Failed)?;
        let name = t.call.tool.clone();
        let call = t.call.clone();
        let trusted = t.trusted;
        let unknown =
            |lat: f64, why: String| if keyed { HostError::Timeout { elapsed: lat } } else { HostError::Failed(why) };
        let t0 = Instant::now();
        let (r, lat) = match self.mcp_send(server, &name, arguments) {
            Ok(x) => x,
            Err(Err(why)) => return Err(HostError::Unreachable(self.scrub_text(&why))),
            // sem resposta no prazo, ou o processo morreu depois de receber o pedido: o efeito
            // pode ter acontecido
            Err(Ok(Fail::Timeout | Fail::Died)) => {
                return Err(HostError::Timeout { elapsed: t0.elapsed().as_secs_f64() })
            }
            Err(Ok(f)) => {
                let lat = t0.elapsed().as_secs_f64();
                return Err(unknown(lat, self.scrub_text(&format!("`{name}`: {f}"))));
            }
        };
        if r["resultType"] == "input_required" {
            return Err(unknown(lat, format!("`{name}`: o servidor pediu entrada; a linguagem não a fornece")));
        }
        if r["isError"] == json!(true) {
            let text =
                r["content"].as_array().and_then(|a| a.first()).and_then(|c| c["text"].as_str()).unwrap_or_default();
            return Err(unknown(lat, format!("`{name}`: {}", self.excerpt(text))));
        }
        let conn = self.servers[server].conn.as_ref().expect("conexão");
        let doc = match BoundHost::mcp_doc(conn, &name, &r) {
            Ok(d) => self.scrub(&d),
            Err(why) => return Err(unknown(lat, why)),
        };
        // o rótulo do servidor só rebaixa: não confiável num resultado declarado confiável é
        // culpa do provedor (o valor não entra); confiável não muda nada
        if trusted && !unit && r["_meta"]["ifc"]["integrity"] == "untrusted" {
            self.blame = Some(format!(
                "R0649: o servidor rotula o resultado de `{name}` como não confiável (`_meta.ifc`), e o programa o declara confiável"
            ));
        }
        let v = if unit {
            Json::Null
        } else {
            match &call.list {
                None => mcp::at(&doc, &call.path).cloned().unwrap_or(Json::Null),
                Some(l) => match mcp::at(&doc, l).and_then(|x| x.as_array()) {
                    Some(a) => {
                        Json::Array(a.iter().map(|e| mcp::at(e, &call.path).cloned().unwrap_or(Json::Null)).collect())
                    }
                    None => Json::Null,
                },
            }
        };
        self.last_raw = Some(v.clone());
        Ok((if unit { Value::Unit } else { BoundHost::value(&v) }, lat))
    }

    /// v1.0-RC9: a consulta por chave por MCP. A ferramenta de leitura de `.find` devolve a
    /// lista em que a operação estaria; ela é o elemento que carrega o marcador.
    fn find_mcp(&mut self, tool: &str, key: u64, args: &[Value]) -> (Status, f64) {
        let t = &self.mcp_tools[tool];
        let Some(f) = t.find.clone() else { return (Status::Unknown, 0.0) };
        let (server, settle, unit) = (t.server, t.settle, t.ret == "Unit");
        let mark = marker(&format!("{}:k{key}", self.run));
        let Ok(arguments) = self.mcp_args(t, &f, args, None) else { return (Status::Unknown, 0.0) };
        let per = f.args.iter().find_map(|(k, a)| match a {
            McpArg::Num(n) if k.eq_ignore_ascii_case("perPage") || k == "per_page" || k == "limit" => Some(*n),
            _ => None,
        });
        let mut total = 0.0;
        for attempt in 0..2 {
            let (r, lat) = match self.mcp_send(server, &f.tool, arguments.clone()) {
                Ok(x) => x,
                Err(_) => return (Status::Unknown, total),
            };
            total += lat;
            // um resultado de erro, um pedido de entrada, ou um rótulo do servidor que contradiz
            // um resultado declarado confiável: a consulta não decide
            let contradicts = self.mcp_tools[tool].trusted && !unit && r["_meta"]["ifc"]["integrity"] == "untrusted";
            if r["isError"] == json!(true) || r["resultType"] == "input_required" || contradicts {
                return (Status::Unknown, total);
            }
            let conn = self.servers[server].conn.as_ref().expect("conexão");
            let Ok(doc) = BoundHost::mcp_doc(conn, &f.tool, &r) else { return (Status::Unknown, total) };
            let doc = self.scrub(&doc);
            let Some(items) = mcp::at(&doc, f.list.as_deref().unwrap_or_default()).and_then(|x| x.as_array()) else {
                return (Status::Unknown, total);
            };
            if let Some(e) = items.iter().find(|e| mcp::carries(e, &mark)) {
                let v = if unit { Json::Null } else { mcp::at(e, &f.path).cloned().unwrap_or(Json::Null) };
                self.last_raw = Some(v.clone());
                return (Status::Done(if unit { Value::Unit } else { BoundHost::value(&v) }), total);
            }
            // ausência numa página que pode não ser a última não é prova de ausência
            let more = doc["pageInfo"]["hasNextPage"] == json!(true) || per.is_some_and(|p| items.len() as i64 >= p);
            if more {
                return (Status::Unknown, total);
            }
            if attempt == 0 && settle > 0 {
                std::thread::sleep(Duration::from_secs(settle as u64));
                total += settle as f64;
                continue;
            }
            return (Status::NotApplied, total);
        }
        (Status::Unknown, total)
    }

    fn value(j: &Json) -> Value {
        match j {
            Json::String(s) => Value::Text(s.clone()),
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) if n.is_i64() => Value::Int(n.as_i64().unwrap_or_default()),
            Json::Null => Value::Unit,
            Json::Array(a) => Value::List(a.iter().map(BoundHost::value).collect()),
            other => Value::Text(other.to_string()),
        }
    }

    fn excerpt(&self, body: &str) -> String {
        let s: String = self.scrub_text(body).chars().take(200).collect();
        s.replace('\n', " ")
    }
}

impl Host for BoundHost {
    fn call(&mut self, tool: &str, args: &[Value]) -> Result<(Value, f64), HostError> {
        if self.mcp_tools.contains_key(tool) {
            return self.call_mcp(tool, args);
        }
        let Some(b) = self.tools.get(tool) else { return self.inner.call(tool, args) };
        let key =
            args.iter().find_map(|v| if let Value::Key(k) = v { Some(format!("{}:k{k}", self.run)) } else { None });
        let mut fields = Map::new();
        for (name, v) in b.params.iter().zip(args) {
            if let Some(n) = name {
                fields.insert(n.clone(), v.to_json());
            }
        }
        // v1.0-RC6: num provedor que ignora `Idempotency-Key`, a chave viaja como marcador no
        // fim de um parâmetro de texto (só no fio: o valor do programa não muda)
        if let (Some(p), Some(k)) = (&b.marker, &key) {
            let text = fields.get(p).and_then(|v| v.as_str()).unwrap_or_default().to_string();
            fields.insert(p.clone(), json!(format!("{text}\n\n{}", marker(k))));
        }
        let route = b.route.clone();
        let (status, body, lat, _) = self.http(b, &route, &fields, key.as_deref())?;
        match status {
            // O efeito concluído: 200, 201, 204. Um `202 Accepted` (ou qualquer outro 2xx) diz
            // que o pedido foi recebido, não que o efeito que a ferramenta promete aconteceu:
            // é resultado desconhecido, e a consulta por chave (`status`) é quem o decide.
            200 | 201 | 204 => {
                let doc: Json = serde_json::from_str(&body).unwrap_or(Json::String(body));
                let doc = self.scrub(&doc);
                if let (Some(k), true) = (&key, doc["receipt"].is_object()) {
                    self.receipts.insert(k.clone(), doc["receipt"].clone());
                }
                let v = extract(&route.field, &doc);
                self.last_raw = Some(v.clone());
                Ok((BoundHost::value(&v), lat))
            }
            202..=299 | 408 | 500..=599 => Err(HostError::Timeout { elapsed: lat }),
            s => Err(HostError::Failed(format!("HTTP {s}: {}", self.excerpt(&body)))),
        }
    }

    fn status(&mut self, tool: &str, key: u64) -> (Status, f64) {
        let Some(b) = self.tools.get(tool) else { return self.inner.status(tool, key) };
        let Some(route) = b.status.clone() else { return (Status::Unknown, 0.0) };
        let wire = format!("{}:k{key}", self.run);
        let mut fields = Map::new();
        fields.insert("tool".into(), json!(tool));
        fields.insert("key".into(), json!(wire));
        match self.http(b, &route, &fields, None) {
            Ok((200..=299, body, lat, _)) => {
                let doc: Json = self.scrub(&serde_json::from_str(&body).unwrap_or(Json::Null));
                if doc["receipt"].is_object() {
                    self.receipts.insert(wire, doc["receipt"].clone());
                }
                let s = match doc["status"].as_str() {
                    Some("done") => {
                        self.last_raw = Some(doc["value"].clone());
                        Status::Done(BoundHost::value(&doc["value"]))
                    }
                    Some("not_applied") => Status::NotApplied,
                    _ => Status::Unknown,
                };
                (s, lat)
            }
            Ok((_, _, lat, _)) => (Status::Unknown, lat),
            Err(_) => (Status::Unknown, 0.0),
        }
    }

    /// **A consulta por chave numa API que não conhece chaves (v1.0-RC6).** `.find` lista o
    /// recurso em que a operação foi criada; a operação é o elemento que carrega o marcador.
    /// Achado: `Done`, com o campo que a rota nomeia. Não achado, numa lista completa:
    /// `NotApplied`. Não achado e há mais páginas, ou a consulta falhou: `Unknown` (ausência
    /// numa página não é prova de ausência).
    fn status_with(&mut self, tool: &str, key: u64, args: &[Value]) -> (Status, f64) {
        if self.mcp_tools.contains_key(tool) {
            return self.find_mcp(tool, key, args);
        }
        let Some(b) = self.tools.get(tool) else { return self.inner.status_with(tool, key, args) };
        let Some(find) = b.find.clone() else { return self.status(tool, key) };
        let wire = format!("{}:k{key}", self.run);
        let mark = marker(&wire);
        // só os parâmetros do caminho da consulta saem: o resto dos argumentos não é filtro
        let wanted = find.path_params();
        let mut fields = Map::new();
        for (name, v) in b.params.iter().zip(args) {
            if let Some(n) = name.as_ref().filter(|n| wanted.contains(&n.as_str())) {
                fields.insert(n.clone(), v.to_json());
            }
        }
        if fields.len() != wanted.len() {
            return (Status::Unknown, 0.0);
        }
        // Uma listagem pode demorar a refletir uma escrita (a do GitHub demora). Não achar a
        // operação só é "não aplicada" depois de esperar `.settle` segundos e consultar de novo:
        // ausência logo depois de um envio possível não é prova de ausência.
        let settle = b.settle;
        let mut total = 0.0;
        for attempt in 0..2 {
            match self.http(b, &find, &fields, None) {
                Ok((200, body, lat, more)) => {
                    total += lat;
                    let doc: Json = self.scrub(&serde_json::from_str(&body).unwrap_or(Json::Null));
                    let Some(items) = doc.as_array() else { return (Status::Unknown, total) };
                    let carries = |e: &Json| {
                        e.as_object().is_some_and(|o| o.values().any(|v| v.as_str().is_some_and(|s| s.contains(&mark))))
                    };
                    match items.iter().find(|e| carries(e)) {
                        Some(e) => {
                            let field = find.field.as_ref().map(|f| f.strip_prefix("[].").unwrap_or(f).to_string());
                            let v = extract(&field, e);
                            self.last_raw = Some(v.clone());
                            return (Status::Done(BoundHost::value(&v)), total);
                        }
                        None if more => return (Status::Unknown, total),
                        None if attempt == 0 && settle > 0 => {
                            std::thread::sleep(Duration::from_secs(settle as u64));
                            total += settle as f64;
                        }
                        None => return (Status::NotApplied, total),
                    }
                }
                Ok((_, _, lat, _)) => return (Status::Unknown, total + lat),
                Err(_) => return (Status::Unknown, total),
            }
        }
        (Status::Unknown, total)
    }

    fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.tools.is_empty() {
            let mut hosts: Vec<String> = self.tools.values().map(|b| self.scrub_text(&b.base)).collect();
            hosts.sort();
            hosts.dedup();
            out.push(format!("{} ferramenta(s) ligada(s) por HTTP a {}", self.tools.len(), hosts.join(", ")));
        }
        for (i, s) in self.servers.iter().enumerate() {
            let n = self.mcp_tools.values().filter(|t| t.server == i).count();
            let pins: Vec<String> = s.pins.iter().map(|(k, v)| format!("{k} #{v}")).collect();
            out.push(self.scrub_text(&format!(
                "{n} ferramenta(s) ligada(s) por MCP (stdio) a {}; conferidas contra o servidor antes de qualquer efeito ({}); descrições e instruções do servidor ignoradas",
                s.who,
                pins.join(", ")
            )));
            if !s.changed.is_empty() {
                out.push(format!(
                    "o servidor mudou a definição de {} entre conexões; a conferência foi refeita",
                    s.changed.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if s.starts > 1 {
                out.push(format!(
                    "o servidor foi iniciado {} vez(es) (reiniciado depois de queda, prazo ou violação)",
                    s.starts
                ));
            }
            let mut by: BTreeMap<&str, usize> = BTreeMap::new();
            for m in &s.refused {
                *by.entry(m.as_str()).or_default() += 1;
            }
            let shown: Vec<String> = by.iter().map(|(m, n)| format!("{m} ×{n}")).collect();
            out.push(format!(
                "{} pedido(s) do servidor ao cliente recusado(s){}",
                s.refused.len(),
                if shown.is_empty() { String::new() } else { format!(" ({})", shown.join(", ")) }
            ));
        }
        out
    }

    fn provider_blame(&mut self) -> Option<String> {
        self.blame.take()
    }

    fn applied(&self, tool: &str, key: u64) -> Option<bool> {
        self.inner.applied(tool, key)
    }

    fn state(&self) -> Json {
        self.inner.state()
    }

    fn trusted_snapshot(&self) -> TrustedSnapshot {
        self.inner.trusted_snapshot()
    }

    fn attestation(&self) -> Option<(String, String)> {
        self.inner.attestation()
    }

    fn receipt(&mut self, wire: &str) -> Option<Json> {
        self.receipts.remove(wire).or_else(|| self.inner.receipt(wire))
    }

    fn raw(&mut self) -> Option<Json> {
        self.last_raw.take().or_else(|| self.inner.raw())
    }
}

/// **A conferência na ligação (v1.0-RC9).** O que o servidor anuncia da ferramenta `c.tool`,
/// contra o que o programa declara da ferramenta `tool` que ela implementa. Devolve a definição
/// conferida. Recusa (`R0646`) se a ferramenta não existe, se um argumento que a chamada envia
/// não existe ou tem outro tipo no esquema de entrada, se um argumento que o servidor exige não é
/// enviado, se um literal está fora do `enum`, ou se o caminho lido não existe ou tem outro tipo
/// no esquema de saída. Recusa (`R0647`) se o servidor afirma que muda o mundo uma ferramenta que
/// o programa trata como leitura (uma `reversible`, ou a consulta de `.find`). O que promete
/// segurança (`readOnlyHint: true`, `idempotentHint`) não muda nada.
fn conform<'c>(
    conn: &'c Conn,
    bind: &str,
    tool: &str,
    t: &McpTool,
    c: &McpCall,
    find: bool,
) -> Result<&'c Json, String> {
    let role = if find { format!("`{tool}.find`") } else { format!("`{tool}`") };
    let refuse = |why: String| format!("R0646: `bind {bind}`: {role} → `{}`: {why}; nada foi executado", c.tool);
    let def = conn.tools.get(&c.tool).ok_or_else(|| refuse("o servidor não oferece esta ferramenta".into()))?;
    let input = &def["inputSchema"];
    for (k, a) in &c.args {
        let ts = mcp::types_at(input, &[Seg::Field(k.clone())]);
        if ts.is_empty() {
            if input["additionalProperties"] == json!(false) {
                return Err(refuse(format!("o esquema de entrada não tem `{k}`")));
            }
            continue;
        }
        let sends = match a {
            McpArg::Param(p) => match t.bases.get(p).map(String::as_str) {
                Some("Int") => "integer",
                _ => "string",
            },
            McpArg::IntOf(_) | McpArg::Num(_) => "integer",
            McpArg::Arg(_) | McpArg::Str(_) => "string",
        };
        let fits = ts.contains("any") || ts.contains(sends) || (sends == "integer" && ts.contains("number"));
        if !fits {
            return Err(refuse(format!(
                "`{k}` é {} no esquema do servidor, e a ligação envia {sends}",
                ts.iter().cloned().collect::<Vec<_>>().join(" | ")
            )));
        }
        if let McpArg::Str(v) = a {
            if let Some(e) = input["properties"][k]["enum"].as_array() {
                if !e.iter().any(|x| x == v) {
                    return Err(refuse(format!("`{k}: '{v}'` está fora do `enum` do servidor")));
                }
            }
        }
    }
    for r in input["required"].as_array().into_iter().flatten().filter_map(|r| r.as_str()) {
        if !c.args.iter().any(|(k, _)| k == r) {
            return Err(refuse(format!("o servidor exige `{r}`, que a ligação não envia")));
        }
    }
    let ann = &def["annotations"];
    let writes = ann["readOnlyHint"] == json!(false) || ann["destructiveHint"] == json!(true);
    if writes && (find || t.reversible) {
        return Err(format!(
            "R0647: `bind {bind}`: o servidor afirma que `{}` muda o mundo (`readOnlyHint: {}`, `destructiveHint: {}`), e o programa a usa como {}; nada foi executado",
            c.tool,
            ann["readOnlyHint"],
            ann["destructiveHint"],
            if find { "a consulta por chave de `.find`" } else { "uma leitura (`reversible`)" }
        ));
    }
    let out = &def["outputSchema"];
    if out.is_object() && (c.projected || find) && !(t.ret == "Unit" && !find) {
        let mut path: Vec<Seg> = Vec::new();
        if let Some(l) = &c.list {
            path.extend(l.iter().map(|x| Seg::Field(x.clone())));
            let ts = mcp::types_at(out, &path);
            if ts.is_empty() || !(ts.contains("array") || ts.contains("any")) {
                return Err(refuse(format!("`{}` não é uma lista no esquema de saída do servidor", l.join("."))));
            }
            path.push(Seg::Items);
        }
        path.extend(c.path.iter().map(|x| Seg::Field(x.clone())));
        if t.ret != "Unit" || !c.path.is_empty() {
            let ts = mcp::types_at(out, &path);
            if ts.is_empty() {
                return Err(refuse(format!("`{}` não existe no esquema de saída do servidor", c.shown_path())));
            }
            let bad = mcp::incompatible(&t.ret, &ts);
            if !bad.is_empty() {
                return Err(refuse(format!(
                    "`{}` é {} no esquema de saída do servidor, e o programa declara `{}`",
                    c.shown_path(),
                    bad.join(" | "),
                    t.ret
                )));
            }
        }
    }
    Ok(def)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Achado 3 da revisão da RC5.** Só o próprio host é local. Um literal IPv6 qualquer,
    /// um endereço mapeado, ou `localhost` na parte de credenciais de outra URL não são.
    #[test]
    fn only_the_own_host_is_loopback() {
        for u in ["http://127.0.0.1:8080/v1", "http://localhost", "http://[::1]:9000/x", "http://127.9.9.9"] {
            assert!(loopback(u), "{u}");
        }
        for u in [
            "http://[::ffff:198.51.100.10]",
            "http://[2001:db8::1]:80/",
            "http://localhost:x@evil.example/",
            "http://127.0.0.1@evil.example/",
            "http://localhost.evil.example",
            "http://198.51.100.10",
            "http://[",
        ] {
            assert!(!loopback(u), "{u} foi classificado como o próprio host");
        }
    }
}
