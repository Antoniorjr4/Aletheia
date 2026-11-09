//! Provedores de modelo. Toda requisição carrega a forma do tipo de saída, de onde cada
//! backend tira a restrição que sabe impor:
//!
//! | Provedor        | Restrição                                   | Válido por construção? |
//! |-----------------|---------------------------------------------|------------------------|
//! | `Scripted`      | nenhuma (devolve o roteiro literal)         | não: o ARX valida      |
//! | `Masked`        | máscara local sobre o autômato do tipo      | sim                    |
//! | `OpenAiCompat`  | `response_format` JSON Schema ou GBNF       | conforme o servidor; o ARX valida |

use ath_grammar::{decode_following, max_bytes, proposal_from_json, to_gbnf, to_json_schema_with, Dialect, Shape};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub struct Request<'a> {
    pub model: &'a str,
    pub prompt: &'a str,
    pub shape: &'a Shape,
    /// Teto de tokens do conteúdo: o menor entre o `max_out` do modelo e o teto derivado do tipo.
    pub max_tokens: u64,
    /// Tokens de raciocínio admitidos pelo contrato do modelo (0 = modelo sem raciocínio).
    pub reasoning: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Completion {
    pub text: String,
    /// Tokens informados pelo provedor; sem isso, o ARX estima.
    pub tok_in: Option<u64>,
    /// Tokens de saída cobrados (conteúdo + raciocínio).
    pub tok_out: Option<u64>,
    pub tok_reasoning: Option<u64>,
    pub reasoning: Option<String>,
    pub note: Option<String>,
    /// Tempo de serviço medido da última tentativa, em segundos.
    pub latency: Option<f64>,
    /// Tempo de espera imposto pelo provedor (429 com Retry-After), em segundos.
    pub queued: f64,
    pub retries: u32,
    pub request_id: Option<String>,
    pub finish_reason: Option<String>,
    /// Modelo que o servidor declara ter servido (campo `model` da resposta).
    pub served_model: Option<String>,
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
        Ok(Completion { text: trace.text, tok_in: proposal.tok_in, note, ..Completion::default() })
    }
}

// ------------------------------------------------------------------ HTTP compatível com OpenAI

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constraint {
    /// `response_format: {type: json_schema, strict: true}` — API OpenAI, Groq, vLLM.
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

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
/// Maior espera aceita num único 429 antes de desistir, em segundos.
const MAX_WAIT: f64 = 60.0;

/// Cliente para `POST {base_url}/chat/completions` (API OpenAI, Groq, vLLM, llama.cpp, LM Studio, Ollama).
pub struct OpenAiCompat {
    pub base_url: String,
    pub model_id: String,
    pub api_key: Option<String>,
    pub constraint: Constraint,
    pub dialect: Dialect,
    /// `reasoning_effort` enviado quando o contrato do modelo admite raciocínio.
    pub reasoning_effort: Option<String>,
    /// Campos extras mesclados no corpo (`ALETHEIA_EXTRA_BODY`); `null` remove um campo.
    pub extra: Option<Json>,
    pub max_retries: u32,
    agent: ureq::Agent,
}

fn is_local(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split(['/', ':']).next().unwrap_or("");
    matches!(host, "localhost" | "127.0.0.1" | "[")
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
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
            dialect: Dialect::Core,
            reasoning_effort: Some("low".into()),
            extra: None,
            max_retries: 6,
            agent: cfg.build().into(),
        }
    }

    /// Configuração pelo ambiente, com precedência para os argumentos da CLI:
    ///
    /// | Variável                     | Uso                                                        |
    /// |------------------------------|------------------------------------------------------------|
    /// | `OPENAI_API_KEY`             | chave (também `ATH_API_KEY`, `GROQ_API_KEY`)               |
    /// | `OPENAI_BASE_URL`            | ex.: `https://api.groq.com/openai/v1` (padrão: API OpenAI) |
    /// | `ALETHEIA_MODEL`             | ex.: `openai/gpt-oss-120b`                                 |
    /// | `ALETHEIA_CONSTRAINT`        | `json-schema` (padrão), `gbnf` ou `none`                   |
    /// | `ALETHEIA_SCHEMA`            | `core` (padrão) ou `full`                                  |
    /// | `ALETHEIA_REASONING_EFFORT`  | `low` (padrão), `medium`, `high` ou `none` para omitir     |
    /// | `ALETHEIA_EXTRA_BODY`        | objeto JSON mesclado no corpo da requisição                |
    pub fn from_env(model: Option<&str>, base_url: Option<&str>) -> Result<Self, String> {
        let model = model
            .map(String::from)
            .or_else(|| env("ALETHEIA_MODEL"))
            .ok_or("informe o modelo remoto com --model ou ALETHEIA_MODEL (ex.: openai/gpt-oss-120b)")?;
        let base = base_url.map(String::from).or_else(|| env("OPENAI_BASE_URL")).unwrap_or(DEFAULT_BASE_URL.into());
        let key = env("OPENAI_API_KEY").or_else(|| env("ATH_API_KEY")).or_else(|| env("GROQ_API_KEY"));
        let constraint = match env("ALETHEIA_CONSTRAINT") {
            Some(c) => Constraint::parse(&c).ok_or("ALETHEIA_CONSTRAINT deve ser json-schema, gbnf ou none")?,
            None => Constraint::JsonSchema,
        };
        let mut p = OpenAiCompat::new(&base, &model, key, constraint);
        if let Some(d) = env("ALETHEIA_SCHEMA") {
            p.dialect = Dialect::parse(&d).ok_or("ALETHEIA_SCHEMA deve ser core ou full")?;
        }
        if let Some(e) = env("ALETHEIA_REASONING_EFFORT") {
            p.reasoning_effort = (e != "none").then_some(e);
        }
        if let Some(x) = env("ALETHEIA_EXTRA_BODY") {
            let j: Json = serde_json::from_str(&x).map_err(|e| format!("ALETHEIA_EXTRA_BODY não é JSON: {e}"))?;
            if !j.is_object() {
                return Err("ALETHEIA_EXTRA_BODY deve ser um objeto JSON".into());
            }
            p.extra = Some(j);
        }
        Ok(p)
    }

    /// Corpo da requisição (exposto para inspeção e testes).
    pub fn body(&self, req: &Request) -> Json {
        let total = req.max_tokens + req.reasoning;
        let mut body = json!({
            "model": self.model_id,
            "messages": [
                { "role": "system", "content": SYSTEM },
                { "role": "user", "content": req.prompt },
            ],
            "temperature": 0,
        });
        match self.constraint {
            Constraint::JsonSchema => {
                body["max_completion_tokens"] = json!(total);
                body["response_format"] = json!({
                    "type": "json_schema",
                    "json_schema": {
                        "name": "aletheia_output",
                        "strict": true,
                        "schema": to_json_schema_with(req.shape, self.dialect),
                    },
                });
            }
            Constraint::Gbnf => {
                body["max_tokens"] = json!(total);
                body["grammar"] = json!(to_gbnf(req.shape));
            }
            Constraint::None => body["max_completion_tokens"] = json!(total),
        }
        if req.reasoning > 0 {
            if let Some(e) = &self.reasoning_effort {
                body["reasoning_effort"] = json!(e);
            }
        }
        if let (Some(Json::Object(extra)), Some(obj)) = (&self.extra, body.as_object_mut()) {
            for (k, v) in extra {
                if v.is_null() {
                    obj.remove(k);
                } else {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }
        body
    }
}

/// Segundos a esperar num 429: cabeçalho `retry-after`, a frase "try again in 7.66s" do corpo,
/// ou recuo exponencial.
fn retry_wait(header: Option<&str>, body: &str, attempt: u32) -> f64 {
    if let Some(s) = header.and_then(|h| h.trim().parse::<f64>().ok()) {
        return s.max(0.5);
    }
    if let Some(i) = body.find("try again in ") {
        let rest = &body[i + 13..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        if let Ok(v) = num.parse::<f64>() {
            let unit_ms = rest[num.len()..].starts_with("ms");
            return if unit_ms { (v / 1000.0).max(0.5) } else { v + 0.5 };
        }
    }
    2f64.powi(attempt as i32).min(MAX_WAIT)
}

impl ModelProvider for OpenAiCompat {
    fn name(&self) -> String {
        format!("openai-compat({}, {}/{})", self.model_id, self.constraint.label(), self.dialect.label())
    }

    fn complete(&mut self, req: &Request) -> Result<Completion, String> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = self.body(req).to_string();
        let mut queued = 0.0;
        let mut attempt = 0;
        loop {
            let mut call = self.agent.post(&url).header("Content-Type", "application/json");
            if let Some(k) = &self.api_key {
                call = call.header("Authorization", &format!("Bearer {k}"));
            }
            let t0 = Instant::now();
            let mut resp = call.send(body.as_str()).map_err(|e| format!("{url}: {e}"))?;
            let latency = t0.elapsed().as_secs_f64();
            let status = resp.status().as_u16();
            let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(String::from);
            let retry_after = header("retry-after");
            let header_id = header("x-request-id");
            let text = resp.body_mut().read_to_string().map_err(|e| format!("{url}: resposta ilegível: {e}"))?;

            if (status == 429 || status >= 500) && attempt < self.max_retries {
                let wait = retry_wait(retry_after.as_deref(), &text, attempt);
                if wait > MAX_WAIT {
                    return Err(format!("{url}: HTTP {status}: o provedor pediu {wait:.0} s de espera"));
                }
                std::thread::sleep(Duration::from_secs_f64(wait));
                queued += wait;
                attempt += 1;
                continue;
            }
            if status == 400 && text.contains("json_validate_failed") {
                // Modo best-effort do servidor: a geração existe, mas fora do esquema.
                let j: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
                let failed = j["error"]["failed_generation"].as_str().unwrap_or_default().to_string();
                return Ok(Completion {
                    text: failed,
                    note: Some("servidor: json_validate_failed".into()),
                    latency: Some(latency),
                    queued,
                    retries: attempt,
                    request_id: header_id,
                    ..Completion::default()
                });
            }
            if status != 200 {
                let snippet: String = text.chars().take(400).collect();
                return Err(format!("{url}: HTTP {status}: {snippet}"));
            }
            let j: Json = serde_json::from_str(&text).map_err(|e| format!("{url}: JSON inválido: {e}"))?;
            let msg = &j["choices"][0]["message"];
            let reasoning = msg["reasoning"].as_str().filter(|s| !s.is_empty()).map(String::from);
            return Ok(Completion {
                text: msg["content"].as_str().unwrap_or_default().to_string(),
                tok_in: j["usage"]["prompt_tokens"].as_u64(),
                tok_out: j["usage"]["completion_tokens"].as_u64(),
                tok_reasoning: j["usage"]["completion_tokens_details"]["reasoning_tokens"].as_u64(),
                reasoning,
                note: Some(format!("restrição no servidor: {}/{}", self.constraint.label(), self.dialect.label())),
                latency: Some(latency),
                queued,
                retries: attempt,
                request_id: j["id"].as_str().map(String::from).or(header_id),
                finish_reason: j["choices"][0]["finish_reason"].as_str().map(String::from),
                served_model: j["model"].as_str().map(String::from),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::retry_wait;

    #[test]
    fn retry_wait_reads_header_body_or_backs_off() {
        assert_eq!(retry_wait(Some("3"), "", 0), 3.0);
        let groq =
            "Rate limit reached for model `openai/gpt-oss-120b` ... Please try again in 7.66s. Need more tokens?";
        assert!((retry_wait(None, groq, 0) - 8.16).abs() < 1e-9);
        assert_eq!(retry_wait(None, "try again in 250ms", 0), 0.5);
        assert_eq!(retry_wait(None, "", 3), 8.0);
    }
}
