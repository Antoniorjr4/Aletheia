//! Write-ahead log das operações remotas com chave (Rev13, M-III).
//!
//! Antes de enviar uma operação com chave ao provedor, o ARX persiste a **intenção**: a chave
//! (a identidade da operação), a ferramenta, os argumentos, a classe do provedor e a obrigação
//! de reconciliação. Depois persiste o **desfecho**, sempre com a evidência que o sustenta:
//! `close done` / `close failed` (uma resposta do provedor) ou `escalate` (sem resposta
//! conclusiva: a obrigação foi transferida). As sagas abertas também ficam no log (início,
//! operações registradas, compensações, fim), para que uma queda no meio de uma saga não deixe
//! efeitos sem estorno.
//!
//! Cada registro é uma linha JSON encadeada por BLAKE3 (como o journal) e sincronizada no disco
//! antes do passo seguinte. Na leitura, uma última linha rasgada (queda no meio da escrita) é
//! descartada: um registro está inteiro ou ausente. Como a intenção é persistida **antes** do
//! envio, uma intenção ausente significa uma operação que não saiu do processo.
//!
//! A recuperação (`Arx::recover`) é o modelo de `lean/Aletheia/Wal.lean`: para cada intenção em
//! aberto, reconcilia pelo protocolo da classe do provedor (`idempotent` reenvia com a mesma
//! chave, `queryable` consulta, `opaque` escala) e persiste o desfecho; nunca fecha sem
//! resposta, nunca descarta. Depois compensa as sagas interrompidas, sem estornar as operações
//! que a reconciliação refutou.

use crate::journal::{entry_hash, Journal, GENESIS};
use serde_json::Value as Json;
use std::io::Write;
use std::path::PathBuf;

/// Onde o processo cai, na injeção de falhas dos testes: depois de persistir a intenção, depois
/// do envio (a resposta, se houve, se perde com o processo), depois de conhecer o desfecho e
/// antes de persisti-lo, ou depois de persisti-lo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashPoint {
    Intent,
    Sent,
    Confirmed,
    Closed,
}

impl CrashPoint {
    pub fn parse(s: &str) -> Option<CrashPoint> {
        Some(match s {
            "intent" => CrashPoint::Intent,
            "sent" => CrashPoint::Sent,
            "confirmed" => CrashPoint::Confirmed,
            "closed" => CrashPoint::Closed,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            CrashPoint::Intent => "intent",
            CrashPoint::Sent => "sent",
            CrashPoint::Confirmed => "confirmed",
            CrashPoint::Closed => "closed",
        }
    }
}

/// A mensagem do pânico de uma queda simulada.
pub const CRASH: &str = "aletheia: queda simulada do processo";

#[derive(Debug, Default)]
pub struct Wal {
    pub log: Journal,
    path: Option<PathBuf>,
    /// a queda injetada: no ponto `.0` da `.1`-ésima operação com chave (a partir de 1)
    pub crash: Option<(CrashPoint, u32)>,
    pub nth: u32,
    /// a ordem de cada chave (a `n`-ésima operação com chave)
    pub ord: std::collections::HashMap<u64, u32>,
}

impl Wal {
    /// Um log em arquivo: os registros já existentes (de uma execução anterior) são carregados.
    pub fn open(path: PathBuf) -> std::io::Result<Wal> {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let log = Wal::parse(&text);
        Ok(Wal { log, path: Some(path), ..Wal::default() })
    }

    /// Um log só em memória (testes).
    pub fn memory() -> Wal {
        Wal::default()
    }

    /// O log que sobrou de uma execução anterior (o reinício depois da queda).
    pub fn resume(log: Journal) -> Wal {
        Wal { log, ..Wal::default() }
    }

    /// Lê um log: a cadeia de hashes é conferida e o sufixo inválido (uma linha rasgada) é
    /// descartado.
    pub fn parse(text: &str) -> Journal {
        let mut ok: Vec<Json> = Vec::new();
        let mut prev = GENESIS.to_string();
        for line in text.lines() {
            let Ok(e) = serde_json::from_str::<Json>(line) else { break };
            let mut body = e.clone();
            let Some(obj) = body.as_object_mut() else { break };
            let Some(h) = obj.remove("hash") else { break };
            if obj.get("prev").and_then(|p| p.as_str()) != Some(prev.as_str())
                || h.as_str() != Some(entry_hash(&body).as_str())
            {
                break;
            }
            prev = h.as_str().unwrap_or_default().to_string();
            ok.push(e);
        }
        Journal::from_entries(ok)
    }

    /// Acrescenta um registro e o sincroniza no disco antes de voltar.
    pub fn append(&mut self, t: f64, kind: &str, detail: Json) {
        self.log.push(t, kind, detail);
        if let Some(p) = &self.path {
            let line = self.log.entries.last().map(|e| e.to_string()).unwrap_or_default();
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).expect("wal: abrir");
            writeln!(f, "{line}").expect("wal: escrever");
            f.sync_all().expect("wal: sincronizar");
        }
    }

    /// A queda injetada, se for este o ponto da operação de chave `key`.
    pub fn maybe_crash_key(&self, at: CrashPoint, key: u64) {
        if let (Some((p, n)), Some(&o)) = (self.crash, self.ord.get(&key)) {
            if p == at && n == o {
                panic!("{CRASH} ({} da operação {n})", at.name());
            }
        }
    }

    pub fn to_jsonl(&self) -> String {
        self.log.to_jsonl()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Uma última linha rasgada (queda no meio da escrita) é descartada; uma linha adulterada
    /// no meio corta o log ali. Um registro está inteiro ou ausente.
    #[test]
    fn torn_tail_is_discarded() {
        let mut w = Wal::memory();
        w.append(0.0, "intent", json!({ "key": "k1", "tool": "pay" }));
        w.append(1.0, "close", json!({ "key": "k1", "status": "done" }));
        w.append(2.0, "intent", json!({ "key": "k2", "tool": "pay" }));
        let text = w.to_jsonl();
        let torn = &text[..text.len() - 17];
        let back = Wal::parse(torn);
        assert_eq!(back.entries.len(), 2);
        assert_eq!(Journal::verify(&back.entries), Ok(()));
        let tampered = text.replacen("\"done\"", "\"failed\"", 1);
        assert_eq!(Wal::parse(&tampered).entries.len(), 1);
        assert_eq!(Wal::parse(&text).entries.len(), 3);
    }

    #[test]
    fn file_wal_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("wal-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("w.jsonl");
        let _ = std::fs::remove_file(&p);
        let mut w = Wal::open(p.clone()).unwrap();
        w.append(0.0, "intent", json!({ "key": "k1" }));
        drop(w);
        let mut w = Wal::open(p.clone()).unwrap();
        assert_eq!(w.log.entries.len(), 1);
        w.append(1.0, "escalate", json!({ "key": "k1" }));
        let st = WalState::of(&Wal::open(p).unwrap().log);
        assert_eq!(st.outcome.get("k1").map(String::as_str), Some("escalated"));
        assert!(st.open().is_empty());
    }
}

/// O estado de um log: as intenções, os desfechos e as sagas.
#[derive(Debug, Default)]
pub struct WalState {
    /// (chave, detalhe da intenção), em ordem
    pub intents: Vec<(String, Json)>,
    /// chave → desfecho (`done`, `failed`, `escalated`)
    pub outcome: std::collections::BTreeMap<String, String>,
    /// saga → (terminou, operações registradas em ordem, chaves tiradas do registro, compensações feitas)
    pub sagas: std::collections::BTreeMap<u64, SagaRec>,
}

#[derive(Debug, Default, Clone)]
pub struct SagaRec {
    pub ended: bool,
    pub ops: Vec<Json>,
    pub unlogged: Vec<String>,
    pub compensated: usize,
}

impl WalState {
    pub fn of(log: &Journal) -> WalState {
        let mut s = WalState::default();
        for e in &log.entries {
            let d = &e["detail"];
            let key = || d["key"].as_str().unwrap_or_default().to_string();
            let saga = || d["saga"].as_u64().unwrap_or_default();
            match e["kind"].as_str() {
                Some("intent") => {
                    s.intents.push((key(), d.clone()));
                    // uma operação com chave dentro de saga pertence a ela desde a intenção: a
                    // queda pode vir antes de o quadro registrá-la (`saga_op`)
                    if let Some(sid) = d["saga"].as_u64() {
                        s.sagas.entry(sid).or_default().ops.push(d.clone());
                    }
                }
                Some("close") => {
                    s.outcome.insert(key(), d["status"].as_str().unwrap_or_default().to_string());
                }
                Some("escalate") => {
                    s.outcome.insert(key(), "escalated".into());
                }
                Some("saga_begin") => {
                    s.sagas.entry(saga()).or_default();
                }
                Some("saga_op") => {
                    let sg = s.sagas.entry(saga()).or_default();
                    if d["key"].is_null() || !sg.ops.iter().any(|o| o["key"] == d["key"]) {
                        sg.ops.push(d.clone());
                    }
                }
                Some("saga_unlog") => s.sagas.entry(saga()).or_default().unlogged.push(key()),
                Some("compensated") => s.sagas.entry(saga()).or_default().compensated += 1,
                Some("saga_end") => s.sagas.entry(saga()).or_default().ended = true,
                _ => {}
            }
        }
        s
    }

    /// As intenções sem desfecho.
    pub fn open(&self) -> Vec<&(String, Json)> {
        self.intents.iter().filter(|(k, _)| !self.outcome.contains_key(k)).collect()
    }
}
