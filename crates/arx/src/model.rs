//! Provedores de modelo. O Marco Zero usa um provedor roteirizado e
//! determinístico; um provedor HTTP real implementa o mesmo trait.

use serde_json::Value as Json;
use std::collections::HashMap;

pub trait ModelProvider {
    fn complete(&mut self, model: &str, prompt: &str) -> Result<String, String>;
}

/// Devolve as respostas na ordem do roteiro; esgotado, repete a última.
#[derive(Default)]
pub struct Scripted {
    responses: HashMap<String, Vec<String>>,
    idx: HashMap<String, usize>,
    pub prompts: Vec<String>,
}

impl Scripted {
    pub fn from_json(j: &Json) -> Result<Self, String> {
        let mut s = Scripted::default();
        let obj = j.as_object().ok_or("roteiro de modelos deve ser um objeto { modelo: [respostas] }")?;
        for (model, list) in obj {
            let arr = list.as_array().ok_or("cada modelo mapeia para uma lista de respostas")?;
            let rs: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            s.responses.insert(model.clone(), rs);
        }
        Ok(s)
    }
}

impl ModelProvider for Scripted {
    fn complete(&mut self, model: &str, prompt: &str) -> Result<String, String> {
        self.prompts.push(prompt.to_string());
        let list = self.responses.get(model).ok_or_else(|| format!("roteiro sem respostas para `{model}`"))?;
        if list.is_empty() {
            return Err(format!("roteiro vazio para `{model}`"));
        }
        let i = self.idx.entry(model.to_string()).or_insert(0);
        let r = list[(*i).min(list.len() - 1)].clone();
        *i += 1;
        Ok(r)
    }
}
