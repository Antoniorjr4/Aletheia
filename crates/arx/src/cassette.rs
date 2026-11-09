//! Cassete de replay: tudo o que uma execução real com modelo remoto precisa para ser
//! reproduzida exatamente, por qualquer backend.
//!
//! - `manifest`: o programa (fonte embutida), o agente, o evento, os argumentos e o mundo;
//! - `config`: a configuração do provedor que determina o corpo das requisições (sem a chave);
//! - `exchanges`: cada requisição enviada (corpo exato) e cada resposta HTTP crua recebida, com
//!   a latência medida;
//! - `journal`: o journal encadeado da execução gravada.
//!
//! No replay, o provedor monta as requisições de novo e elas precisam ser idênticas às gravadas;
//! as respostas são interpretadas de novo pelo código do provedor. O journal resultante precisa
//! ser idêntico, byte a byte, ao gravado.

use crate::model::Exchange;
use serde_json::{json, Value as Json};

pub const FORMAT: &str = "aletheia-cassette/1";

#[derive(Clone, Debug)]
pub struct Cassette {
    pub provider: String,
    pub config: Json,
    pub manifest: Json,
    pub exchanges: Vec<Exchange>,
    pub journal: String,
}

impl Cassette {
    pub fn to_json(&self) -> Json {
        json!({
            "format": FORMAT,
            "provider": self.provider,
            "config": self.config,
            "manifest": self.manifest,
            "exchanges": self.exchanges.iter().map(Exchange::to_json).collect::<Vec<_>>(),
            "journal": self.journal,
        })
    }

    pub fn from_json(j: &Json) -> Result<Self, String> {
        if j["format"].as_str() != Some(FORMAT) {
            return Err(format!("formato de cassete desconhecido (esperado `{FORMAT}`)"));
        }
        Ok(Cassette {
            provider: j["provider"].as_str().unwrap_or_default().to_string(),
            config: j["config"].clone(),
            manifest: j["manifest"].clone(),
            exchanges: j["exchanges"]
                .as_array()
                .ok_or("cassete sem `exchanges`")?
                .iter()
                .map(Exchange::from_json)
                .collect::<Result<_, _>>()?,
            journal: j["journal"].as_str().unwrap_or_default().to_string(),
        })
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let s = std::fs::read_to_string(path).map_err(|e| format!("não foi possível ler `{}`: {e}", path.display()))?;
        let j: Json = serde_json::from_str(&s).map_err(|e| format!("cassete inválido `{}`: {e}", path.display()))?;
        Cassette::from_json(&j)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&self.to_json()).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| format!("falha ao gravar `{}`: {e}", path.display()))
    }
}

/// Primeira linha em que dois journals diferem (`None` se idênticos).
pub fn first_divergence(a: &str, b: &str) -> Option<(usize, String, String)> {
    if a == b {
        return None;
    }
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let i = la.iter().zip(&lb).position(|(x, y)| x != y).unwrap_or(la.len().min(lb.len()));
    Some((i, la.get(i).unwrap_or(&"<fim>").to_string(), lb.get(i).unwrap_or(&"<fim>").to_string()))
}
