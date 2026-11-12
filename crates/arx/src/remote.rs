//! O adaptador de ferramentas real (v1.0-RC1): o `Host` que executa cada ferramenta num serviço
//! externo por TCP (`ath-world`), em vez do mundo simulado em memória.
//!
//! * `call` envia a ferramenta, os argumentos e a **chave de fio** `execução:kN`. A chave leva a
//!   identidade da execução: um processo novo que recupera a mesma execução reenvia a mesma
//!   chave, e outra execução nunca colide com ela.
//! * conexão recusada: esta tentativa não saiu do processo (`Unreachable`). Na primeira
//!   tentativa, a operação não aconteceu; numa retentativa, o resultado continua desconhecido;
//! * enviado e sem resposta (prazo, conexão fechada): o efeito pode ou não ter acontecido
//!   (`Timeout`): é o `TimeoutUnknown` da linguagem, e a obrigação segue o protocolo da classe;
//! * resposta de erro do serviço: recusa do provedor, o efeito não aconteceu (`Failed`).
//!
//! A fotografia confiável (`s.…`) **não** vem do serviço: vem de um arquivo de configuração do
//! host. O que o serviço devolve é dado `Untrusted`, como declara o tipo de cada ferramenta.

use crate::host::{Host, HostError, Status, TrustedSnapshot};
use crate::value::Value;
use serde_json::{json, Value as Json};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

pub struct RemoteHost {
    addr: SocketAddr,
    /// a identidade da execução, prefixo de toda chave de fio
    pub run: String,
    /// o prazo de cada resposta
    pub timeout: Duration,
    pub snapshot: TrustedSnapshot,
    /// o BLAKE3 dos bytes de `--trusted` e a assinatura ao lado (`<arquivo>.sig`)
    pub attestation: Option<(String, String)>,
}

enum Wire {
    /// nada foi enviado
    Unreachable(String),
    /// enviado, sem resposta inteira
    Silent(f64),
    Reply(Json, f64),
}

impl RemoteHost {
    pub fn new(addr: &str, run: &str, timeout: f64, snapshot: TrustedSnapshot) -> Result<RemoteHost, String> {
        let addr: SocketAddr =
            addr.trim_start_matches("tcp://").parse().map_err(|_| format!("endereço de host inválido `{addr}`"))?;
        Ok(RemoteHost {
            addr,
            run: run.to_string(),
            timeout: Duration::from_secs_f64(timeout.max(0.05)),
            snapshot,
            attestation: None,
        })
    }

    /// A fotografia confiável de `{"inventories": {"nome": ["item", …]}}`.
    pub fn snapshot_from_json(j: &Json) -> Result<TrustedSnapshot, String> {
        let mut s = TrustedSnapshot::default();
        if let Some(m) = j.get("inventories") {
            for (k, v) in m.as_object().ok_or("`inventories` deve ser um objeto de listas")? {
                let mut items = Vec::new();
                for x in v.as_array().ok_or("inventário deve ser uma lista")? {
                    items.push(match x {
                        Json::String(t) => t.clone(),
                        Json::Number(n) => n.to_string(),
                        _ => return Err(format!("item não textual no inventário `{k}`")),
                    });
                }
                items.sort();
                items.dedup();
                s.inventories.insert(k.clone(), items);
            }
        }
        Ok(s)
    }

    fn wire_key(&self, k: u64) -> String {
        format!("{}:k{k}", self.run)
    }

    fn send(&self, req: &Json) -> Wire {
        let t0 = Instant::now();
        let mut stream = match TcpStream::connect_timeout(&self.addr, self.timeout) {
            Ok(s) => s,
            Err(e) => return Wire::Unreachable(format!("serviço em {} inacessível: {e}", self.addr)),
        };
        let _ = stream.set_read_timeout(Some(self.timeout));
        let _ = stream.set_write_timeout(Some(self.timeout));
        if stream.write_all(format!("{req}\n").as_bytes()).is_err() {
            // parte da requisição pode ter saído: o resultado é desconhecido
            return Wire::Silent(t0.elapsed().as_secs_f64());
        }
        let mut line = String::new();
        match BufReader::new(stream).read_line(&mut line) {
            Ok(n) if n > 0 && line.ends_with('\n') => match serde_json::from_str::<Json>(&line) {
                Ok(j) => Wire::Reply(j, t0.elapsed().as_secs_f64()),
                Err(_) => Wire::Silent(t0.elapsed().as_secs_f64()),
            },
            _ => Wire::Silent(t0.elapsed().as_secs_f64()),
        }
    }

    fn value(j: &Json) -> Value {
        match j {
            Json::String(s) => Value::Text(s.clone()),
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) if n.is_i64() => Value::Int(n.as_i64().unwrap_or_default()),
            Json::Null => Value::Unit,
            other => Value::Text(other.to_string()),
        }
    }
}

impl Host for RemoteHost {
    fn call(&mut self, tool: &str, args: &[Value]) -> Result<(Value, f64), HostError> {
        let key = args.iter().find_map(|v| if let Value::Key(k) = v { Some(self.wire_key(*k)) } else { None });
        // capacidades e a chave não são argumentos do provedor: a autoridade fica no ARX
        let plain: Vec<Json> =
            args.iter().filter(|v| !matches!(v, Value::Cap(_) | Value::Key(_))).map(Value::to_json).collect();
        match self.send(&json!({ "op": "call", "tool": tool, "key": key, "args": plain })) {
            Wire::Unreachable(m) => Err(HostError::Unreachable(m)),
            Wire::Silent(elapsed) => Err(HostError::Timeout { elapsed }),
            Wire::Reply(j, lat) => {
                if j["ok"] == json!(true) {
                    Ok((RemoteHost::value(&j["value"]), lat))
                } else {
                    Err(HostError::Failed(j["error"].as_str().unwrap_or("erro do provedor").to_string()))
                }
            }
        }
    }

    fn status(&mut self, tool: &str, key: u64) -> (Status, f64) {
        match self.send(&json!({ "op": "status", "tool": tool, "key": self.wire_key(key) })) {
            Wire::Reply(j, lat) => {
                let s = match j["status"].as_str() {
                    Some("done") => Status::Done(RemoteHost::value(&j["value"])),
                    Some("not_applied") => Status::NotApplied,
                    _ => Status::Unknown,
                };
                (s, lat)
            }
            Wire::Silent(lat) => (Status::Unknown, lat),
            Wire::Unreachable(_) => (Status::Unknown, 0.0),
        }
    }

    fn summary(&self) -> Vec<String> {
        match self.state() {
            Json::Object(m) => {
                let acc = m.get("accounts").and_then(|a| a.as_object()).map(|a| {
                    a.iter()
                        .map(|(k, v)| format!("{k} {}", v.as_str().unwrap_or_default()))
                        .collect::<Vec<_>>()
                        .join(" · ")
                });
                acc.into_iter().collect()
            }
            _ => vec![format!("serviço em {} sem resposta", self.addr)],
        }
    }

    fn state(&self) -> Json {
        match self.send(&json!({ "op": "state" })) {
            Wire::Reply(j, _) if j["ok"] == json!(true) => j["value"].clone(),
            _ => Json::Null,
        }
    }

    fn trusted_snapshot(&self) -> TrustedSnapshot {
        self.snapshot.clone()
    }

    fn attestation(&self) -> Option<(String, String)> {
        self.attestation.clone()
    }
}
