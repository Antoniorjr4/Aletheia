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

    /// Acrescenta uma entrada sem carimbo de tempo: o formato do journal do núcleo.
    fn push_untimed(&mut self, kind: &str, detail: Value) {
        let body = json!({ "seq": self.entries.len(), "kind": kind, "detail": detail, "prev": self.head });
        let h = entry_hash(&body);
        let mut entry = body;
        entry["hash"] = json!(h);
        self.head = h;
        self.entries.push(entry);
    }

    /// A vista do núcleo λ_Ath de um journal do ARX: só os eventos observáveis que a semântica
    /// formal (`lean/Aletheia/Eval.lean`) define, encadeados de novo por BLAKE3, sem tempo.
    ///
    /// * `tool` → `act {args, cap, tool}`: `cap` é o conjunto de efeitos da capacidade que deu
    ///   autoridade à chamada, `args` os demais argumentos;
    /// * `endorse` → `endorse {accepted: true, policy, value}`;
    /// * `refine_exhausted` → `endorse {accepted: false, policy, value: null}`;
    /// * `end` → `end {value}`.
    ///
    /// O resto (início, inferências, rejeições intermediárias, snapshot, sagas) não é evento do
    /// núcleo e fica de fora. O avaliador do Lean (`ath-lean-eval`) e o runtime Python
    /// (`aletheia_runtime.journal.core_view`) produzem esta mesma vista, byte a byte.
    pub fn core_view(entries: &[Value]) -> Journal {
        let mut j = Journal::default();
        for e in entries {
            let d = &e["detail"];
            match e["kind"].as_str() {
                Some("tool") => {
                    let mut cap = Value::Null;
                    let mut args = Vec::new();
                    for a in d["args"].as_array().into_iter().flatten() {
                        match a.get("cap") {
                            Some(c) if cap.is_null() && c.is_array() && a.as_object().is_some_and(|o| o.len() == 1) => {
                                cap = c.clone()
                            }
                            _ => args.push(a.clone()),
                        }
                    }
                    j.push_untimed("act", json!({ "args": args, "cap": cap, "tool": d["tool"] }));
                }
                Some("endorse") => {
                    j.push_untimed("endorse", json!({ "accepted": true, "policy": d["policy"], "value": d["value"] }))
                }
                Some("refine_exhausted") => {
                    j.push_untimed("endorse", json!({ "accepted": false, "policy": d["policy"], "value": null }))
                }
                Some("end") => j.push_untimed("end", json!({ "value": d["result"] })),
                _ => {}
            }
        }
        j
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

    /// A vista do núcleo: só `act`/`endorse`/`end`, a capacidade separada dos argumentos (um
    /// ADT com campo chamado `cap` não é capacidade) e a cadeia refeita sem tempo.
    #[test]
    fn vista_do_nucleo() {
        let mut j = Journal::default();
        j.push(0.0, "start", json!({ "agent": "A" }));
        j.push(0.1, "infer", json!({ "model": "m0" }));
        j.push(0.1, "policy_reject", json!({ "policy": "P", "value": 1, "attempt": 1 }));
        j.push(0.1, "refine_exhausted", json!({ "policy": "P", "attempts": 1 }));
        j.push(0.2, "endorse", json!({ "policy": "P", "value": 5, "attempt": 1 }));
        j.push(
            0.3,
            "tool",
            json!({ "tool": "t", "args": [{ "Scale": { "cap": 1 } }, { "cap": ["E.B", "E.A"] }, 5], "latency": 0.1 }),
        );
        j.push(0.4, "end", json!({ "result": 6, "spent": {} }));
        let core = Journal::core_view(&j.entries);
        let kinds: Vec<_> = core.entries.iter().map(|e| e["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["endorse", "endorse", "act", "end"]);
        assert_eq!(core.entries[0]["detail"], json!({ "accepted": false, "policy": "P", "value": null }));
        assert_eq!(core.entries[2]["detail"]["cap"], json!(["E.B", "E.A"]));
        assert_eq!(core.entries[2]["detail"]["args"], json!([{ "Scale": { "cap": 1 } }, 5]));
        assert!(core.entries.iter().all(|e| e.get("t").is_none()));
        assert_eq!(Journal::verify(&core.entries), Ok(()));
    }

    /// A grafia dos floats no journal é parte do hash: o runtime Python
    /// (`runtimes/python/aletheia_runtime/canon.py`) reproduz esta mesma tabela.
    #[test]
    fn grafia_dos_floats_que_o_runtime_python_reproduz() {
        for (x, s) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (2.0, "2.0"),
            (131072.0, "131072.0"),
            (9.630000000000001e-05, "0.00009630000000000001"),
            (3.3098843550000003, "3.3098843550000003"),
            (1e-5, "0.00001"),
            (1e-6, "1e-6"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (9e15, "9000000000000000.0"),
            (1e16, "1e+16"),
            (1.2345e16, "1.2345e+16"),
            (1e20, "1e+20"),
            (1.5e300, "1.5e+300"),
            (5e-324, "5e-324"),
            (123.456, "123.456"),
        ] {
            assert_eq!(json!(x).to_string(), s, "{x:?}");
        }
    }
}
