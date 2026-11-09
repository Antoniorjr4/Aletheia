//! Provedores de modelo. Toda requisição carrega a forma do tipo de saída, de onde cada
//! backend tira a restrição que sabe impor:
//!
//! | Provedor        | Restrição                                   | Válido por construção? |
//! |-----------------|---------------------------------------------|------------------------|
//! | `Scripted`      | nenhuma (devolve o roteiro literal)         | não: o ARX valida      |
//! | `Masked`        | máscara local sobre o autômato do tipo      | sim                    |
//! | `OpenAiCompat`  | `response_format` JSON Schema ou GBNF       | conforme o servidor; o ARX valida |

use ath_grammar::{decode_following, max_bytes, proposal_from_json, to_gbnf, to_json_schema, Shape};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::time::Duration;

pub struct Request<'a> {
    pub model: &'a str,
    pub prompt: &'a str,
    pub shape: &'a Shape,
    /// Teto de tokens de saída: o menor entre o `max_out` do modelo e o teto derivado do tipo.
    pub max_tokens: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Completion {
    pub text: String,
    /// Tokens informados pelo provedor; sem isso, o ARX estima.
    pub tok_in: Option<u64>,
    pub tok_out: Option<u64>,
    pub note: Option<String>,
}

pub trait ModelProvider {
    fn name(&self) -> String;
    fn complete(&mut self, req: &Request) -> Result<Completion, String>;
}

// ------------------------------------------------------------------ roteirizado

/// Devolve as respostas na ordem do roteiro; esgotado, repete a última. Uma resposta pode ser
/// texto bruto ou um objeto JSON (codificado na ordem canônica da forma).
#[derive(Default)]
pub struct Scripted {
    responses: HashMap<String, Vec<Json>>,
    idx: HashMap<String, usize>,
    pub prompts: Vec<String>,
}

impl Scripted {
    pub fn from_json(j: &Json) -> Result<Self, String> {
        let mut s = Scripted::default();
        let obj = j.as_object().ok_or("roteiro de modelos deve ser um objeto { modelo: [respostas] }")?;
        for (model, list) in obj {
            let arr = list.as_array().ok_or("cada modelo mapeia para uma lista de respostas")?;
            s.responses.insert(model.clone(), arr.clone());
        }
        Ok(s)
    }
}

impl ModelProvider for Scripted {
    fn name(&self) -> String {
        "scripted".into()
    }

    fn complete(&mut self, req: &Request) -> Result<Completion, String> {
        self.prompts.push(req.prompt.to_string());
        let list =
            self.responses.get(req.model).ok_or_else(|| format!("roteiro sem respostas para `{}`", req.model))?;
        if list.is_empty() {
            return Err(format!("roteiro vazio para `{}`", req.model));
        }
        let i = self.idx.entry(req.model.to_string()).or_insert(0);
        let r = &list[(*i).min(list.len() - 1)];
        *i += 1;
        let text = match r {
            Json::String(s) => s.clone(),
            other => proposal_from_json(req.shape, other),
        };
        Ok(Completion { text, ..Completion::default() })
    }
}

// ------------------------------------------------------------------ máscara local

/// Decodificação restrita local: o provedor interno propõe o que "quer" emitir e a máscara
/// derivada do tipo só deixa passar continuações viáveis. A saída é válida por construção.
pub struct Masked {
    pub inner: Box<dyn ModelProvider>,
}

impl ModelProvider for Masked {
    fn name(&self) -> String {
        format!("masked({})", self.inner.name())
    }

    fn complete(&mut self, req: &Request) -> Result<Completion, String> {
        let proposal = self.inner.complete(req)?;
        let limit = max_bytes(req.shape).map_or(16_384, |b| b as usize);
        let trace = decode_following(req.shape, &proposal.text, limit)?;
        let note = trace.deviation.map(|d| {
            let allowed: Vec<String> = d.allowed.iter().map(|c| c.to_string()).collect();
            format!(
                "máscara: na posição {}, o modelo queria {}; o tipo só admitia {}",
                d.pos,
                d.wanted.map(|c| format!("`{c}`")).unwrap_or_else(|| "encerrar".into()),
                allowed.join(" | ")
            )
        });
        Ok(Completion { text: trace.text, tok_in: proposal.tok_in, tok_out: None, note })
    }
}

// ------------------------------------------------------------------ HTTP compatível com OpenAI

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constraint {
    /// `response_format: {type: json_schema, strict: true}` — API OpenAI, vLLM, llama.cpp.
    JsonSchema,
    /// `grammar` (GBNF) — servidor do llama.cpp.
    Gbnf,
    /// Sem restrição no servidor; só a validação do ARX.
    None,
}

impl Constraint {
    pub fn parse(s: &str) -> Option<Constraint> {
        match s {
            "json-schema" => Some(Constraint::JsonSchema),
            "gbnf" => Some(Constraint::Gbnf),
            "none" => Some(Constraint::None),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Constraint::JsonSchema => "json-schema",
            Constraint::Gbnf => "gbnf",
            Constraint::None => "none",
        }
    }
}

const SYSTEM: &str = "Você é um componente de um programa Aletheia. Responda somente com o documento JSON pedido, \
no formato {\"value\": …}, sem texto adicional.";

/// Cliente para `POST {base_url}/chat/completions` (API OpenAI, vLLM, llama.cpp, LM Studio, Ollama).
pub struct OpenAiCompat {
    pub base_url: String,
    pub model_id: String,
    pub api_key: Option<String>,
    pub constraint: Constraint,
    agent: ureq::Agent,
}

fn is_local(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split(['/', ':']).next().unwrap_or("");
    matches!(host, "localhost" | "127.0.0.1" | "[")
}

impl OpenAiCompat {
    pub fn new(base_url: &str, model_id: &str, api_key: Option<String>, constraint: Constraint) -> Self {
        let mut cfg =
            ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(180))).http_status_as_error(false);
        if is_local(base_url) {
            cfg = cfg.proxy(None);
        }
        OpenAiCompat {
            base_url: base_url.trim_end_matches('/').to_string(),
            model_id: model_id.to_string(),
            api_key,
            constraint,
            agent: cfg.build().into(),
        }
    }

    /// Corpo da requisição (exposto para inspeção e testes).
    pub fn body(&self, req: &Request) -> Json {
        let mut body = json!({
            "model": self.model_id,
            "messages": [
                { "role": "system", "content": SYSTEM },
                { "role": "user", "content": req.prompt },
            ],
            "max_tokens": req.max_tokens,
            "temperature": 0,
        });
        match self.constraint {
            Constraint::JsonSchema => {
                body["response_format"] = json!({
                    "type": "json_schema",
                    "json_schema": { "name": "aletheia_output", "strict": true, "schema": to_json_schema(req.shape) },
                });
            }
            Constraint::Gbnf => body["grammar"] = json!(to_gbnf(req.shape)),
            Constraint::None => {}
        }
        body
    }
}

impl ModelProvider for OpenAiCompat {
    fn name(&self) -> String {
        format!("openai-compat({}, {})", self.model_id, self.constraint.label())
    }

    fn complete(&mut self, req: &Request) -> Result<Completion, String> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut call = self.agent.post(&url).header("Content-Type", "application/json");
        if let Some(k) = &self.api_key {
            call = call.header("Authorization", &format!("Bearer {k}"));
        }
        let mut resp = call.send(self.body(req).to_string()).map_err(|e| format!("{url}: {e}"))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().map_err(|e| format!("{url}: resposta ilegível: {e}"))?;
        if status != 200 {
            let snippet: String = text.chars().take(300).collect();
            return Err(format!("{url}: HTTP {status}: {snippet}"));
        }
        let j: Json = serde_json::from_str(&text).map_err(|e| format!("{url}: JSON inválido: {e}"))?;
        let content = j["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| format!("{url}: resposta sem choices[0].message.content"))?
            .to_string();
        Ok(Completion {
            text: content,
            tok_in: j["usage"]["prompt_tokens"].as_u64(),
            tok_out: j["usage"]["completion_tokens"].as_u64(),
            note: Some(format!("restrição no servidor: {}", self.constraint.label())),
        })
    }
}
