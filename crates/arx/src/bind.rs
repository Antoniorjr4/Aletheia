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
//! | 2xx | o valor (`-> campo`, se a rota o nomeia), conferido contra o tipo |
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
use crate::value::Value;
use ath_syntax::ast::{BindDecl, BindUrl, Item, Module, Route, TypeExpr};
use serde_json::{json, Map, Value as Json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const R0641: &str = "o programa liga ferramentas a provedores reais (`bind`); esta execução não tem a ligação";

struct Bound {
    base: String,
    route: Route,
    /// os nomes dos parâmetros que são dados do provedor, na ordem dos argumentos
    params: Vec<Option<String>>,
    secret: Option<usize>,
    status: Option<Route>,
}

pub struct BoundHost {
    inner: Box<dyn Host>,
    tools: BTreeMap<String, Bound>,
    /// (nome, valor): o valor nunca sai deste módulo, a não ser no cabeçalho da requisição
    secrets: Vec<(String, String)>,
    run: String,
    agent: ureq::Agent,
    last_raw: Option<Json>,
    receipts: BTreeMap<String, Json>,
}

fn loopback(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split(['/', ':']).next().unwrap_or_default();
    matches!(host, "127.0.0.1" | "localhost" | "[")
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
        for b in binds {
            let name = &b.name.name;
            let base = match &b.url {
                BindUrl::Lit(u) => u.clone(),
                BindUrl::Env(v) => env(v).filter(|x| !x.is_empty()).ok_or_else(|| {
                    format!("R0642: `bind {name}`: a variável de ambiente `{v}` (url) não está definida; nada foi executado")
                })?,
            };
            let base = base.trim_end_matches('/').to_string();
            if !(base.starts_with("http://") || base.starts_with("https://")) {
                return Err(format!("R0642: `bind {name}`: a url não é http(s); nada foi executado"));
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
                    Bound { base: base.clone(), route: route.clone(), params, secret, status: b.status.clone() },
                );
            }
        }
        let mut cfg = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs_f64(timeout.max(0.05))))
            .http_status_as_error(false);
        if local {
            cfg = cfg.proxy(None);
        }
        Ok(BoundHost {
            inner,
            tools,
            secrets,
            run: run.to_string(),
            agent: cfg.build().into(),
            last_raw: None,
            receipts: BTreeMap::new(),
        })
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
    ) -> Result<(u16, String, f64), HostError> {
        let mut url = format!("{}{}", b.base, route.path);
        let get = route.method == "GET";
        if get && !fields.is_empty() {
            let q: Vec<String> = fields
                .iter()
                .map(|(k, v)| {
                    let v = v.as_str().map(String::from).unwrap_or_else(|| v.to_string());
                    format!("{}={}", urlencode(k), urlencode(&v))
                })
                .collect();
            url = format!("{url}?{}", q.join("&"));
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
                match resp.body_mut().read_to_string() {
                    Ok(body) => Ok((status, body, elapsed)),
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
            // qualquer outra falha depois de começar a enviar: resultado desconhecido
            Err(_) => Err(HostError::Timeout { elapsed }),
        }
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
        let Some(b) = self.tools.get(tool) else { return self.inner.call(tool, args) };
        let key =
            args.iter().find_map(|v| if let Value::Key(k) = v { Some(format!("{}:k{k}", self.run)) } else { None });
        let mut fields = Map::new();
        for (name, v) in b.params.iter().zip(args) {
            if let Some(n) = name {
                fields.insert(n.clone(), v.to_json());
            }
        }
        let route = b.route.clone();
        let (status, body, lat) = self.http(b, &route, &fields, key.as_deref())?;
        match status {
            200..=299 => {
                let doc: Json = serde_json::from_str(&body).unwrap_or(Json::String(body));
                let doc = self.scrub(&doc);
                if let (Some(k), true) = (&key, doc["receipt"].is_object()) {
                    self.receipts.insert(k.clone(), doc["receipt"].clone());
                }
                let v = match &route.field {
                    Some(f) => doc.get(f).cloned().unwrap_or(Json::Null),
                    None => doc,
                };
                self.last_raw = Some(v.clone());
                Ok((BoundHost::value(&v), lat))
            }
            408 | 500..=599 => Err(HostError::Timeout { elapsed: lat }),
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
            Ok((200..=299, body, lat)) => {
                let doc: Json = self.scrub(&serde_json::from_str(&body).unwrap_or(Json::Null));
                if doc["receipt"].is_object() {
                    self.receipts.insert(wire, doc["receipt"].clone());
                }
                let s = match doc["status"].as_str() {
                    Some("done") => Status::Done(BoundHost::value(&doc["value"])),
                    Some("not_applied") => Status::NotApplied,
                    _ => Status::Unknown,
                };
                (s, lat)
            }
            Ok((_, _, lat)) => (Status::Unknown, lat),
            Err(_) => (Status::Unknown, 0.0),
        }
    }

    fn summary(&self) -> Vec<String> {
        let mut hosts: Vec<String> = self.tools.values().map(|b| self.scrub_text(&b.base)).collect();
        hosts.sort();
        hosts.dedup();
        vec![format!("{} ferramenta(s) ligada(s) por HTTP a {}", self.tools.len(), hosts.join(", "))]
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
