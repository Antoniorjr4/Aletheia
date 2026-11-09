//! Journal append-only com encadeamento por hash: cada entrada carrega o hash
//! da anterior, formando a cadeia de custódia de todo efeito.

use serde_json::{json, Value};

pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

pub struct Journal {
    pub entries: Vec<Value>,
    pub head: String,
}

impl Default for Journal {
    fn default() -> Self {
        Journal { entries: Vec::new(), head: GENESIS.to_string() }
    }
}

fn entry_hash(body: &Value) -> String {
    blake3::hash(body.to_string().as_bytes()).to_hex().to_string()
}

impl Journal {
    pub fn push(&mut self, t: f64, kind: &str, detail: Value) {
        let body = json!({
            "seq": self.entries.len(),
            "t": (t * 1000.0).round() / 1000.0,
            "kind": kind,
            "detail": detail,
            "prev": self.head,
        });
        let h = entry_hash(&body);
        let mut entry = body;
        entry["hash"] = json!(h);
        self.head = h;
        self.entries.push(entry);
    }

    pub fn to_jsonl(&self) -> String {
        let mut s = String::new();
        for e in &self.entries {
            s.push_str(&e.to_string());
            s.push('\n');
        }
        s
    }

    /// Recalcula a cadeia; devolve o índice da primeira entrada adulterada.
    pub fn verify(entries: &[Value]) -> Result<(), usize> {
        let mut prev = GENESIS.to_string();
        for (i, e) in entries.iter().enumerate() {
            let mut body = e.clone();
            let Some(obj) = body.as_object_mut() else { return Err(i) };
            let Some(h) = obj.remove("hash") else { return Err(i) };
            if obj.get("prev").and_then(|p| p.as_str()) != Some(prev.as_str()) {
                return Err(i);
            }
            if h.as_str() != Some(entry_hash(&body).as_str()) {
                return Err(i);
            }
            prev = h.as_str().unwrap_or_default().to_string();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Latências medidas e somas de custo produzem floats longos (`3.3098843550000003`,
    /// `9.630000000000001e-05`). O journal relido do disco precisa reproduzir o mesmo hash:
    /// sem leitura exata de floats, o auditor acusaria adulteração num journal íntegro.
    #[test]
    fn floats_medidos_sobrevivem_a_releitura() {
        let mut j = Journal::default();
        j.push(0.551_234_567_8, "infer", json!({"latency_s": 0.551_234_567_890_123_4, "usd": 1.0e-5 / 3.0}));
        j.push(3.31, "end", json!({"spent": {"tok": 516, "usd": 9.630000000000001e-05, "wall": 3.3098843550000003}}));
        let reread: Vec<Value> = j.to_jsonl().lines().map(|l| serde_json::from_str(l).expect("linha JSON")).collect();
        assert_eq!(Journal::verify(&reread), Ok(()));
    }
}
