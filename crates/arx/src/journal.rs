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
