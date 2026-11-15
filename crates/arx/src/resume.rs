//! **A retomada verificada (v1.0-RC5, `lean/Aletheia/Resume.lean`).**
//!
//! Um programa Aletheia é determinístico **dadas as suas entradas**: as amostras dos modelos,
//! os resultados das ferramentas e a fotografia confiável. Com `--wal`, o ARX grava cada
//! entrada numa **fita** ao lado do WAL (`<wal>.tape`: durável, encadeada, sob trava; o WAL
//! continua byte a byte o da RC4) no instante em que o programa a observa:
//!
//! * `input`: a resposta de um modelo, pela ordem da amostra;
//! * `ret`: o que uma chamada de ferramenta devolveu **ao programa** (valor, erro ou desvio do
//!   tratador), com o relógio e o medidor de orçamento depois dela.
//!
//! `athc resume`, num processo novo, primeiro fecha as intenções em aberto pelo protocolo da
//! classe de cada provedor (a mesma recuperação da RC1 à RC4, sem compensar as sagas) e depois
//! **re-executa o programa desde o início**: cada entrada gravada é devolvida do WAL, sem tocar
//! no mundo, e conferida contra o que o programa pede (ferramenta, argumentos, prompt). Na
//! fronteira da queda, a operação interrompida é decidida pelo desfecho que a recuperação
//! gravou:
//!
//! | desfecho no WAL | a chamada, na retomada |
//! |---|---|
//! | `done` | devolve o valor: o efeito aconteceu uma vez e não é reenviado |
//! | `failed`, provadamente não enviada (`recovery:unspent`, `anchor:unsent`, `anchor:unreserved`) | é executada agora, com uma **chave nova** (a antiga nunca é reaproveitada) |
//! | `failed`, o provedor responde que não aplicou (`recovery:status`), sem concessão | idem |
//! | `failed`, idem, **com** concessão | falha: a autorização foi consumida por aquela tentativa |
//! | `failed`, recusa do provedor | falha, como falharia sem queda |
//! | `escalated` | é o `TimeoutUnknown` da linguagem: o tratador do programa decide |
//!
//! Daí em diante a execução é ao vivo. Uma divergência entre o que o programa pede e o que o
//! WAL registra (`R0650`) envenena a execução: nenhuma chamada sai depois dela.
//!
//! O que a retomada **não** precisa confiar: as entradas gravadas são dados `Untrusted` ou
//! resultados de provedores. As garantias da linguagem (política, contratos, concessões,
//! orçamento) valem para qualquer valor delas; adulterá-las muda o caminho, não a segurança.

use crate::journal::Journal;
use crate::value::Value;
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, HashMap};

/// O passo entre gerações de chave: a chave nova da `i`-ésima cunhagem é `i + g × KEY_GEN`.
pub const KEY_GEN: u64 = 1_000_000;

/// As evidências de que uma operação fechada como `failed` **não foi aplicada**, com certeza.
pub const NOT_APPLIED: [&str; 4] = ["recovery:unspent", "anchor:unsent", "anchor:unreserved", "recovery:status"];

/// O que o WAL de uma execução interrompida diz à retomada.
#[derive(Debug, Default)]
pub struct Resume {
    /// chave → o que a chamada devolveu ao programa
    pub rets: HashMap<String, Json>,
    /// ordem da chamada sem chave → o que ela devolveu
    pub seq_rets: BTreeMap<u64, Json>,
    /// ordem da amostra → a resposta do modelo
    pub inputs: BTreeMap<u64, Json>,
    /// chave → o registro do valor que o provedor devolveu (`value` e, se o monitor de tipo o
    /// recusou, `blame`)
    pub values: HashMap<String, Json>,
    /// chave → a intenção mais recente
    pub intents: HashMap<String, Json>,
    /// chave → (desfecho, detalhe do registro)
    pub outcome: HashMap<String, (String, Json)>,
    /// os registros de saga já gravados (a re-execução os reproduz e não os duplica)
    sagas: HashMap<String, u32>,
    /// (saga, chave) → a compensação já feita deu certo?
    pub compensated: HashMap<(u64, String), bool>,
    /// uma recuperação anterior compensou sagas: a execução foi encerrada, não é retomável
    pub settled_by_recovery: bool,
    pub replayed: u32,
    pub live: u32,
    pub generation: u64,
}

impl Resume {
    /// Do WAL (intenções, desfechos, sagas) e da fita das entradas (`input`, `ret`, `value`).
    pub fn of(log: &Journal, tape: &Journal) -> Resume {
        let mut r = Resume::default();
        for e in log.entries.iter().chain(&tape.entries) {
            let d = &e["detail"];
            let key = d["key"].as_str().map(String::from);
            match e["kind"].as_str() {
                Some("ret") => match (key, d["n"].as_u64()) {
                    (Some(k), _) => {
                        r.rets.insert(k, d.clone());
                    }
                    (None, Some(n)) => {
                        r.seq_rets.insert(n, d.clone());
                    }
                    _ => {}
                },
                Some("value") => {
                    if let Some(k) = key {
                        r.values.insert(k, d.clone());
                    }
                }
                Some("input") => {
                    if let Some(n) = d["n"].as_u64() {
                        r.inputs.insert(n, d.clone());
                    }
                }
                Some("intent") => {
                    if let Some(k) = key {
                        r.intents.insert(k, d.clone());
                    }
                }
                Some("close") => {
                    if let Some(k) = key {
                        r.outcome.insert(k, (d["status"].as_str().unwrap_or_default().to_string(), d.clone()));
                    }
                }
                Some("escalate") => {
                    if let Some(k) = key {
                        r.outcome.insert(k, ("escalated".into(), d.clone()));
                    }
                }
                Some("compensated") => {
                    if d["via"] == "recovery" {
                        r.settled_by_recovery = true;
                    }
                    r.compensated.insert(
                        (d["saga"].as_u64().unwrap_or_default(), key.unwrap_or_default()),
                        d["ok"] == json!(true),
                    );
                }
                Some("resume") => r.generation += 1,
                Some(k) if k.starts_with("saga_") => {
                    if d["outcome"] == "recovered" {
                        r.settled_by_recovery = true;
                    }
                    *r.sagas.entry(format!("{k} {d}")).or_default() += 1;
                }
                _ => {}
            }
        }
        r
    }

    /// A operação de chave `k` foi interrompida e **provadamente não foi aplicada**: a chave
    /// está morta, e a operação é executada de novo com outra.
    pub fn dead(&self, k: u64) -> bool {
        let k = format!("k{k}");
        !self.rets.contains_key(&k)
            && self.intents.contains_key(&k)
            && self.outcome.get(&k).is_some_and(|(s, d)| {
                // `recovery:status` vem depois de um envio possível: a autorização daquela
                // tentativa foi consumida. Só uma operação sem concessão é refeita; com
                // concessão, a chamada falha e uma nova autorização é decisão de quem concede.
                let burned = d["evidence"] == "recovery:status"
                    && self.intents[&k]["grants"].as_array().is_some_and(|g| !g.is_empty());
                s == "failed" && !burned && d["evidence"].as_str().is_some_and(|e| NOT_APPLIED.contains(&e))
            })
    }

    /// A chave da `i`-ésima cunhagem: a original, ou a primeira geração que não está morta.
    pub fn key_for(&self, i: u64) -> u64 {
        let mut k = i;
        let mut g = 0;
        while self.dead(k) {
            g += 1;
            k = i + g * KEY_GEN;
        }
        k
    }

    /// O registro de saga `kind`/`detail` já está no WAL? Se sim, consome uma ocorrência.
    pub fn absorb(&mut self, kind: &str, detail: &Json) -> bool {
        match self.sagas.get_mut(&format!("{kind} {detail}")) {
            Some(n) if *n > 0 => {
                *n -= 1;
                true
            }
            _ => false,
        }
    }
}

/// O BLAKE3 dos argumentos de uma chamada, como o WAL os identifica.
pub fn args_hash(args: &[Value]) -> String {
    let j: Vec<Json> = args.iter().map(Value::to_json).collect();
    blake3::hash(Json::Array(j).to_string().as_bytes()).to_hex().to_string()
}

/// Um valor com o seu construtor: o que `ret` grava e a retomada lê de volta sem conhecer o
/// tipo. Capacidades, rascunhos e obrigações não são resultados de chamadas e não são gravados.
pub fn enc(v: &Value) -> Option<Json> {
    Some(match v {
        Value::Int(n) => json!({ "i": n }),
        Value::Bool(b) => json!({ "b": b }),
        Value::Text(s) => json!({ "t": s }),
        Value::Unit => json!({ "u": 0 }),
        Value::Key(k) => json!({ "k": k }),
        Value::List(xs) => json!({ "l": xs.iter().map(enc).collect::<Option<Vec<_>>>()? }),
        Value::Adt { ty, variant, fields } => {
            let fs = fields.iter().map(|(n, x)| Some(json!([n, enc(x)?]))).collect::<Option<Vec<_>>>()?;
            json!({ "a": [ty, variant, fs] })
        }
        _ => return None,
    })
}

pub fn dec(j: &Json) -> Option<Value> {
    let (tag, x) = j.as_object()?.iter().next()?;
    Some(match tag.as_str() {
        "i" => Value::Int(x.as_i64()?),
        "b" => Value::Bool(x.as_bool()?),
        "t" => Value::Text(x.as_str()?.to_string()),
        "u" => Value::Unit,
        "k" => Value::Key(x.as_u64()?),
        "l" => Value::List(x.as_array()?.iter().map(dec).collect::<Option<Vec<_>>>()?),
        "a" => {
            let a = x.as_array()?;
            let mut fields = Vec::new();
            for f in a.get(2)?.as_array()? {
                fields.push((f.get(0)?.as_str()?.to_string(), dec(f.get(1)?)?));
            }
            Value::Adt { ty: a.first()?.as_str()?.to_string(), variant: a.get(1)?.as_str()?.to_string(), fields }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        let v = Value::List(vec![
            Value::Int(-3),
            Value::Text("a\"b".into()),
            Value::Adt { ty: "P".into(), variant: "Q".into(), fields: vec![("x".into(), Value::Bool(true))] },
            Value::Unit,
            Value::Key(1_000_003),
        ]);
        assert!(dec(&enc(&v).unwrap()) == Some(v));
        assert!(enc(&Value::Cap(Default::default())).is_none());
    }

    #[test]
    fn a_dead_key_is_never_reused() {
        let mut log = Journal::default();
        log.push(0.0, "intent", json!({ "key": "k2", "tool": "pay" }));
        log.push(0.0, "close", json!({ "key": "k2", "status": "failed", "evidence": "anchor:unsent" }));
        log.push(0.0, "intent", json!({ "key": "k1000002", "tool": "pay" }));
        log.push(0.0, "close", json!({ "key": "k1000002", "status": "failed", "evidence": "recovery:unspent" }));
        log.push(0.0, "intent", json!({ "key": "k3", "tool": "pay" }));
        log.push(0.0, "close", json!({ "key": "k3", "status": "failed", "evidence": "reply" }));
        let r = Resume::of(&log, &Journal::default());
        assert_eq!(r.key_for(1), 1);
        assert_eq!(r.key_for(2), 2 + 2 * KEY_GEN);
        // a recusa do provedor não mata a chave: a chamada falha, como falharia sem queda
        assert_eq!(r.key_for(3), 3);
    }
}
