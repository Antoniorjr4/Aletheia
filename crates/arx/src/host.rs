//! Ligações de host: implementações reais das ferramentas declaradas em `.ath`.
//! O Marco Zero traz um mundo simulado (cluster + pager) carregado de JSON.

use crate::value::Value;
use serde_json::Value as Json;
use std::collections::{BTreeMap, HashMap};

pub trait Host {
    /// Executa a ferramenta; devolve o valor e a latência observada em segundos.
    fn call(&mut self, tool: &str, args: &[Value]) -> Result<(Value, f64), String>;
    fn summary(&self) -> Vec<String>;
}

#[derive(Default)]
pub struct SimWorld {
    latency: HashMap<String, f64>,
    logs: HashMap<String, String>,
    pub replicas: BTreeMap<String, i64>,
    pub restarts: BTreeMap<String, u32>,
    pub outbox: Vec<String>,
    /// Chaves já aplicadas → recibo. Repetir uma chave não repete o efeito.
    applied: HashMap<u64, Value>,
}

fn key_of(args: &[Value]) -> Option<u64> {
    args.iter().find_map(|v| if let Value::Key(k) = v { Some(*k) } else { None })
}

impl SimWorld {
    pub fn from_json(j: &Json) -> Result<Self, String> {
        let mut w = SimWorld::default();
        if let Some(m) = j.get("latency").and_then(|v| v.as_object()) {
            for (k, v) in m {
                w.latency.insert(k.clone(), v.as_f64().ok_or("latência deve ser número")?);
            }
        }
        if let Some(m) = j.get("logs").and_then(|v| v.as_object()) {
            for (k, v) in m {
                let text = match v {
                    Json::String(s) => s.clone(),
                    Json::Array(lines) => lines.iter().filter_map(|l| l.as_str()).collect::<Vec<_>>().join("\n"),
                    _ => return Err("logs devem ser texto ou lista de linhas".into()),
                };
                w.logs.insert(k.clone(), text);
            }
        }
        if let Some(m) = j.get("replicas").and_then(|v| v.as_object()) {
            for (k, v) in m {
                w.replicas.insert(k.clone(), v.as_i64().ok_or("réplicas devem ser inteiros")?);
            }
        }
        Ok(w)
    }

    fn lat(&self, tool: &str) -> f64 {
        self.latency.get(tool).copied().unwrap_or(0.5)
    }
}

impl Host for SimWorld {
    fn call(&mut self, tool: &str, args: &[Value]) -> Result<(Value, f64), String> {
        let lat = self.lat(tool);
        if let Some(k) = key_of(args) {
            if let Some(prev) = self.applied.get(&k) {
                return Ok((prev.clone(), lat));
            }
        }
        let out = match tool {
            "logs" => {
                let deploy = args.iter().find_map(|v| if let Value::Text(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
                Value::Text(self.logs.get(&deploy).cloned().unwrap_or_else(|| format!("(sem logs para {deploy})")))
            }
            "apply" => {
                let action = args.iter().find(|v| matches!(v, Value::Adt { .. })).ok_or("apply sem ação")?;
                let Value::Adt { variant, fields, .. } = action else { unreachable!() };
                let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
                let deploy = match get("deploy") {
                    Some(Value::Text(d)) => d,
                    _ => return Err("ação sem `deploy`".into()),
                };
                match variant.as_str() {
                    "Restart" => {
                        *self.restarts.entry(deploy.clone()).or_default() += 1;
                        Value::Text(format!("restart {deploy}"))
                    }
                    "Scale" => {
                        let Some(Value::Int(to)) = get("to") else { return Err("Scale sem `to`".into()) };
                        let from = self.replicas.insert(deploy.clone(), to).unwrap_or(0);
                        Value::Text(format!("scale {deploy} {from}→{to}"))
                    }
                    other => return Err(format!("ação desconhecida pelo host: {other}")),
                }
            }
            "page" => {
                let msg = args.iter().find_map(|v| if let Value::Text(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
                self.outbox.push(msg);
                Value::Unit
            }
            other => return Err(format!("ferramenta `{other}` sem ligação no host")),
        };
        if let Some(k) = key_of(args) {
            self.applied.insert(k, out.clone());
        }
        Ok((out, lat))
    }

    fn summary(&self) -> Vec<String> {
        let mut s = Vec::new();
        for (d, n) in &self.restarts {
            s.push(format!("{d} reiniciado {n}×"));
        }
        for (d, n) in &self.replicas {
            s.push(format!("{d}: {n} réplicas"));
        }
        for m in &self.outbox {
            s.push(format!("pager ← {m:?}"));
        }
        s
    }
}
