//! Autorização como capacidade linear (v1.0-RC1).
//!
//! Uma **concessão** (`Grant`) autoriza **uma** operação: a operação nomeada, com exatamente
//! estes argumentos, para esta execução, e registra de onde veio a autoridade. O contrato de uma
//! ferramenta a exige com `s.authorizes("op", x1, …, xn)`; o efeito a **consome**. Duas
//! consequências que a relação `s.binds` da Rev13 não dava:
//!
//! * não há produto cartesiano: cada concessão é uma tupla inteira, confirmada como tupla;
//!   `send 100 → A` e `schedule 250 → B` não autorizam `send 250 → A`;
//! * não há repetição: uma concessão vale um efeito. O consumo é persistido (`<grants>.spent`)
//!   antes do efeito, de modo que um segundo processo com o mesmo arquivo não a reutiliza.
//!
//! De onde vêm as concessões: de uma **confirmação estruturada** do principal (`athc grant`), nunca
//! de uma inferência sobre o pedido. Extrair uma intenção de um texto produz uma *sugestão*; só a
//! confirmação da tupla exata produz autoridade. O conteúdo de um arquivo que o pedido nomeia
//! também é só sugestão: ler é permitido, confiar no conteúdo e autorizar um efeito são passos
//! separados.
//!
//! O modelo formal é `lean/Aletheia/Grant.lean`.

use crate::host::decimal;
use serde_json::{json, Value as Json};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;

/// As origens que conferem autoridade. Uma concessão de qualquer outra origem (uma sugestão
/// extraída do pedido, o conteúdo de um arquivo, a saída de um modelo) é recusada na carga.
pub const ORIGINS: [&str; 2] = ["confirmation", "standing"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// a identidade da concessão: BLAKE3 de todos os outros campos
    pub id: String,
    pub op: String,
    pub args: Vec<String>,
    /// `confirmation` (o principal confirmou esta tupla) ou `standing` (uma ordem permanente do
    /// principal), seguido de `:` e a referência da evidência
    pub origin: String,
    /// a execução para a qual a concessão vale
    pub run: String,
    /// distingue duas concessões da mesma tupla (duas operações iguais confirmadas)
    pub nonce: u64,
}

impl Grant {
    pub fn new(run: &str, op: &str, args: &[String], origin: &str, nonce: u64) -> Grant {
        let canon = format!("{run}\u{1f}{op}\u{1f}{}\u{1f}{origin}\u{1f}{nonce}", args.join("\u{1e}"));
        Grant {
            id: blake3::hash(canon.as_bytes()).to_hex().to_string(),
            op: op.into(),
            args: args.to_vec(),
            origin: origin.into(),
            run: run.into(),
            nonce,
        }
    }

    pub fn to_json(&self) -> Json {
        json!({ "id": self.id, "op": self.op, "args": self.args, "origin": self.origin, "run": self.run,
                "nonce": self.nonce })
    }

    /// A tupla casa: mesma operação, mesma aridade e cada componente igual como texto ou, se os
    /// dois são decimais, como valor (`10.00` = `10.0`).
    pub fn matches(&self, op: &str, xs: &[String]) -> bool {
        let same = |a: &str, b: &str| a == b || matches!((decimal(a), decimal(b)), (Some(x), Some(y)) if x == y);
        self.op == op && self.args.len() == xs.len() && self.args.iter().zip(xs).all(|(a, x)| same(a, x))
    }
}

/// As concessões de uma execução e o registro do que já foi consumido.
#[derive(Debug, Default)]
pub struct GrantStore {
    pub grants: Vec<Grant>,
    pub spent: BTreeSet<String>,
    /// o registro durável do consumo (`<grants>.spent`), se as concessões vieram de arquivo
    ledger: Option<PathBuf>,
}

impl GrantStore {
    /// Lê `{"grants": [{op, args, origin, run, nonce?, id?}, …]}`. Falha fechado: origem que não
    /// confere autoridade, campo ausente, `id` que não confere ou concessão repetida recusam o
    /// arquivo inteiro.
    pub fn from_json(j: &Json) -> Result<GrantStore, String> {
        let list = j.get("grants").and_then(|g| g.as_array()).ok_or("concessões: falta a lista `grants`")?;
        let mut s = GrantStore::default();
        for (i, g) in list.iter().enumerate() {
            let text = |k: &str| {
                g.get(k)
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| format!("concessão {}: falta `{k}`", i + 1))
            };
            let (op, origin, run) = (text("op")?, text("origin")?, text("run")?);
            let kind = origin.split(':').next().unwrap_or_default();
            if !ORIGINS.contains(&kind) {
                return Err(format!(
                    "concessão {}: a origem `{origin}` não confere autoridade (só `confirmation:…` ou `standing:…`)",
                    i + 1
                ));
            }
            let args: Vec<String> = g
                .get("args")
                .and_then(|a| a.as_array())
                .ok_or_else(|| format!("concessão {}: falta `args`", i + 1))?
                .iter()
                .map(|a| match a {
                    Json::String(x) => Ok(x.clone()),
                    Json::Number(n) => Ok(n.to_string()),
                    _ => Err(format!("concessão {}: argumento que não é texto nem número", i + 1)),
                })
                .collect::<Result<_, _>>()?;
            let nonce = g.get("nonce").and_then(|n| n.as_u64()).unwrap_or(0);
            let grant = Grant::new(run, op, &args, origin, nonce);
            if let Some(id) = g.get("id").and_then(|v| v.as_str()) {
                if id != grant.id {
                    return Err(format!("concessão {}: o `id` não confere com o conteúdo", i + 1));
                }
            }
            if s.grants.iter().any(|x| x.id == grant.id) {
                return Err(format!("concessão {}: repetida (use `nonce` para duas operações iguais)", i + 1));
            }
            s.grants.push(grant);
        }
        Ok(s)
    }

    /// As concessões de um arquivo, com o consumo já registrado em `<arquivo>.spent`.
    pub fn open(path: &std::path::Path) -> Result<GrantStore, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("concessões `{}`: {e}", path.display()))?;
        let j: Json = serde_json::from_str(&text).map_err(|e| format!("concessões `{}`: {e}", path.display()))?;
        let mut s = GrantStore::from_json(&j)?;
        let ledger = PathBuf::from(format!("{}.spent", path.display()));
        match std::fs::read_to_string(&ledger) {
            Ok(t) => {
                for (n, line) in t.lines().enumerate() {
                    // falha fechado: um registro de consumo ilegível não libera concessão alguma
                    let e: Json = serde_json::from_str(line)
                        .map_err(|_| format!("consumo `{}`: linha {} ilegível", ledger.display(), n + 1))?;
                    let id = e["grant"]
                        .as_str()
                        .ok_or_else(|| format!("consumo `{}`: linha {} sem `grant`", ledger.display(), n + 1))?;
                    s.spent.insert(id.to_string());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("consumo `{}`: {e}", ledger.display())),
        }
        s.ledger = Some(ledger);
        Ok(s)
    }

    /// A primeira concessão livre desta execução que casa a tupla e não está em `claimed`.
    pub fn find(&self, run: &str, op: &str, xs: &[String], claimed: &[usize]) -> Option<usize> {
        self.grants.iter().enumerate().position(|(i, g)| {
            g.run == run && !self.spent.contains(&g.id) && !claimed.contains(&i) && g.matches(op, xs)
        })
    }

    /// Consome a concessão: persiste o consumo (sincronizado) e só então a marca. Se o registro
    /// não pode ser escrito, o processo para: nenhum efeito sai com a concessão ainda livre.
    pub fn consume(&mut self, i: usize, tool: &str) {
        let g = &self.grants[i];
        if let Some(p) = &self.ledger {
            let line = json!({ "grant": g.id, "run": g.run, "tool": tool }).to_string();
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .unwrap_or_else(|e| panic!("concessões: não foi possível abrir `{}`: {e}", p.display()));
            f.write_all(format!("{line}\n").as_bytes()).and_then(|_| f.sync_all()).unwrap_or_else(|e| {
                panic!("concessões: não foi possível registrar o consumo em `{}`: {e}", p.display())
            });
        }
        self.spent.insert(g.id.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(op: &str, args: &[&str], nonce: u64) -> Json {
        json!({ "op": op, "args": args, "origin": "confirmation:teste", "run": "r1", "nonce": nonce })
    }

    #[test]
    fn a_grant_is_one_whole_tuple() {
        let s = GrantStore::from_json(
            &json!({ "grants": [g("send", &["A", "100.00"], 0), g("schedule", &["B", "250.00"], 0)] }),
        )
        .unwrap();
        let t = |op: &str, a: &str, v: &str| s.find("r1", op, &[a.into(), v.into()], &[]).is_some();
        assert!(t("send", "A", "100.0") && t("schedule", "B", "250"));
        // o produto cartesiano que `intents_of` da Rev13 autorizava
        for (op, a, v) in [
            ("send", "A", "250.00"),
            ("send", "B", "100.00"),
            ("schedule", "A", "100.00"),
            ("schedule", "B", "100.00"),
            ("send", "B", "250.00"),
            ("schedule", "A", "250.00"),
        ] {
            assert!(!t(op, a, v), "{op} {a} {v}");
        }
        // outra execução não usa a concessão
        assert!(s.find("r2", "send", &["A".into(), "100.00".into()], &[]).is_none());
    }

    #[test]
    fn a_grant_is_spent_once_and_the_spending_is_durable() {
        let dir = std::env::temp_dir().join(format!("grants-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("g.json");
        std::fs::write(&p, json!({ "grants": [g("send", &["A", "1"], 0), g("send", &["A", "1"], 1)] }).to_string())
            .unwrap();
        let mut s = GrantStore::open(&p).unwrap();
        let xs = ["A".to_string(), "1".to_string()];
        let i = s.find("r1", "send", &xs, &[]).unwrap();
        // duas operações iguais na mesma proposta pedem duas concessões
        let j = s.find("r1", "send", &xs, &[i]).unwrap();
        assert_ne!(i, j);
        assert!(s.find("r1", "send", &xs, &[i, j]).is_none());
        s.consume(i, "send");
        // outro processo, mesmo arquivo: a consumida não volta
        let mut s2 = GrantStore::open(&p).unwrap();
        let k = s2.find("r1", "send", &xs, &[]).unwrap();
        assert_eq!(k, j);
        s2.consume(k, "send");
        assert!(GrantStore::open(&p).unwrap().find("r1", "send", &xs, &[]).is_none());
        // um registro de consumo ilegível recusa a carga (não libera nada)
        std::fs::write(format!("{}.spent", p.display()), "lixo\n").unwrap();
        assert!(GrantStore::open(&p).is_err());
    }

    #[test]
    fn only_a_confirmation_confers_authority() {
        for origin in ["request", "extracted:pedido", "file:bill.txt", "model", ""] {
            let j = json!({ "grants": [{ "op": "send", "args": ["A", "1"], "origin": origin, "run": "r1" }] });
            assert!(GrantStore::from_json(&j).is_err(), "{origin}");
        }
        let dup = json!({ "grants": [g("send", &["A", "1"], 0), g("send", &["A", "1"], 0)] });
        assert!(GrantStore::from_json(&dup).is_err());
        let forged = json!({ "grants": [{ "id": "00", "op": "send", "args": ["A", "1"], "origin": "confirmation:x", "run": "r1" }] });
        assert!(GrantStore::from_json(&forged).is_err());
    }
}
