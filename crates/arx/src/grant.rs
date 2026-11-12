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

use crate::anchor::{Anchor, Mark, ZERO};
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
    /// o carimbo da assinatura (v1.0-RC3): para que programa e em que intervalo ela vale
    pub program: Option<String>,
    pub issued: Option<u64>,
    pub expires: Option<u64>,
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
            program: None,
            issued: None,
            expires: None,
        }
    }

    /// O carimbo da assinatura, se a concessão foi assinada com vínculos.
    pub fn stamp(&self) -> Option<crate::sign::Stamp> {
        Some(crate::sign::Stamp {
            program: self.program.clone()?,
            run: self.run.clone(),
            issued: self.issued?,
            expires: self.expires?,
            sig: self.sig.clone()?,
        })
    }

    pub fn to_json(&self) -> Json {
        let mut j = json!({ "id": self.id, "op": self.op, "args": self.args, "origin": self.origin, "run": self.run,
                "nonce": self.nonce });
        if let Some(s) = &self.sig {
            j["sig"] = json!(s);
        }
        if let (Some(p), Some(i), Some(e)) = (&self.program, self.issued, self.expires) {
            j["program"] = json!(p);
            j["issued"] = json!(i);
            j["expires"] = json!(e);
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
    /// o programa (raiz do Codex) que consumiu; vazio nos registros do RC1/RC2
    pub program: String,
    pub tool: String,
    /// a chave da operação (`kN`) e o hash do registro `intent` do WAL que a concessão pagou;
    /// ausentes numa execução sem WAL
    pub key: Option<String>,
    pub intent: Option<String>,
    /// o hash da linha do consumo na cadeia do registro (v1.0-RC3): o desfecho no WAL o cita
    pub line: Option<String>,
}

/// Por que um consumo não começou ou não se completou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpendError {
    /// outro executor consumiu a concessão entre a avaliação do contrato e a seção crítica
    Taken(String),
    /// o registro de consumo ou a âncora recusaram: falha fechado (`L0001`…`L0005`)
    Ledger(String),
}

/// A seção crítica do consumo: a trava exclusiva do registro, mantida de `begin` a `seal`.
/// Largar a guarda sem `commit` libera a trava e não consome nada.
#[derive(Debug)]
pub struct SpendGuard {
    file: Option<std::fs::File>,
    /// o avanço da âncora que `seal` deve fazer: (de, para)
    pending: Option<(Mark, Mark)>,
}

/// As concessões de uma execução e o registro do que já foi consumido.
#[derive(Debug, Default)]
pub struct GrantStore {
    pub grants: Vec<Grant>,
    pub spent: BTreeSet<String>,
    /// concessão → quem a consumiu
    pub owners: std::collections::BTreeMap<String, Owner>,
    /// as concessões que não podem ser usadas: sem a assinatura de uma autoridade declarada
    pub barred: BTreeSet<usize>,
    /// o registro durável do consumo (`<grants>.spent`), se as concessões vieram de arquivo
    ledger: Option<PathBuf>,
    /// a cadeia conferida do registro: o elo depois de cada linha
    heads: Vec<String>,
    /// a âncora monotônica e o escopo (programa, execução) a que o registro pertence
    anchor: Option<(Box<dyn Anchor>, String, String)>,
}

/// O registro de consumo lido e conferido.
#[derive(Debug, Default)]
struct Ledger {
    spends: Vec<(String, Owner)>,
    /// o elo depois de cada linha: o `hash` da linha encadeada, ou o BLAKE3 dos bytes de uma
    /// linha dos formatos anteriores (que assim também ficam presos à cadeia que vem depois)
    heads: Vec<String>,
    /// todas as linhas são encadeadas (exigido com âncora)
    chained: bool,
    /// os (programa, execução) das linhas
    scopes: BTreeSet<(String, String)>,
    good: usize,
}

fn line_hash(body: &Json) -> String {
    blake3::hash(body.to_string().as_bytes()).to_hex().to_string()
}

/// Lê e confere o registro de consumo.
///
/// * Cada linha encadeada traz `seq`, `prev` e `hash`: `L0001` se o número não é a posição, se
///   `prev` não é o elo anterior ou se o `hash` não é o do conteúdo. Alterar, remover ou
///   reordenar uma linha do meio quebra a cadeia.
/// * Uma última linha sem `\n` é de um escritor que morreu no meio da escrita, antes do `fsync`
///   e portanto antes de qualquer envio: não consumiu nada.
/// * O que a cadeia **não** vê é o retrocesso: um prefixo íntegro. Isso é da âncora (`settle`).
fn read_ledger(bytes: &[u8], name: &str) -> Result<Ledger, String> {
    let mut l = Ledger { chained: true, ..Ledger::default() };
    let mut rest = bytes;
    let mut n = 0usize;
    let mut any_chained = false;
    while let Some(nl) = rest.iter().position(|&b| b == b'\n') {
        let (line, tail) = rest.split_at(nl);
        rest = &tail[1..];
        l.good += nl + 1;
        // falha fechado: um registro de consumo ilegível não libera concessão alguma
        let bad = |why: &str| format!("L0001: consumo `{name}`: linha {} {why}", n + 1);
        let mut e: Json =
            std::str::from_utf8(line).ok().and_then(|t| serde_json::from_str(t).ok()).ok_or_else(|| bad("ilegível"))?;
        let prev = l.heads.last().cloned().unwrap_or_else(|| ZERO.to_string());
        match e.as_object_mut().and_then(|o| o.remove("hash")) {
            Some(h) => {
                if e["seq"].as_u64() != Some(n as u64)
                    || e["prev"].as_str() != Some(&prev)
                    || h.as_str() != Some(&line_hash(&e))
                {
                    return Err(bad("adulterada ou fora da cadeia"));
                }
                any_chained = true;
                l.heads.push(h.as_str().unwrap_or_default().to_string());
            }
            None => {
                if any_chained {
                    return Err(bad("sem cadeia depois de linhas encadeadas"));
                }
                l.chained = false;
                l.heads.push(blake3::hash(line).to_hex().to_string());
            }
        }
        let s = |k: &str| e[k].as_str().map(String::from);
        let owner = Owner {
            run: s("run").unwrap_or_default(),
            program: s("program").unwrap_or_default(),
            tool: s("tool").unwrap_or_default(),
            key: s("key"),
            intent: s("intent"),
            line: l.heads.last().cloned().filter(|_| any_chained),
        };
        // o formato do RC1 (`grant`) e o seguinte (`grants`: os consumos de um efeito, numa linha)
        let ids: Vec<String> = match (e["grant"].as_str(), e["grants"].as_array()) {
            (Some(id), _) => vec![id.to_string()],
            (None, Some(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            _ => Vec::new(),
        };
        if ids.is_empty() {
            return Err(bad("sem `grant`"));
        }
        l.scopes.insert((owner.program.clone(), owner.run.clone()));
        l.spends.extend(ids.into_iter().map(|id| (id, owner.clone())));
        n += 1;
    }
    Ok(l)
}

/// Abre e trava o registro de consumo, repara o rabo rasgado e devolve o arquivo travado com o
/// registro conferido. É o único caminho de leitura do registro: quem lê está sozinho.
fn lock_ledger(path: &std::path::Path) -> Result<(std::fs::File, Ledger), String> {
    let name = path.display().to_string();
    let err = |e: std::io::Error| format!("consumo `{name}`: {e}");
    let f = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path).map_err(err)?;
    crate::lock::exclusive(&f, true).map_err(err)?;
    let bytes = std::fs::read(path).map_err(err)?;
    let l = read_ledger(&bytes, &name)?;
    if l.good < bytes.len() {
        f.set_len(l.good as u64).and_then(|_| f.sync_all()).map_err(err)?;
    }
    Ok((f, l))
}

/// **Confronta o registro com a âncora** (sob a trava do registro).
///
/// Com `n` consumos no registro e a âncora em `a`:
///
/// * `a = n`, mesma cabeça: em dia;
/// * `a = n - 1` e a cabeça da âncora é o elo `n - 1` do registro: o último consumo foi gravado
///   e o processo caiu antes de avançar a âncora. Esse consumo ainda não enviou nada (o envio
///   vem depois do avanço); a âncora é adiantada;
/// * `a > n`: **retrocesso** (`L0002`). Há consumos ancorados que o registro não tem;
/// * o resto: **bifurcação** (`L0003`). O registro não passa pela marca da âncora.
///
/// E o registro ancorado é de um só escopo, todo encadeado (`L0004`).
fn settle(anchor: &mut dyn Anchor, program: &str, run: &str, l: &Ledger, name: &str) -> Result<(), String> {
    if !l.chained {
        return Err(format!("L0004: consumo `{name}`: o registro tem linhas sem cadeia; um registro ancorado é encadeado desde a primeira"));
    }
    if let Some((p, r)) = l.scopes.iter().find(|(p, r)| p != program || r != run) {
        return Err(format!(
            "L0004: consumo `{name}`: o registro é de outro escopo (programa #{}, execução `{r}`)",
            &p[..12.min(p.len())]
        ));
    }
    let id = crate::anchor::scope(program, run);
    let a = anchor.read(&id).map_err(|e| format!("L0005: {e}; sem a âncora não se consome nem se recupera"))?;
    let n = l.heads.len() as u64;
    let at = |k: u64| if k == 0 { ZERO.to_string() } else { l.heads[k as usize - 1].clone() };
    if a.seq == n && a.head == at(n) {
        return Ok(());
    }
    if a.seq + 1 == n && a.head == at(a.seq) {
        return anchor
            .advance(&id, &a, &Mark { seq: n, head: at(n) })
            .map_err(|e| format!("L0005: não foi possível adiantar a âncora: {e}"));
    }
    if a.seq > n {
        return Err(format!(
            "L0002: consumo `{name}`: RETROCESSO. O registro tem {n} consumo(s) e a âncora registra {}; \
             há consumos de efeitos já autorizados que sumiram do registro. Nada foi enviado",
            a.seq
        ));
    }
    Err(format!(
        "L0003: consumo `{name}`: BIFURCAÇÃO. O registro ({n} consumo(s)) não passa pela marca da âncora \
         ({} consumo(s), cabeça #{}). Nada foi enviado",
        a.seq,
        &a.head[..12.min(a.head.len())]
    ))
}

/// O consumo registrado em `ledger`, lido sob a trava e, com âncora, confrontado com ela (para
/// a recuperação, que só tem o WAL). O registro ausente é erro: ele é criado antes de qualquer
/// intenção que cite concessões.
pub fn ledger_owners(
    ledger: &std::path::Path,
    anchor: Option<(&mut dyn Anchor, &str, &str)>,
) -> Result<std::collections::BTreeMap<String, Owner>, String> {
    if !ledger.exists() && anchor.is_none() {
        return Err(format!("consumo `{}`: o registro não existe", ledger.display()));
    }
    let (_f, l) = lock_ledger(ledger)?;
    if let Some((a, program, run)) = anchor {
        settle(a, program, run, &l, &ledger.display().to_string())?;
    }
    Ok(l.spends.into_iter().collect())
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
            grant.program = g.get("program").and_then(|v| v.as_str()).map(String::from);
            grant.issued = g.get("issued").and_then(|v| v.as_u64());
            grant.expires = g.get("expires").and_then(|v| v.as_u64());
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
            let l = lock_ledger(&ledger)?.1;
            s.absorb(l);
        }
        s.ledger = Some(ledger);
        Ok(s)
    }

    /// O caminho do registro de consumo, se as concessões vieram de arquivo.
    pub fn ledger(&self) -> Option<&std::path::Path> {
        self.ledger.as_deref()
    }

    pub fn anchored(&self) -> bool {
        self.anchor.is_some()
    }

    /// **Liga o registro à âncora** do escopo (programa, execução) e o confronta já: um registro
    /// retrocedido ou bifurcado é recusado antes de a execução começar.
    pub fn attach_anchor(&mut self, anchor: Box<dyn Anchor>, program: &str, run: &str) -> Result<(), String> {
        self.anchor = Some((anchor, program.to_string(), run.to_string()));
        self.refresh().map(|_| ())
    }

    /// Trava (se há arquivo), relê, confere a cadeia e confronta com a âncora (se há).
    fn refresh(&mut self) -> Result<Option<std::fs::File>, String> {
        let name = self.ledger.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(memória)".into());
        let (file, led) = match self.ledger.clone() {
            Some(p) => {
                let (f, l) = lock_ledger(&p)?;
                (Some(f), l)
            }
            // sem arquivo, o registro é o que este processo tem em memória
            None => (
                None,
                Ledger {
                    heads: self.heads.clone(),
                    chained: true,
                    scopes: self.owners.values().map(|o| (o.program.clone(), o.run.clone())).collect(),
                    ..Ledger::default()
                },
            ),
        };
        if let Some((a, program, run)) = self.anchor.as_mut() {
            settle(a.as_mut(), program, run, &led, &name)?;
        }
        if file.is_some() {
            self.absorb(led);
        }
        Ok(file)
    }

    fn absorb(&mut self, l: Ledger) {
        for (id, o) in l.spends {
            self.spent.insert(id.clone());
            self.owners.entry(id).or_insert(o);
        }
        self.heads = l.heads;
    }

    /// A primeira concessão livre desta execução que casa a tupla e não está em `claimed`.
    /// É uma leitura otimista: quem decide é `begin`, sob a trava.
    pub fn find(&self, run: &str, op: &str, xs: &[String], claimed: &[usize]) -> Option<usize> {
        self.grants.iter().enumerate().position(|(i, g)| {
            g.run == run
                && !self.spent.contains(&g.id)
                && !self.barred.contains(&i)
                && !claimed.contains(&i)
                && g.matches(op, xs)
        })
    }

    /// **Abre a seção crítica do consumo.** Trava o registro, relê o que outros processos
    /// consumiram desde a carga, confere a cadeia, confronta com a âncora e confere que cada
    /// concessão reivindicada continua livre. Com a guarda na mão, o chamador persiste a
    /// intenção no WAL, chama `commit` e depois `seal`. Dois executores com a mesma concessão:
    /// um passa, o outro recebe `Taken` sem ter escrito nada.
    pub fn begin(&mut self, idxs: &[usize]) -> Result<SpendGuard, SpendError> {
        if idxs.is_empty() {
            return Ok(SpendGuard { file: None, pending: None });
        }
        let file = self.refresh().map_err(SpendError::Ledger)?;
        match idxs.iter().find(|&&i| self.spent.contains(&self.grants[i].id)) {
            Some(&i) => Err(SpendError::Taken(self.grants[i].id.clone())),
            None => Ok(SpendGuard { file, pending: None }),
        }
    }

    /// **Persiste o consumo:** uma linha para o efeito inteiro, encadeada na anterior e
    /// sincronizada, citando o programa, a execução e a intenção do WAL que ele paga. Devolve o
    /// hash da linha. A trava continua com a guarda: falta `seal`.
    pub fn commit(&mut self, guard: &mut SpendGuard, idxs: &[usize], mut owner: Owner) -> Option<String> {
        if idxs.is_empty() {
            return None;
        }
        let ids: Vec<String> = idxs.iter().map(|&i| self.grants[i].id.clone()).collect();
        let prev = self.heads.last().cloned().unwrap_or_else(|| ZERO.to_string());
        let seq = self.heads.len() as u64;
        let mut line = json!({ "grants": ids, "run": owner.run, "program": owner.program, "tool": owner.tool,
                               "key": owner.key, "intent": owner.intent, "seq": seq, "prev": prev });
        let hash = line_hash(&line);
        line["hash"] = json!(hash);
        if let Some(f) = guard.file.as_mut() {
            f.write_all(format!("{line}\n").as_bytes())
                .and_then(|_| f.sync_all())
                .unwrap_or_else(|e| panic!("concessões: não foi possível registrar o consumo: {e}"));
        }
        owner.line = Some(hash.clone());
        for id in ids {
            self.spent.insert(id.clone());
            self.owners.insert(id, owner.clone());
        }
        self.heads.push(hash.clone());
        guard.pending = Some((Mark { seq, head: prev }, Mark { seq: seq + 1, head: hash.clone() }));
        Some(hash)
    }

    /// **Fecha a seção crítica:** avança a âncora para a marca do consumo que `commit` gravou e
    /// solta a trava. O efeito só pode ser enviado depois disto. Se a âncora não avança, o
    /// consumo fica gravado e não ancorado, nada é enviado, e a próxima abertura (ou a
    /// recuperação) a adianta.
    pub fn seal(&mut self, guard: SpendGuard) -> Result<(), SpendError> {
        if let (Some((from, to)), Some((a, program, run))) = (&guard.pending, self.anchor.as_mut()) {
            a.advance(&crate::anchor::scope(program, run), from, to)
                .map_err(|e| SpendError::Ledger(format!("L0005: o consumo foi gravado e a âncora não avançou: {e}")))?;
        }
        Ok(())
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
            let mut gd = s.begin(&[i]).unwrap();
            s.commit(&mut gd, &[i], Owner { run: "r1".into(), tool: "send".into(), ..Owner::default() });
            s.seal(gd).unwrap();
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
        let mut ga = a.begin(&[ia]).unwrap();
        let owner = Owner {
            run: "r1".into(),
            tool: "send".into(),
            key: Some("k1".into()),
            intent: Some("h".into()),
            ..Owner::default()
        };
        a.commit(&mut ga, &[ia], owner);
        a.seal(ga).unwrap();
        assert_eq!(b.begin(&[ib]).unwrap_err(), SpendError::Taken(a.grants[ia].id.clone()));
        let ledger = std::fs::read_to_string(format!("{}.spent", p.display())).unwrap();
        assert_eq!(ledger.lines().count(), 1);
        // o registro diz quem consumiu e qual intenção do WAL o consumo paga
        let o = ledger_owners(std::path::Path::new(&format!("{}.spent", p.display())), None).unwrap();
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

    // ---------------------------------------------------------------- v1.0-RC3: cadeia e âncora

    use crate::anchor::{Anchor, AnchorError, Mark, MemAnchor};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Uma âncora compartilhada entre "processos" do teste (cada `GrantStore` é um executor).
    #[derive(Debug, Clone, Default)]
    struct Shared(Rc<RefCell<MemAnchor>>);

    impl Anchor for Shared {
        fn read(&mut self, s: &str) -> Result<Mark, AnchorError> {
            self.0.borrow_mut().read(s)
        }
        fn advance(&mut self, s: &str, from: &Mark, to: &Mark) -> Result<(), AnchorError> {
            self.0.borrow_mut().advance(s, from, to)
        }
    }

    fn anchored(p: &std::path::Path, a: &Shared) -> Result<GrantStore, String> {
        let mut s = GrantStore::open(p)?;
        s.attach_anchor(Box::new(a.clone()), "prog", "r1")?;
        Ok(s)
    }

    fn spend(s: &mut GrantStore, i: usize) -> Result<(), SpendError> {
        let mut gd = s.begin(&[i])?;
        s.commit(
            &mut gd,
            &[i],
            Owner { run: "r1".into(), program: "prog".into(), tool: "send".into(), ..Owner::default() },
        );
        s.seal(gd)
    }

    fn three(name: &str) -> PathBuf {
        file(name, json!([g("send", &["A", "1"], 0), g("send", &["B", "2"], 0), g("send", &["C", "3"], 0)]))
    }

    /// A cadeia vê o meio; a âncora vê o fim. Um prefixo íntegro do registro é uma cadeia
    /// íntegra (o contraexemplo do RC2), e só a âncora o recusa.
    #[test]
    fn the_chain_sees_the_middle_and_the_anchor_sees_the_end() {
        let p = three("chain");
        let ledger = PathBuf::from(format!("{}.spent", p.display()));
        let a = Shared::default();
        let mut s = anchored(&p, &a).unwrap();
        for i in 0..3 {
            spend(&mut s, i).unwrap();
        }
        let real = std::fs::read_to_string(&ledger).unwrap();
        let ls: Vec<&str> = real.lines().collect();
        let put = |v: &[&str]| std::fs::write(&ledger, v.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        // um prefixo: a cadeia aceita (sem âncora, as concessões voltam a estar livres) …
        put(&ls[..1]);
        assert_eq!(GrantStore::open(&p).unwrap().spent.len(), 1, "a cadeia sozinha não vê o retrocesso");
        // … e a âncora recusa
        assert!(anchored(&p, &a).unwrap_err().starts_with("L0002"));
        put(&[]);
        assert!(anchored(&p, &a).unwrap_err().starts_with("L0002"));
        // o meio: removido, trocado, alterado
        put(&[ls[0], ls[2]]);
        assert!(GrantStore::open(&p).unwrap_err().starts_with("L0001"));
        put(&[ls[1], ls[0], ls[2]]);
        assert!(GrantStore::open(&p).unwrap_err().starts_with("L0001"));
        put(&[ls[0], &ls[1].replace("\"tool\":\"send\"", "\"tool\":\"sent\""), ls[2]]);
        assert!(GrantStore::open(&p).unwrap_err().starts_with("L0001"));
        // outro escopo: o mesmo registro não serve a outro programa nem a outra execução
        put(&ls);
        let mut other = GrantStore::open(&p).unwrap();
        assert!(other.attach_anchor(Box::new(a.clone()), "outro", "r1").unwrap_err().starts_with("L0004"));
        // íntegro e na marca: aceito, e tudo continua gasto
        assert_eq!(anchored(&p, &a).unwrap().spent.len(), 3);
    }

    /// Uma bifurcação bem encadeada (a última linha reescrita, com `prev` e `hash` certos) passa
    /// pela cadeia e para na âncora. E o registro um consumo à frente da âncora (queda entre a
    /// gravação e o avanço) é adiantado, uma vez.
    #[test]
    fn a_fork_stops_at_the_anchor_and_one_unanchored_spend_rolls_forward() {
        let p = three("fork");
        let ledger = PathBuf::from(format!("{}.spent", p.display()));
        let a = Shared::default();
        let mut s = anchored(&p, &a).unwrap();
        spend(&mut s, 0).unwrap();
        // a queda entre a gravação e o avanço: `commit` sem `seal`
        let mut gd = s.begin(&[1]).unwrap();
        s.commit(
            &mut gd,
            &[1],
            Owner { run: "r1".into(), program: "prog".into(), tool: "send".into(), ..Owner::default() },
        );
        drop(gd);
        drop(s);
        assert_eq!(a.0.borrow().marks.values().next().unwrap().seq, 1);
        let mut s = anchored(&p, &a).unwrap();
        assert_eq!((s.spent.len(), a.0.borrow().marks.values().next().unwrap().seq), (2, 2), "adiantada");
        spend(&mut s, 2).unwrap();
        // a bifurcação: a última linha reescrita para outra concessão
        let real = std::fs::read_to_string(&ledger).unwrap();
        let mut ls: Vec<String> = real.lines().map(String::from).collect();
        let mut e: Json = serde_json::from_str(&ls[2]).unwrap();
        e.as_object_mut().unwrap().remove("hash");
        e["grants"] = json!(["f".repeat(64)]);
        e["hash"] = json!(line_hash(&e));
        ls[2] = e.to_string();
        std::fs::write(&ledger, ls.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        assert!(GrantStore::open(&p).is_ok(), "a cadeia aceita a bifurcação");
        assert!(anchored(&p, &a).unwrap_err().starts_with("L0003"));
        // duas linhas à frente da âncora não são uma queda: recusado
        std::fs::write(&ledger, &real).unwrap();
        a.0.borrow_mut().marks.values_mut().for_each(|m| *m = Mark::zero());
        assert!(anchored(&p, &a).unwrap_err().starts_with("L0003"));
    }

    /// Dois executores com **cópias** do mesmo registro (duas máquinas, um backup): a trava do
    /// arquivo não os separa, a âncora sim. O primeiro avança; o segundo, com a cópia velha, é
    /// recusado sem consumir. E com a âncora fora do ar ninguém consome.
    #[test]
    fn the_anchor_arbitrates_between_copies_of_the_ledger() {
        let (p1, p2) = (three("copy-a"), three("copy-b"));
        let a = Shared::default();
        let (mut s1, mut s2) = (anchored(&p1, &a).unwrap(), anchored(&p2, &a).unwrap());
        spend(&mut s1, 0).unwrap();
        match spend(&mut s2, 0) {
            Err(SpendError::Ledger(m)) => assert!(m.starts_with("L0002"), "{m}"),
            x => panic!("a cópia velha consumiu: {x:?}"),
        }
        assert!(s2.spent.is_empty());
        a.0.borrow_mut().down = 1;
        match spend(&mut s1, 1) {
            Err(SpendError::Ledger(m)) => assert!(m.starts_with("L0005"), "{m}"),
            x => panic!("consumiu sem âncora: {x:?}"),
        }
        assert_eq!(s1.spent.len(), 1);
        spend(&mut s1, 1).unwrap();
    }
}
