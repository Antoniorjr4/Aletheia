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
    /// a assinatura da autoridade sobre a identidade (`ed25519:<hex>`), se houver
    pub sig: Option<String>,
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
            sig: None,
        }
    }

    pub fn to_json(&self) -> Json {
        let mut j = json!({ "id": self.id, "op": self.op, "args": self.args, "origin": self.origin, "run": self.run,
                "nonce": self.nonce });
        if let Some(s) = &self.sig {
            j["sig"] = json!(s);
        }
        j
    }

    /// A tupla casa: mesma operação, mesma aridade e cada componente igual como texto ou, se os
    /// dois são decimais, como valor (`10.00` = `10.0`).
    pub fn matches(&self, op: &str, xs: &[String]) -> bool {
        let same = |a: &str, b: &str| a == b || matches!((decimal(a), decimal(b)), (Some(x), Some(y)) if x == y);
        self.op == op && self.args.len() == xs.len() && self.args.iter().zip(xs).all(|(a, x)| same(a, x))
    }
}

/// Quem consumiu uma concessão, como está no registro de consumo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Owner {
    pub run: String,
    pub tool: String,
    /// a chave da operação (`kN`) e o hash do registro `intent` do WAL que a concessão pagou;
    /// ausentes numa execução sem WAL
    pub key: Option<String>,
    pub intent: Option<String>,
}

/// Por que um consumo não começou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpendError {
    /// outro executor consumiu a concessão entre a avaliação do contrato e a seção crítica
    Taken(String),
    /// o registro de consumo não pôde ser lido ou travado: falha fechado
    Ledger(String),
}

/// A seção crítica do consumo: a trava exclusiva do registro, mantida de `begin` a `commit`.
/// Largar a guarda sem `commit` libera a trava e não consome nada.
#[derive(Debug)]
pub struct SpendGuard {
    file: Option<std::fs::File>,
}

/// As concessões de uma execução e o registro do que já foi consumido.
#[derive(Debug, Default)]
pub struct GrantStore {
    pub grants: Vec<Grant>,
    pub spent: BTreeSet<String>,
    /// concessão → quem a consumiu
    pub owners: std::collections::BTreeMap<String, Owner>,
    /// o registro durável do consumo (`<grants>.spent`), se as concessões vieram de arquivo
    ledger: Option<PathBuf>,
}

/// Lê o registro de consumo. Uma última linha sem `\n` é de um escritor que morreu no meio da
/// escrita, antes do `fsync` e portanto antes de qualquer envio: não consumiu nada. Devolve os
/// consumos e o tamanho da parte íntegra. Uma linha inteira ilegível recusa o registro.
fn read_ledger(bytes: &[u8], name: &str) -> Result<(Vec<(String, Owner)>, usize), String> {
    let mut out = Vec::new();
    let mut good = 0;
    let mut rest = bytes;
    let mut n = 0;
    while let Some(nl) = rest.iter().position(|&b| b == b'\n') {
        let (line, tail) = rest.split_at(nl);
        rest = &tail[1..];
        good += nl + 1;
        n += 1;
        // falha fechado: um registro de consumo ilegível não libera concessão alguma
        let e: Json = std::str::from_utf8(line)
            .ok()
            .and_then(|t| serde_json::from_str(t).ok())
            .ok_or_else(|| format!("consumo `{name}`: linha {n} ilegível"))?;
        let s = |k: &str| e[k].as_str().map(String::from);
        let owner = Owner {
            run: s("run").unwrap_or_default(),
            tool: s("tool").unwrap_or_default(),
            key: s("key"),
            intent: s("intent"),
        };
        // o formato do RC1 (`grant`) e o do RC2 (`grants`: os consumos de um efeito, numa linha)
        let ids: Vec<String> = match (e["grant"].as_str(), e["grants"].as_array()) {
            (Some(id), _) => vec![id.to_string()],
            (None, Some(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            _ => Vec::new(),
        };
        if ids.is_empty() {
            return Err(format!("consumo `{name}`: linha {n} sem `grant`"));
        }
        out.extend(ids.into_iter().map(|id| (id, owner.clone())));
    }
    Ok((out, good))
}

/// Abre e trava o registro de consumo, repara o rabo rasgado e devolve o arquivo travado com os
/// consumos. É o único caminho de leitura do registro: quem lê está sozinho.
fn lock_ledger(path: &std::path::Path) -> Result<(std::fs::File, Vec<(String, Owner)>), String> {
    let name = path.display().to_string();
    let err = |e: std::io::Error| format!("consumo `{name}`: {e}");
    let f = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path).map_err(err)?;
    crate::lock::exclusive(&f, true).map_err(err)?;
    let bytes = std::fs::read(path).map_err(err)?;
    let (spends, good) = read_ledger(&bytes, &name)?;
    if good < bytes.len() {
        f.set_len(good as u64).and_then(|_| f.sync_all()).map_err(err)?;
    }
    Ok((f, spends))
}

/// O consumo registrado em `ledger`, lido sob a trava (para a recuperação, que só tem o WAL).
/// O registro ausente é erro: ele é criado antes de qualquer intenção que cite concessões.
pub fn ledger_owners(ledger: &std::path::Path) -> Result<std::collections::BTreeMap<String, Owner>, String> {
    if !ledger.exists() {
        return Err(format!("consumo `{}`: o registro não existe", ledger.display()));
    }
    Ok(lock_ledger(ledger)?.1.into_iter().collect())
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
            let mut grant = Grant::new(run, op, &args, origin, nonce);
            if let Some(id) = g.get("id").and_then(|v| v.as_str()) {
                if id != grant.id {
                    return Err(format!("concessão {}: o `id` não confere com o conteúdo", i + 1));
                }
            }
            grant.sig = g.get("sig").and_then(|v| v.as_str()).map(String::from);
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
        if ledger.exists() {
            s.absorb(lock_ledger(&ledger)?.1);
        }
        s.ledger = Some(ledger);
        Ok(s)
    }

    /// O caminho do registro de consumo, se as concessões vieram de arquivo.
    pub fn ledger(&self) -> Option<&std::path::Path> {
        self.ledger.as_deref()
    }

    fn absorb(&mut self, spends: Vec<(String, Owner)>) {
        for (id, o) in spends {
            self.spent.insert(id.clone());
            self.owners.entry(id).or_insert(o);
        }
    }

    /// A primeira concessão livre desta execução que casa a tupla e não está em `claimed`.
    /// É uma leitura otimista: quem decide é `begin`, sob a trava.
    pub fn find(&self, run: &str, op: &str, xs: &[String], claimed: &[usize]) -> Option<usize> {
        self.grants.iter().enumerate().position(|(i, g)| {
            g.run == run && !self.spent.contains(&g.id) && !claimed.contains(&i) && g.matches(op, xs)
        })
    }

    /// **Abre a seção crítica do consumo.** Trava o registro, relê o que outros processos
    /// consumiram desde a carga e confere que cada concessão reivindicada continua livre. Com a
    /// guarda na mão, o chamador persiste a intenção no WAL e chama `commit`. Dois executores
    /// com a mesma concessão: um passa, o outro recebe `Taken` sem ter escrito nada.
    pub fn begin(&mut self, idxs: &[usize]) -> Result<SpendGuard, SpendError> {
        if idxs.is_empty() {
            return Ok(SpendGuard { file: None });
        }
        let file = match self.ledger.clone() {
            Some(p) => {
                let (f, spends) = lock_ledger(&p).map_err(SpendError::Ledger)?;
                self.absorb(spends);
                Some(f)
            }
            None => None,
        };
        match idxs.iter().find(|&&i| self.spent.contains(&self.grants[i].id)) {
            Some(&i) => Err(SpendError::Taken(self.grants[i].id.clone())),
            None => Ok(SpendGuard { file }),
        }
    }

    /// **Fecha a seção crítica:** persiste o consumo (uma linha para o efeito inteiro,
    /// sincronizada), citando a intenção do WAL que ele paga, e solta a trava. Se o registro
    /// não pode ser escrito, o processo para: o efeito não sai, e a intenção sem consumo é
    /// fechada pela recuperação como não enviada.
    pub fn commit(&mut self, guard: SpendGuard, idxs: &[usize], owner: Owner) {
        if idxs.is_empty() {
            return;
        }
        let ids: Vec<String> = idxs.iter().map(|&i| self.grants[i].id.clone()).collect();
        if let Some(mut f) = guard.file {
            let line = json!({ "grants": ids, "run": owner.run, "tool": owner.tool, "key": owner.key,
                               "intent": owner.intent })
            .to_string();
            f.write_all(format!("{line}\n").as_bytes())
                .and_then(|_| f.sync_all())
                .unwrap_or_else(|e| panic!("concessões: não foi possível registrar o consumo: {e}"));
        }
        for id in ids {
            self.spent.insert(id.clone());
            self.owners.insert(id, owner.clone());
        }
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
        let spend = |s: &mut GrantStore, i: usize| {
            let gd = s.begin(&[i]).unwrap();
            s.commit(gd, &[i], Owner { run: "r1".into(), tool: "send".into(), ..Owner::default() });
        };
        spend(&mut s, i);
        // outro processo, mesmo arquivo: a consumida não volta
        let mut s2 = GrantStore::open(&p).unwrap();
        let k = s2.find("r1", "send", &xs, &[]).unwrap();
        assert_eq!(k, j);
        spend(&mut s2, k);
        assert!(GrantStore::open(&p).unwrap().find("r1", "send", &xs, &[]).is_none());
        // um registro de consumo ilegível recusa a carga (não libera nada)
        std::fs::write(format!("{}.spent", p.display()), "lixo\n").unwrap();
        assert!(GrantStore::open(&p).is_err());
    }

    fn file(name: &str, grants: Json) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grants-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("g.json");
        std::fs::write(&p, json!({ "grants": grants }).to_string()).unwrap();
        p
    }

    /// Duas cargas do mesmo arquivo veem a concessão livre (a leitura otimista do RC1). Quem
    /// decide é `begin`, que relê sob a trava: a segunda recebe `Taken` e não escreve nada.
    #[test]
    fn a_stale_view_cannot_spend_what_another_store_spent() {
        let p = file("stale", json!([g("send", &["A", "1"], 0)]));
        let (mut a, mut b) = (GrantStore::open(&p).unwrap(), GrantStore::open(&p).unwrap());
        let xs = ["A".to_string(), "1".to_string()];
        let (ia, ib) = (a.find("r1", "send", &xs, &[]).unwrap(), b.find("r1", "send", &xs, &[]).unwrap());
        let ga = a.begin(&[ia]).unwrap();
        a.commit(
            ga,
            &[ia],
            Owner { run: "r1".into(), tool: "send".into(), key: Some("k1".into()), intent: Some("h".into()) },
        );
        assert_eq!(b.begin(&[ib]).unwrap_err(), SpendError::Taken(a.grants[ia].id.clone()));
        let ledger = std::fs::read_to_string(format!("{}.spent", p.display())).unwrap();
        assert_eq!(ledger.lines().count(), 1);
        // o registro diz quem consumiu e qual intenção do WAL o consumo paga
        let o = ledger_owners(std::path::Path::new(&format!("{}.spent", p.display()))).unwrap();
        assert_eq!(o[&a.grants[ia].id].intent.as_deref(), Some("h"));
    }

    /// Largar a guarda sem `commit` não consome; um rabo rasgado (o escritor morreu antes do
    /// `fsync`, portanto antes do envio) não consome e é aparado; o formato do RC1 continua lido.
    #[test]
    fn an_abandoned_or_torn_spend_consumes_nothing() {
        let p = file("torn", json!([g("send", &["A", "1"], 0), g("send", &["B", "2"], 0)]));
        let ledger = format!("{}.spent", p.display());
        let mut s = GrantStore::open(&p).unwrap();
        drop(s.begin(&[0]).unwrap());
        assert!(GrantStore::open(&p).unwrap().spent.is_empty());
        let rc1 = json!({ "grant": s.grants[1].id, "run": "r1", "tool": "send" }).to_string();
        std::fs::write(&ledger, format!("{rc1}\n{{\"grants\":[\"{}", s.grants[0].id)).unwrap();
        let t = GrantStore::open(&p).unwrap();
        assert_eq!(t.spent.iter().collect::<Vec<_>>(), vec![&s.grants[1].id]);
        assert_eq!(std::fs::read_to_string(&ledger).unwrap(), format!("{rc1}\n"));
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
