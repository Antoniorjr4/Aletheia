//! **O ciclo de vida verificado do efeito** (v1.0-RC4, `lean/Aletheia/Lifecycle.lean`).
//!
//! Na RC3 a âncora guardava só a marca do registro de consumo, e o relógio do executor decidia
//! o prazo. Aqui a âncora passa a ser o **registro unificado** do ciclo de vida das operações
//! autorizadas por concessão, e cada registro é aceito por regras que conferem evidência, não
//! só a forma:
//!
//! ```text
//! Grant ─▶ Intent ─▶ reserve ─▶ send ─▶ (efeito) ─▶ outcome
//!   (assinada)  (WAL)   (âncora)  (âncora)            (âncora, recibo assinado)
//!                         └──────▶ release            (provadamente não enviada)
//!                                   send ─▶ withdraw  (o executor desiste do reenvio)
//! ```
//!
//! * `reserve` consome as concessões (linear: uma reserva não liberada por concessão), com o
//!   tempo autenticado que mostra cada uma válida;
//! * `send` é a autorização de envio: o executor só envia o efeito depois dela, com um tempo
//!   autenticado obtido **imediatamente antes** e que ainda mostra as concessões válidas;
//! * `outcome` só é aceito com o **recibo assinado pelo provedor** (`outcomes`), ligado à chave
//!   de fio, à ferramenta e aos argumentos da operação reservada;
//! * `release` libera as concessões de uma reserva **sem** `send`: como o envio só vem depois do
//!   `send` ancorado, a ausência dele no registro confiável prova que nada saiu. (A ausência de
//!   um registro **local** não prova nada e nunca é consultada.)
//!
//! **O tempo** vem de uma testemunha declarada no programa (`witness`, usada como `clock`), não
//! do relógio do executor. Cada leitura é um pedido com um *nonce* derivado do escopo, da
//! cabeça do registro e de um sal aleatório; a resposta é assinada sobre o nonce e o instante.
//! O executor confere a assinatura, o eco do nonce (repetição: `T0001`), o atraso de ida e volta
//! contra a tolerância declarada (`T0003`), o piso monotônico do registro ancorado
//! (retrocesso: `T0002`) e a coerência com o relógio **monotônico** local entre duas leituras
//! (congelamento: `T0004`). O relógio de parede local não entra em decisão nenhuma.
//!
//! A testemunha arredonda o instante para cima (`t ≥` o instante da assinatura). Logo, no
//! momento da conferência, o instante real está em `[t − 1, t + tolerância]`; uma concessão é
//! válida se `issued ≤ t − 1` e `t + tolerância ≤ expires`. É a definição `valid` de
//! `Lifecycle.lean`.

use crate::grant::{Grant, GrantStore};
use crate::sign;
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn h(s: &str) -> String {
    blake3::hash(s.as_bytes()).to_hex().to_string()
}

// ---------------------------------------------------------------- tempo autenticado

/// Uma resposta assinada da testemunha de tempo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attestation {
    pub t: u64,
    pub nonce: String,
    pub salt: String,
    pub sig: String,
}

impl Attestation {
    pub fn to_json(&self) -> Json {
        json!({ "t": self.t, "nonce": self.nonce, "salt": self.salt, "sig": self.sig })
    }

    pub fn from_json(j: &Json) -> Option<Attestation> {
        Some(Attestation {
            t: j["t"].as_u64()?,
            nonce: j["nonce"].as_str()?.to_string(),
            salt: j["salt"].as_str()?.to_string(),
            sig: j["sig"].as_str()?.to_string(),
        })
    }

    /// O intervalo em que o instante real está, no momento em que a resposta chega.
    pub fn window(&self, tolerance: u64) -> (u64, u64) {
        (self.t.saturating_sub(1), self.t + tolerance)
    }
}

/// A carga que a testemunha assina (tipo `time`).
pub fn time_payload(nonce: &str, t: u64) -> String {
    format!("{nonce}\n{t}")
}

/// O nonce de uma leitura de tempo: preso ao escopo e à posição do registro em que ela será
/// usada, e a um sal novo. Uma resposta só serve ao pedido para o qual foi emitida.
pub fn time_nonce(scope: &str, head: &str, salt: &str) -> String {
    h(&format!("aletheia/v1/time-nonce\n{scope}\n{head}\n{salt}"))
}

#[derive(Debug, Clone, PartialEq)]
pub enum TimeError {
    /// assinatura que não confere, ou resposta a outro pedido (repetição)
    Invalid(String),
    /// o instante atestado é anterior ao piso do registro ancorado
    Rewound { got: u64, floor: u64 },
    /// a resposta demorou mais que a tolerância declarada
    Late { rtt: f64, tolerance: u64 },
    /// o instante atestado andou menos que o relógio monotônico local entre duas leituras
    Frozen { t1: u64, t2: u64, elapsed: f64 },
    /// a testemunha não respondeu
    Unavailable(String),
}

impl TimeError {
    pub fn code(&self) -> &'static str {
        match self {
            TimeError::Invalid(_) => "T0001",
            TimeError::Rewound { .. } => "T0002",
            TimeError::Late { .. } => "T0003",
            TimeError::Frozen { .. } => "T0004",
            TimeError::Unavailable(_) => "T0005",
        }
    }
}

impl std::fmt::Display for TimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let c = self.code();
        match self {
            TimeError::Invalid(m) => write!(f, "{c}: tempo não autenticado: {m}"),
            TimeError::Rewound { got, floor } => write!(
                f,
                "{c}: RETROCESSO TEMPORAL. A testemunha atesta {got}, abaixo do piso {floor} (o maior instante já ancorado ou já aceito)"
            ),
            TimeError::Late { rtt, tolerance } => {
                write!(f, "{c}: a resposta de tempo levou {rtt:.2} s, acima da tolerância declarada de {tolerance} s")
            }
            TimeError::Frozen { t1, t2, elapsed } => write!(
                f,
                "{c}: CONGELAMENTO. A testemunha foi de {t1} a {t2} enquanto o relógio monotônico local mediu {elapsed:.1} s"
            ),
            TimeError::Unavailable(m) => {
                write!(f, "{c}: a testemunha de tempo não respondeu ({m}); sem tempo autenticado não há efeito")
            }
        }
    }
}

/// A fonte de respostas de tempo (a testemunha, vista pelo executor).
pub trait TimeSource: std::fmt::Debug {
    fn ask(&mut self, nonce: &str) -> Result<Json, String>;
}

fn line_call(addr: &SocketAddr, timeout: Duration, req: &Json) -> Result<Json, String> {
    let e = |x: std::io::Error| x.to_string();
    let mut s = TcpStream::connect_timeout(addr, timeout).map_err(e)?;
    s.set_read_timeout(Some(timeout)).map_err(e)?;
    s.set_write_timeout(Some(timeout)).map_err(e)?;
    s.write_all(format!("{req}\n").as_bytes()).map_err(e)?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).map_err(e)?;
    serde_json::from_str(&line).map_err(|x| format!("resposta ilegível: {x}"))
}

fn resolve(addr: &str) -> Result<SocketAddr, String> {
    let a = addr.strip_prefix("tcp://").unwrap_or(addr);
    a.to_socket_addrs().map_err(|e| format!("`{a}`: {e}"))?.next().ok_or_else(|| format!("`{a}`: endereço sem destino"))
}

/// A testemunha num serviço (`ath-world serve --sign-key`, operação `time`).
#[derive(Debug)]
pub struct TcpTime {
    addr: SocketAddr,
    timeout: Duration,
}

impl TcpTime {
    pub fn new(addr: &str, timeout: f64) -> Result<TcpTime, String> {
        Ok(TcpTime { addr: resolve(addr)?, timeout: Duration::from_secs_f64(timeout.max(0.05)) })
    }
}

impl TimeSource for TcpTime {
    fn ask(&mut self, nonce: &str) -> Result<Json, String> {
        let j = line_call(&self.addr, self.timeout, &json!({ "op": "time", "nonce": nonce }))?;
        if j["ok"] != true {
            return Err(j["error"].as_str().unwrap_or("resposta sem `ok`").to_string());
        }
        Ok(j)
    }
}

/// Uma testemunha em memória, que assina com `secret` o instante `now` (para testes).
#[derive(Debug, Clone)]
pub struct MemTime {
    pub secret: [u8; 32],
    pub now: u64,
}

impl TimeSource for MemTime {
    fn ask(&mut self, nonce: &str) -> Result<Json, String> {
        let sig = sign::sign(&self.secret, "time", &time_payload(nonce, self.now));
        Ok(json!({ "ok": true, "t": self.now, "nonce": nonce, "sig": sig }))
    }
}

fn salt() -> Result<String, String> {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| format!("sem fonte de aleatoriedade: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// **O verificador de tempo do executor.** Só aceita um instante assinado pela testemunha
/// declarada, para o pedido que acabou de fazer, dentro da tolerância de atraso, nunca abaixo do
/// piso ancorado e coerente com o relógio monotônico local entre leituras.
#[derive(Debug)]
pub struct TimeKeeper {
    src: Box<dyn TimeSource>,
    key: [u8; 32],
    pub tolerance: u64,
    /// a última leitura aceita e o instante monotônico em que chegou
    last: Option<(u64, Instant)>,
    /// o piso: o maior instante já ancorado (ou já aceito)
    pub floor: u64,
}

impl TimeKeeper {
    pub fn new(src: Box<dyn TimeSource>, key: [u8; 32], tolerance: u64) -> TimeKeeper {
        TimeKeeper { src, key, tolerance, last: None, floor: 0 }
    }

    pub fn attest(&mut self, scope: &str, head: &str) -> Result<Attestation, TimeError> {
        let salt = salt().map_err(TimeError::Unavailable)?;
        let nonce = time_nonce(scope, head, &salt);
        let start = Instant::now();
        let raw = self.src.ask(&nonce).map_err(TimeError::Unavailable)?;
        let recv = Instant::now();
        let rtt = recv.duration_since(start).as_secs_f64();
        let (Some(t), Some(echo), Some(sig)) = (raw["t"].as_u64(), raw["nonce"].as_str(), raw["sig"].as_str()) else {
            return Err(TimeError::Invalid("resposta incompleta".into()));
        };
        if echo != nonce {
            return Err(TimeError::Invalid("a resposta é de outro pedido (repetição de uma resposta antiga)".into()));
        }
        if !sign::verify(&self.key, "time", &time_payload(&nonce, t), sig) {
            return Err(TimeError::Invalid("a assinatura não é da testemunha declarada".into()));
        }
        if rtt > self.tolerance as f64 {
            return Err(TimeError::Late { rtt, tolerance: self.tolerance });
        }
        if t < self.floor {
            return Err(TimeError::Rewound { got: t, floor: self.floor });
        }
        if let Some((t1, i1)) = self.last {
            let elapsed = recv.duration_since(i1).as_secs_f64();
            // o instante é arredondado para cima: um segundo de folga além da tolerância
            if (t as f64) + (self.tolerance as f64) + 1.0 < (t1 as f64) + elapsed {
                return Err(TimeError::Frozen { t1, t2: t, elapsed });
            }
        }
        self.last = Some((t, recv));
        self.floor = self.floor.max(t);
        Ok(Attestation { t, nonce, salt, sig: sig.to_string() })
    }

    /// O intervalo do instante real agora, a partir da última leitura aceita e do relógio
    /// monotônico desde então.
    pub fn bounds(&self) -> Option<(u64, u64)> {
        self.last.map(|(t, i)| {
            let el = i.elapsed().as_secs_f64();
            (t.saturating_sub(1) + el.floor() as u64, t + self.tolerance + el.ceil() as u64)
        })
    }
}

// ---------------------------------------------------------------- evidência de desfecho

/// Os argumentos de uma operação como vão ao provedor, em forma canônica.
pub fn args_hash(plain: &[Json]) -> String {
    h(&Json::Array(plain.to_vec()).to_string())
}

/// A carga de um recibo (tipo `receipt`): a chave de fio, a ferramenta, os argumentos (`-`
/// numa consulta de status, que não os reenvia), o estado e o valor.
pub fn receipt_payload(wire: &str, tool: &str, args: &str, status: &str, value: &str) -> String {
    format!("{wire}\n{tool}\n{args}\n{status}\n{value}")
}

/// O recibo do provedor: a evidência de um desfecho.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub wire: String,
    pub tool: String,
    pub args: String,
    /// `done`, `refused` ou `not_applied`
    pub status: String,
    pub value: String,
    pub sig: String,
}

impl Receipt {
    pub fn from_json(j: &Json) -> Option<Receipt> {
        let s = |k: &str| j[k].as_str().map(String::from);
        Some(Receipt {
            wire: s("wire")?,
            tool: s("tool")?,
            args: s("args")?,
            status: s("status")?,
            value: s("value")?,
            sig: s("sig")?,
        })
    }

    pub fn to_json(&self) -> Json {
        json!({ "wire": self.wire, "tool": self.tool, "args": self.args, "status": self.status,
                "value": self.value, "sig": self.sig })
    }

    pub fn sign(secret: &[u8; 32], wire: &str, tool: &str, args: &str, status: &str, value: &str) -> Receipt {
        let sig = sign::sign(secret, "receipt", &receipt_payload(wire, tool, args, status, value));
        Receipt {
            wire: wire.into(),
            tool: tool.into(),
            args: args.into(),
            status: status.into(),
            value: value.into(),
            sig,
        }
    }

    pub fn verify(&self, key: &[u8; 32]) -> bool {
        sign::verify(
            key,
            "receipt",
            &receipt_payload(&self.wire, &self.tool, &self.args, &self.status, &self.value),
            &self.sig,
        )
    }
}

// ---------------------------------------------------------------- o registro unificado

/// Uma autoridade temporal, como o escopo a conhece.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetaAuthority {
    pub name: String,
    pub key: String,
    pub ops: Vec<String>,
    pub max_age: u64,
}

/// A configuração de confiança de um escopo: o programa, a execução, as autoridades e as
/// testemunhas. O identificador do escopo é o hash dela: o escopo é auto-certificado (quem
/// abrisse o escopo com outras chaves abriria outro escopo, que nenhum executor deste programa
/// usa).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    pub program: String,
    pub run: String,
    pub authorities: Vec<MetaAuthority>,
    pub clock: String,
    pub tolerance: u64,
    pub outcomes: String,
}

impl Meta {
    pub fn to_json(&self) -> Json {
        let auths: Vec<Json> = self
            .authorities
            .iter()
            .map(|a| json!({ "name": a.name, "key": a.key, "ops": a.ops, "max_age": a.max_age }))
            .collect();
        json!({ "program": self.program, "run": self.run, "authorities": auths, "clock": self.clock,
                "tolerance": self.tolerance, "outcomes": self.outcomes })
    }

    pub fn from_json(j: &Json) -> Option<Meta> {
        let s = |v: &Json| v.as_str().map(String::from);
        let mut authorities = Vec::new();
        for a in j["authorities"].as_array()? {
            authorities.push(MetaAuthority {
                name: s(&a["name"])?,
                key: s(&a["key"])?,
                ops: a["ops"].as_array()?.iter().filter_map(|o| o.as_str().map(String::from)).collect(),
                max_age: a["max_age"].as_u64()?,
            });
        }
        Some(Meta {
            program: s(&j["program"])?,
            run: s(&j["run"])?,
            authorities,
            clock: s(&j["clock"])?,
            tolerance: j["tolerance"].as_u64()?,
            outcomes: s(&j["outcomes"])?,
        })
    }

    pub fn scope(&self) -> String {
        h(&format!("aletheia/v1/lifecycle\n{}", self.to_json()))
    }
}

/// Um registro do ciclo de vida, encadeado.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub seq: u64,
    pub prev: String,
    pub kind: String,
    pub body: Json,
    pub hash: String,
}

pub fn entry_hash(seq: u64, prev: &str, kind: &str, body: &Json) -> String {
    h(&json!({ "seq": seq, "prev": prev, "kind": kind, "body": body }).to_string())
}

impl Entry {
    pub fn new(chain: &[Entry], kind: &str, body: Json) -> Entry {
        let seq = chain.len() as u64;
        let prev = chain.last().map(|e| e.hash.clone()).unwrap_or_else(|| ZERO.to_string());
        let hash = entry_hash(seq, &prev, kind, &body);
        Entry { seq, prev, kind: kind.into(), body, hash }
    }

    pub fn to_json(&self) -> Json {
        json!({ "seq": self.seq, "prev": self.prev, "kind": self.kind, "body": self.body, "hash": self.hash })
    }

    pub fn from_json(j: &Json) -> Option<Entry> {
        Some(Entry {
            seq: j["seq"].as_u64()?,
            prev: j["prev"].as_str()?.to_string(),
            kind: j["kind"].as_str()?.to_string(),
            body: j["body"].clone(),
            hash: j["hash"].as_str()?.to_string(),
        })
    }
}

/// O estado de uma operação no registro ancorado.
#[derive(Clone, Debug, Default)]
pub struct Op {
    pub key: String,
    pub tool: String,
    pub args: Vec<Json>,
    pub args_hash: String,
    pub grants: Vec<Grant>,
    pub intent: String,
    pub reserve: String,
    pub send: Option<String>,
    pub outcome: Option<(String, Receipt)>,
    pub release: Option<String>,
    pub withdraw: Option<String>,
}

/// O estado de um escopo, reconstruído do registro ancorado.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub ops: BTreeMap<String, Op>,
    /// as chaves, na ordem das reservas
    pub order: Vec<String>,
    /// concessão → a chave que a reservou (reservas não liberadas)
    pub taken: BTreeMap<String, String>,
    /// o piso temporal: o maior instante atestado no registro
    pub floor: u64,
    pub head: String,
    pub len: u64,
}

impl State {
    /// Aplica um registro já aceito.
    fn apply(&mut self, e: &Entry) {
        let key = e.body["key"].as_str().unwrap_or_default().to_string();
        if let Some(t) = e.body["time"]["t"].as_u64() {
            self.floor = self.floor.max(t);
        }
        match e.kind.as_str() {
            "reserve" => {
                let grants =
                    GrantStore::from_json(&json!({ "grants": e.body["grants"] })).map(|s| s.grants).unwrap_or_default();
                for g in &grants {
                    self.taken.insert(g.id.clone(), key.clone());
                }
                self.ops.insert(
                    key.clone(),
                    Op {
                        key: key.clone(),
                        tool: e.body["tool"].as_str().unwrap_or_default().into(),
                        args: e.body["args"].as_array().cloned().unwrap_or_default(),
                        args_hash: e.body["args_hash"].as_str().unwrap_or_default().into(),
                        grants,
                        intent: e.body["intent"].as_str().unwrap_or_default().into(),
                        reserve: e.hash.clone(),
                        ..Op::default()
                    },
                );
                self.order.push(key);
            }
            kind => {
                if let Some(op) = self.ops.get_mut(&key) {
                    match kind {
                        "send" => op.send = Some(e.hash.clone()),
                        "outcome" => op.outcome = Receipt::from_json(&e.body["receipt"]).map(|r| (e.hash.clone(), r)),
                        "withdraw" => op.withdraw = Some(e.hash.clone()),
                        "release" => {
                            op.release = Some(e.hash.clone());
                            for g in &op.grants {
                                self.taken.remove(&g.id);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        self.head = e.hash.clone();
        self.len = e.seq + 1;
    }

    pub fn empty() -> State {
        State { head: ZERO.to_string(), ..State::default() }
    }
}

fn parse_key(k: &str) -> Result<[u8; 32], String> {
    sign::parse_public(k)
}

/// A validade de uma concessão no intervalo de tempo atestado: assinada por uma autoridade do
/// escopo, para este programa e esta execução, no escopo e no `max_age` dela, e com
/// `issued ≤ lo` e `hi ≤ expires`.
pub fn grant_valid(meta: &Meta, g: &Grant, window: (u64, u64)) -> Result<String, String> {
    let st =
        g.stamp().ok_or_else(|| format!("a concessão {} não tem carimbo (programa, execução, prazo)", short(&g.id)))?;
    if st.program != meta.program {
        return Err(format!("a concessão {} é de outro programa", short(&g.id)));
    }
    if st.run != meta.run {
        return Err(format!("a concessão {} é de outra execução", short(&g.id)));
    }
    let a = meta
        .authorities
        .iter()
        .find(|a| parse_key(&a.key).is_ok_and(|k| st.verify(&k, "grant2", &g.id)))
        .ok_or_else(|| format!("a concessão {} não tem a assinatura de uma autoridade do escopo", short(&g.id)))?;
    if !a.ops.contains(&g.op) {
        return Err(format!("`{}` está fora do escopo da autoridade `{}`", g.op, a.name));
    }
    if st.expires.saturating_sub(st.issued) > a.max_age {
        return Err(format!("a concessão {} tem validade maior que o `max_age` de `{}`", short(&g.id), a.name));
    }
    let (lo, hi) = window;
    if st.issued > lo {
        return Err(format!("a concessão {} ainda não é válida no instante atestado", short(&g.id)));
    }
    if hi > st.expires {
        return Err(format!(
            "a concessão {} está EXPIRADA no instante atestado ({} > {})",
            short(&g.id),
            hi,
            st.expires
        ));
    }
    Ok(a.name.clone())
}

fn short(s: &str) -> &str {
    &s[..12.min(s.len())]
}

fn check_time(meta: &Meta, st: &State, body: &Json) -> Result<Attestation, String> {
    let att = Attestation::from_json(&body["time"]).ok_or("sem tempo atestado")?;
    let key = parse_key(&meta.clock)?;
    if !sign::verify(&key, "time", &time_payload(&att.nonce, att.t), &att.sig) {
        return Err("o tempo não tem a assinatura da testemunha do escopo".into());
    }
    if att.nonce != time_nonce(&meta.scope(), &st.head, &att.salt) {
        return Err("o tempo atestado não foi pedido para esta posição do registro".into());
    }
    if att.t < st.floor {
        return Err(format!("retrocesso temporal: {} abaixo do piso {}", att.t, st.floor));
    }
    Ok(att)
}

/// **As regras da âncora.** Um registro só entra no escopo se a evidência dele confere; um
/// registro com hashes corretos e evidência ausente ou forjada é recusado. As mesmas regras são
/// aplicadas pela âncora (`ath-world`) ao acrescentar e pelo executor ao reler o registro
/// inteiro (`verify_chain`).
pub fn accept(meta: &Meta, st: &State, kind: &str, body: &Json) -> Result<(), String> {
    let key = body["key"].as_str().filter(|k| !k.is_empty()).ok_or("registro sem chave")?;
    match kind {
        "reserve" => {
            if st.ops.contains_key(key) {
                return Err(format!("a chave {key} já tem reserva neste escopo"));
            }
            let args = body["args"].as_array().ok_or("reserva sem argumentos")?;
            if body["args_hash"].as_str() != Some(&args_hash(args)) {
                return Err("o hash dos argumentos não confere".into());
            }
            if body["tool"].as_str().is_none_or(str::is_empty) {
                return Err("reserva sem ferramenta".into());
            }
            let grants = GrantStore::from_json(&json!({ "grants": body["grants"] }))?.grants;
            if grants.is_empty() {
                return Err("reserva sem concessão".into());
            }
            let att = check_time(meta, st, body)?;
            for g in &grants {
                grant_valid(meta, g, att.window(meta.tolerance))?;
                if let Some(k) = st.taken.get(&g.id) {
                    return Err(format!("CONSUMIDA: a concessão {} já está reservada pela chave {k}", short(&g.id)));
                }
            }
            Ok(())
        }
        _ => {
            let op = st.ops.get(key).ok_or_else(|| format!("a chave {key} não tem reserva"))?;
            match kind {
                "send" => {
                    if op.send.is_some() || op.release.is_some() {
                        return Err(format!("a chave {key} já foi autorizada ou liberada"));
                    }
                    let att = check_time(meta, st, body)?;
                    for g in &op.grants {
                        grant_valid(meta, g, att.window(meta.tolerance))?;
                    }
                    Ok(())
                }
                "outcome" => {
                    if op.send.is_none() {
                        return Err(format!("a chave {key} não tem autorização de envio: não há desfecho a registrar"));
                    }
                    if op.outcome.is_some() {
                        return Err(format!("a chave {key} já tem desfecho"));
                    }
                    let r = Receipt::from_json(&body["receipt"]).ok_or("desfecho sem recibo")?;
                    if !r.verify(&parse_key(&meta.outcomes)?) {
                        return Err("FABRICADO: o recibo não tem a assinatura do provedor do escopo".into());
                    }
                    if r.wire != format!("{}:{key}", meta.run) || r.tool != op.tool {
                        return Err("o recibo é de outra operação".into());
                    }
                    if r.args != "-" && r.args != op.args_hash {
                        return Err("o recibo é de outros argumentos".into());
                    }
                    if !matches!(r.status.as_str(), "done" | "refused" | "not_applied") {
                        return Err(format!("estado de recibo desconhecido: `{}`", r.status));
                    }
                    Ok(())
                }
                "release" => {
                    if op.send.is_some() {
                        return Err(format!("a chave {key} tem autorização de envio: a operação pode ter saído"));
                    }
                    if op.release.is_some() {
                        return Err(format!("a chave {key} já foi liberada"));
                    }
                    Ok(())
                }
                "withdraw" => {
                    if op.send.is_none() || op.outcome.is_some() || op.withdraw.is_some() {
                        return Err(format!("a chave {key} não está em envio sem desfecho"));
                    }
                    Ok(())
                }
                other => Err(format!("registro desconhecido: `{other}`")),
            }
        }
    }
}

/// **Relê o registro inteiro** e o reaceita do zero: elos, hashes e evidência de cada registro.
/// Um registro que a âncora tenha aceito indevidamente (ou que outro processo responda no lugar
/// dela) é recusado aqui.
pub fn verify_chain(meta: &Meta, chain: &[Entry]) -> Result<State, String> {
    let mut st = State::empty();
    for (i, e) in chain.iter().enumerate() {
        if e.seq != i as u64 || e.prev != st.head || e.hash != entry_hash(e.seq, &e.prev, &e.kind, &e.body) {
            return Err(format!("L0006: o registro ancorado está quebrado na posição {i}"));
        }
        accept(meta, &st, &e.kind, &e.body).map_err(|m| {
            format!("L0006: o registro ancorado tem, na posição {i}, um `{}` sem evidência: {m}", e.kind)
        })?;
        st.apply(e);
    }
    Ok(st)
}

/// Por que um acréscimo não entrou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendError {
    /// outro executor acrescentou antes: a cabeça mudou
    Conflict,
    /// a âncora recusou a evidência
    Refused(String),
    Unreachable(String),
}

/// O registro unificado, visto pelo executor.
pub trait Registry: std::fmt::Debug {
    fn open(&mut self, meta: &Meta) -> Result<(), String>;
    fn read(&mut self, scope: &str) -> Result<Vec<Entry>, String>;
    /// Acrescenta na posição `seq` (compare-and-swap sobre o tamanho do registro).
    fn append(&mut self, scope: &str, seq: u64, kind: &str, body: Json) -> Result<Entry, AppendError>;
}

/// A âncora em memória, com as mesmas regras (testes de unidade).
#[derive(Debug, Default, Clone)]
pub struct MemRegistry {
    pub scopes: BTreeMap<String, (Meta, Vec<Entry>)>,
}

impl MemRegistry {
    /// O acréscimo como a âncora o faz: confere o tamanho e a evidência, e só então grava.
    pub fn append_to(
        meta: &Meta,
        chain: &mut Vec<Entry>,
        seq: u64,
        kind: &str,
        body: Json,
    ) -> Result<Entry, AppendError> {
        if chain.len() as u64 != seq {
            return Err(AppendError::Conflict);
        }
        let st = verify_chain(meta, chain).map_err(AppendError::Refused)?;
        accept(meta, &st, kind, &body).map_err(AppendError::Refused)?;
        let e = Entry::new(chain, kind, body);
        chain.push(e.clone());
        Ok(e)
    }
}

impl Registry for MemRegistry {
    fn open(&mut self, meta: &Meta) -> Result<(), String> {
        self.scopes.entry(meta.scope()).or_insert_with(|| (meta.clone(), Vec::new()));
        Ok(())
    }

    fn read(&mut self, scope: &str) -> Result<Vec<Entry>, String> {
        Ok(self.scopes.get(scope).map(|s| s.1.clone()).unwrap_or_default())
    }

    fn append(&mut self, scope: &str, seq: u64, kind: &str, body: Json) -> Result<Entry, AppendError> {
        let (meta, chain) =
            self.scopes.get_mut(scope).ok_or_else(|| AppendError::Refused("escopo não aberto".into()))?;
        let meta = meta.clone();
        MemRegistry::append_to(&meta, chain, seq, kind, body)
    }
}

/// A âncora num serviço (`ath-world serve`, operação `lifecycle`).
#[derive(Debug)]
pub struct TcpRegistry {
    addr: SocketAddr,
    timeout: Duration,
}

impl TcpRegistry {
    pub fn new(addr: &str, timeout: f64) -> Result<TcpRegistry, String> {
        Ok(TcpRegistry { addr: resolve(addr)?, timeout: Duration::from_secs_f64(timeout.max(0.05)) })
    }

    fn call(&self, req: Json) -> Result<Json, String> {
        line_call(&self.addr, self.timeout, &req)
    }
}

impl Registry for TcpRegistry {
    fn open(&mut self, meta: &Meta) -> Result<(), String> {
        let j = self.call(json!({ "op": "lifecycle", "do": "open", "scope": meta.scope(), "meta": meta.to_json() }))?;
        if j["ok"] == true {
            Ok(())
        } else {
            Err(j["error"].as_str().unwrap_or("a âncora recusou o escopo").to_string())
        }
    }

    fn read(&mut self, scope: &str) -> Result<Vec<Entry>, String> {
        let j = self.call(json!({ "op": "lifecycle", "do": "read", "scope": scope }))?;
        if j["ok"] != true {
            return Err(j["error"].as_str().unwrap_or("a âncora não leu o escopo").to_string());
        }
        j["chain"]
            .as_array()
            .ok_or("resposta sem registro")?
            .iter()
            .map(|e| Entry::from_json(e).ok_or("registro ilegível".to_string()))
            .collect()
    }

    fn append(&mut self, scope: &str, seq: u64, kind: &str, body: Json) -> Result<Entry, AppendError> {
        let j = self
            .call(json!({ "op": "lifecycle", "do": "append", "scope": scope, "seq": seq, "kind": kind, "body": body }))
            .map_err(AppendError::Unreachable)?;
        if j["ok"] == true {
            return Entry::from_json(&j["entry"])
                .ok_or_else(|| AppendError::Unreachable("resposta sem registro".into()));
        }
        if j["conflict"] == true {
            return Err(AppendError::Conflict);
        }
        Err(AppendError::Refused(j["error"].as_str().unwrap_or("recusado").to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 8032, §7.1, vetores 1, 2 e 3: pares públicos, não credenciais
    const AUTH: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const CLOCK: &str = "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb";
    const BANK: &str = "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7";

    fn sk(s: &str) -> [u8; 32] {
        sign::parse_secret(s).unwrap()
    }

    fn meta() -> Meta {
        Meta {
            program: "prog".into(),
            run: "r".into(),
            authorities: vec![MetaAuthority {
                name: "a".into(),
                key: sign::public_of(&sk(AUTH)),
                ops: vec!["pay".into()],
                max_age: 900,
            }],
            clock: sign::public_of(&sk(CLOCK)),
            tolerance: 2,
            outcomes: sign::public_of(&sk(BANK)),
        }
    }

    fn grant(op: &str, issued: u64, ttl: u64) -> Json {
        grant_n(op, issued, ttl, 0)
    }

    fn grant_n(op: &str, issued: u64, ttl: u64, nonce: u64) -> Json {
        let mut g = Grant::new("r", op, &["A".to_string(), "1".to_string()], "confirmation:t", nonce);
        let st = sign::Stamp::sign(&sk(AUTH), "grant2", &g.id, "prog", "r", issued, issued + ttl);
        (g.program, g.issued, g.expires, g.sig) = (Some(st.program), Some(st.issued), Some(st.expires), Some(st.sig));
        g.to_json()
    }

    fn time(meta: &Meta, st: &State, t: u64) -> Json {
        let mut k = TimeKeeper::new(
            Box::new(MemTime { secret: sk(CLOCK), now: t }),
            sign::parse_public(&meta.clock).unwrap(),
            2,
        );
        k.attest(&meta.scope(), &st.head).unwrap().to_json()
    }

    fn reserve(meta: &Meta, chain: &mut Vec<Entry>, key: &str, g: Json, t: u64) -> Result<Entry, AppendError> {
        let st = verify_chain(meta, chain).unwrap();
        let args = vec![json!("A"), json!("1")];
        let body = json!({ "key": key, "tool": "pay", "args": args, "args_hash": args_hash(&args), "intent": "i",
                           "grants": [g], "time": time(meta, &st, t) });
        MemRegistry::append_to(meta, chain, chain.len() as u64, "reserve", body)
    }

    fn step(meta: &Meta, chain: &mut Vec<Entry>, kind: &str, key: &str, t: Option<u64>) -> Result<Entry, AppendError> {
        let st = verify_chain(meta, chain).unwrap();
        let mut body = json!({ "key": key });
        if let Some(t) = t {
            body["time"] = time(meta, &st, t);
        }
        MemRegistry::append_to(meta, chain, chain.len() as u64, kind, body)
    }

    fn refused(r: Result<Entry, AppendError>, why: &str) {
        match r {
            Err(AppendError::Refused(m)) => assert!(m.contains(why), "esperado `{why}`: {m}"),
            other => panic!("esperado recusa `{why}`: {other:?}"),
        }
    }

    /// A âncora só aceita o que a evidência sustenta: concessão válida no tempo atestado,
    /// linearidade, autorização de envio antes de desfecho, recibo assinado da operação certa,
    /// liberação só sem envio.
    #[test]
    fn the_anchor_accepts_only_what_the_evidence_supports() {
        let m = meta();
        let mut c = Vec::new();
        let g = grant("pay", 1000, 600);
        // expirada, futura, fora do escopo
        refused(reserve(&m, &mut c, "k1", g.clone(), 1599), "EXPIRADA");
        refused(reserve(&m, &mut c, "k1", g.clone(), 1000), "ainda não é válida");
        refused(reserve(&m, &mut c, "k1", grant("wire", 1000, 600), 1100), "fora do escopo");
        reserve(&m, &mut c, "k1", g.clone(), 1100).unwrap();
        // linear: a mesma concessão não é reservada de novo; o tempo não volta
        refused(reserve(&m, &mut c, "k2", g.clone(), 1200), "CONSUMIDA");
        refused(step(&m, &mut c, "send", "k1", Some(1050)), "retrocesso");
        // desfecho sem envio, recibo forjado, recibo de outra operação
        let r = Receipt::sign(&sk(BANK), "r:k1", "pay", &args_hash(&[json!("A"), json!("1")]), "done", "ok");
        refused(
            MemRegistry::append_to(&m, &mut c, 1, "outcome", json!({ "key": "k1", "receipt": r.to_json() })),
            "não tem autorização de envio",
        );
        step(&m, &mut c, "send", "k1", Some(1110)).unwrap();
        let forged = Receipt::sign(&sk(AUTH), "r:k1", "pay", &r.args, "done", "ok");
        refused(
            MemRegistry::append_to(&m, &mut c, 2, "outcome", json!({ "key": "k1", "receipt": forged.to_json() })),
            "FABRICADO",
        );
        let other = Receipt::sign(&sk(BANK), "r:k9", "pay", &r.args, "done", "ok");
        refused(
            MemRegistry::append_to(&m, &mut c, 2, "outcome", json!({ "key": "k1", "receipt": other.to_json() })),
            "outra operação",
        );
        // depois do envio não há liberação: a operação pode ter saído
        refused(step(&m, &mut c, "release", "k1", None), "pode ter saído");
        MemRegistry::append_to(&m, &mut c, 2, "outcome", json!({ "key": "k1", "receipt": r.to_json() })).unwrap();
        // uma reserva sem envio é liberada, e a concessão volta
        let g2 = grant_n("pay", 1000, 600, 1);
        reserve(&m, &mut c, "k2", g2.clone(), 1200).unwrap();
        step(&m, &mut c, "release", "k2", None).unwrap();
        refused(step(&m, &mut c, "send", "k2", Some(1210)), "liberada");
        reserve(&m, &mut c, "k3", g2, 1220).unwrap();
        let st = verify_chain(&m, &c).unwrap();
        assert_eq!((st.len, st.floor, st.taken.len()), (6, 1220, 2));
        // o registro inteiro é reaceito do zero; um registro alterado não é
        let mut bad = c.clone();
        bad[3].body["receipt"]["value"] = json!("outro");
        bad[3].hash = entry_hash(bad[3].seq, &bad[3].prev, &bad[3].kind, &bad[3].body);
        assert!(verify_chain(&m, &bad).unwrap_err().contains("L0006"));
    }

    /// O verificador de tempo: repetição, atraso, retrocesso e congelamento.
    #[test]
    fn the_time_keeper_rejects_replay_rewind_and_freeze() {
        #[derive(Debug)]
        struct Replay(Option<Json>, MemTime);
        impl TimeSource for Replay {
            fn ask(&mut self, nonce: &str) -> Result<Json, String> {
                let fresh = self.1.ask(nonce)?;
                Ok(self.0.replace(fresh.clone()).unwrap_or(fresh))
            }
        }
        let key = sign::parse_public(&sign::public_of(&sk(CLOCK))).unwrap();
        let mut k = TimeKeeper::new(Box::new(Replay(None, MemTime { secret: sk(CLOCK), now: 100 })), key, 2);
        k.attest("s", ZERO).unwrap();
        assert_eq!(k.attest("s", ZERO).unwrap_err().code(), "T0001");
        let mut k = TimeKeeper::new(Box::new(MemTime { secret: sk(AUTH), now: 100 }), key, 2);
        assert_eq!(k.attest("s", ZERO).unwrap_err().code(), "T0001");
        let mut k = TimeKeeper::new(Box::new(MemTime { secret: sk(CLOCK), now: 100 }), key, 2);
        k.floor = 200;
        assert_eq!(k.attest("s", ZERO).unwrap_err().code(), "T0002");
    }
}

// ---------------------------------------------------------------- o protocolo no executor

use crate::host::{HostError, Status};
use crate::{rt, Arx, RtError, Value};
use ath_syntax::ast::{ProviderClass, ToolDecl, TypeExpr};

/// A mensagem de quem não pode realizar o protocolo RC4 (o mesmo texto no ARX e no runtime
/// Python, que recusam igual).
pub const R0639: &str = "o protocolo RC4 (autoridade temporal) exige o registro de ciclo de vida ancorado, o tempo \
autenticado e recibos assinados do provedor; esta execução não os tem";

/// O protocolo ligado a um executor.
#[derive(Debug)]
pub struct Lc {
    pub reg: Box<dyn Registry>,
    pub time: TimeKeeper,
    pub meta: Meta,
    pub scope: String,
    outcomes: [u8; 32],
    /// chave → (ferramenta, hash dos argumentos, concessões) das operações com envio
    /// autorizado neste processo
    sent: BTreeMap<u64, (String, String, Vec<Grant>)>,
    /// o prazo das fontes `Trusted` aceitas (fotografia, pedido), revalidado na fronteira
    pub stamps: Vec<(&'static str, u64)>,
}

fn time_err(e: TimeError) -> RtError {
    rt(e.code(), e.to_string())
}

impl<'m> Arx<'m> {
    /// A configuração de confiança das autoridades temporais do módulo: as que declaram escopo,
    /// prazo, registro ancorado, `clock` e `outcomes`. Todas compartilham as testemunhas.
    pub fn temporal_meta(&self) -> Result<(Meta, [u8; 32], [u8; 32]), String> {
        let ts: Vec<&crate::Authority> = self.authorities.iter().filter(|a| a.temporal).collect();
        let first = ts.first().ok_or("o programa não declara autoridade temporal")?;
        let (Some((ck, tol)), Some(ok)) = (first.clock.clone(), first.outcomes.clone()) else {
            return Err("autoridade temporal sem testemunhas".into());
        };
        if ts.iter().any(|a| a.clock.as_ref() != Some(&(ck.clone(), tol)) || a.outcomes.as_ref() != Some(&ok)) {
            return Err("as autoridades temporais de um programa compartilham `clock` e `outcomes`".into());
        }
        let authorities = ts
            .iter()
            .map(|a| {
                let mut ops = a.ops.clone().unwrap_or_default();
                ops.sort();
                MetaAuthority { name: a.name.clone(), key: a.key_text.clone(), ops, max_age: a.max_age.unwrap_or(0) }
            })
            .collect();
        let meta = Meta {
            program: self.program.clone(),
            run: self.run_id.clone(),
            authorities,
            clock: ck.clone(),
            tolerance: tol,
            outcomes: ok.clone(),
        };
        Ok((meta, parse_key(&ck)?, parse_key(&ok)?))
    }

    /// Liga o executor ao registro unificado e à testemunha de tempo, e abre o escopo.
    pub fn attach_lifecycle(&mut self, mut reg: Box<dyn Registry>, time: Box<dyn TimeSource>) -> Result<(), String> {
        let (meta, ck, ok) = self.temporal_meta()?;
        reg.open(&meta).map_err(|e| format!("L0005: a âncora não abriu o escopo: {e}"))?;
        let scope = meta.scope();
        let keeper = TimeKeeper::new(time, ck, meta.tolerance);
        self.lc = Some(Lc { reg, time: keeper, meta, scope, outcomes: ok, sent: BTreeMap::new(), stamps: Vec::new() });
        Ok(())
    }

    /// O intervalo do instante real: o atestado pela testemunha (programa temporal) ou o
    /// relógio do executor (formas anteriores, que não dão garantia contra ele).
    pub(crate) fn time_bounds(&self) -> (u64, u64) {
        match self.lc.as_ref().and_then(|l| l.time.bounds()) {
            Some(b) => b,
            None => (self.now_unix, self.now_unix),
        }
    }

    /// Lê e reaceita o registro ancorado do escopo.
    fn lc_state(&mut self, lc: &mut Lc) -> Result<State, RtError> {
        let chain = lc.reg.read(&lc.scope).map_err(|e| rt("L0005", format!("a âncora não respondeu: {e}")))?;
        let st = verify_chain(&lc.meta, &chain).map_err(|m| rt("L0006", m))?;
        lc.time.floor = lc.time.floor.max(st.floor);
        Ok(st)
    }

    /// No início da execução: o registro é reaceito, o tempo é atestado contra o piso dele, e as
    /// concessões já reservadas não são encontradas livres.
    pub fn lc_start(&mut self) -> Result<(), RtError> {
        if !self.temporal {
            return Ok(());
        }
        let Some(mut lc) = self.lc.take() else { return Err(rt("R0639", R0639)) };
        let r = (|| {
            let st = self.lc_state(&mut lc)?;
            let att = lc.time.attest(&lc.scope, &st.head).map_err(time_err)?;
            self.grants.spent.extend(st.taken.keys().cloned());
            // as chaves são do escopo, não do processo: uma nova execução na mesma `run` numera
            // depois da maior chave já ancorada (uma chave liberada nunca é reaproveitada)
            let top = st.ops.keys().filter_map(|k| k.strip_prefix('k')?.parse::<u64>().ok()).max().unwrap_or(0);
            self.next_key = self.next_key.max(top + 1);
            self.journal.push(self.clock, "time", json!({ "t": att.t, "floor": st.floor, "anchored": st.len }));
            Ok(())
        })();
        self.lc = Some(lc);
        r
    }

    /// Guarda o prazo de uma fonte `Trusted` aceita, para revalidá-la na fronteira.
    pub(crate) fn lc_stamp(&mut self, what: &'static str, expires: u64) {
        if let Some(lc) = self.lc.as_mut() {
            lc.stamps.push((what, expires));
        }
    }

    fn sources_valid(lc: &Lc, window: (u64, u64)) -> Result<(), String> {
        match lc.stamps.iter().find(|(_, e)| window.1 > *e) {
            Some((what, e)) => Err(format!("a fonte `Trusted` `{what}` EXPIROU antes do efeito ({} > {e})", window.1)),
            None => Ok(()),
        }
    }

    fn lc_fail(&mut self, key: &str, evidence: &str, release: Option<String>) {
        let mut d = json!({ "key": key, "status": "failed", "evidence": evidence });
        if let Some(h) = release {
            d["release"] = json!(h);
        }
        self.wlog("close", d);
    }

    /// Libera a reserva de uma operação que **provadamente não saiu** (não há `send`).
    fn lc_release(&mut self, lc: &mut Lc, key: &str) -> Option<String> {
        for _ in 0..8 {
            let st = self.lc_state(lc).ok()?;
            match lc.reg.append(&lc.scope, st.len, "release", json!({ "key": key })) {
                Ok(e) => return Some(e.hash),
                Err(AppendError::Conflict) => continue,
                Err(_) => return None,
            }
        }
        None
    }

    /// **A fronteira do efeito** (RC4). Na ordem:
    ///
    /// 1. o registro é reaceito; as concessões reivindicadas não podem estar reservadas;
    /// 2. o tempo é atestado (para esta posição do registro) e as concessões e as fontes
    ///    `Trusted` são conferidas nele;
    /// 3. a intenção vai para o WAL, citando as concessões;
    /// 4. `reserve` é acrescentado à âncora (que reconfere tudo);
    /// 5. **imediatamente antes do efeito**, o tempo é atestado de novo e tudo é reconferido;
    ///    expirado, a reserva é liberada (nada saiu) e a chamada é recusada (`R0638`);
    /// 6. `send` é acrescentado à âncora. Só então o efeito pode sair.
    pub(crate) fn lc_boundary(
        &mut self,
        name: &str,
        t: &ToolDecl,
        args: &[Value],
        k: u64,
        claims: &[usize],
    ) -> Result<(), RtError> {
        let mut lc = self.lc.take().ok_or_else(|| rt("R0639", R0639))?;
        let r = self.lc_boundary_in(&mut lc, name, t, args, k, claims);
        self.lc = Some(lc);
        r
    }

    fn lc_boundary_in(
        &mut self,
        lc: &mut Lc,
        name: &str,
        t: &ToolDecl,
        args: &[Value],
        k: u64,
        claims: &[usize],
    ) -> Result<(), RtError> {
        let key = format!("k{k}");
        let plain = crate::remote::plain_args(args);
        let ah = args_hash(&plain);
        let grants: Vec<Grant> = claims.iter().map(|&i| self.grants.grants[i].clone()).collect();
        let ids: Vec<String> = grants.iter().map(|g| g.id.clone()).collect();
        let tol = lc.meta.tolerance;
        let mut intent: Option<String> = None;
        let mut reserved = None;
        for _ in 0..8 {
            let st = self.lc_state(lc)?;
            if let Some((id, other)) = ids.iter().find_map(|id| st.taken.get(id).map(|o| (id, o))) {
                self.journal.push(self.clock, "contract_refused", json!({ "tool": name, "linear": true, "taken": id }));
                if intent.is_some() {
                    self.lc_fail(&key, "refused:R0632", None);
                }
                return Err(rt(
                    "R0632",
                    format!(
                        "`{name}`: a concessão {} já está reservada no registro ancorado (chave {other})",
                        short(id)
                    ),
                ));
            }
            let att = lc.time.attest(&lc.scope, &st.head).map_err(|e| {
                if intent.is_some() {
                    self.lc_fail(&key, &format!("refused:{}", e.code()), None);
                }
                time_err(e)
            })?;
            let window = att.window(tol);
            if let Err(m) = grants
                .iter()
                .try_for_each(|g| grant_valid(&lc.meta, g, window).map(|_| ()))
                .and_then(|_| Self::sources_valid(lc, window))
            {
                self.journal.push(
                    self.clock,
                    "expired_at_boundary",
                    json!({ "tool": name, "key": key, "t": att.t, "why": m }),
                );
                if intent.is_some() {
                    self.lc_fail(&key, "refused:R0638", None);
                }
                return Err(rt("R0638", format!("`{name}`: {m}; nenhum efeito")));
            }
            if intent.is_none() {
                let sid = self.saga_ids.last().copied();
                self.wlog(
                    "intent",
                    json!({ "key": key, "tool": name, "provider": t.provider.name(), "obligation": "reconcile",
                            "saga": sid, "args": args.iter().map(|v| v.to_json()).collect::<Vec<_>>(), "grants": ids }),
                );
                intent = Some(self.wal.as_ref().and_then(|w| w.last_hash()).unwrap_or_default());
                self.wcrash(crate::wal::CrashPoint::Logged, k);
            }
            let body = json!({ "key": key, "tool": name, "args": plain, "args_hash": ah, "intent": intent,
                               "grants": grants.iter().map(Grant::to_json).collect::<Vec<_>>(), "time": att.to_json() });
            match lc.reg.append(&lc.scope, st.len, "reserve", body) {
                Ok(e) => {
                    reserved = Some(e);
                    break;
                }
                Err(AppendError::Conflict) => continue,
                Err(AppendError::Refused(m)) => {
                    self.lc_fail(&key, "refused:anchor", None);
                    let code = if m.contains("CONSUMIDA") || m.contains("já tem reserva") { "R0632" } else { "L0007" };
                    return Err(rt(code, format!("`{name}`: a âncora recusou a reserva: {m}")));
                }
                Err(AppendError::Unreachable(m)) => {
                    self.lc_fail(&key, "refused:L0005", None);
                    return Err(rt("L0005", format!("a âncora não respondeu: {m}; nenhum efeito")));
                }
            }
        }
        let reserved = reserved.ok_or_else(|| rt("L0005", "a âncora não aceitou a reserva depois de 8 conflitos"))?;
        self.journal.push(self.clock, "reserved", json!({ "tool": name, "key": key, "entry": reserved.hash }));
        self.wcrash(crate::wal::CrashPoint::Reserved, k);
        // a revalidação imediatamente antes do efeito
        for _ in 0..8 {
            let st = self.lc_state(lc)?;
            let att = match lc.time.attest(&lc.scope, &st.head) {
                Ok(a) => a,
                Err(e) => {
                    let h = self.lc_release(lc, &key);
                    self.lc_fail(&key, &format!("released:{}", e.code()), h);
                    return Err(time_err(e));
                }
            };
            let window = att.window(tol);
            if let Err(m) = grants
                .iter()
                .try_for_each(|g| grant_valid(&lc.meta, g, window).map(|_| ()))
                .and_then(|_| Self::sources_valid(lc, window))
            {
                self.journal.push(
                    self.clock,
                    "expired_at_boundary",
                    json!({ "tool": name, "key": key, "t": att.t, "why": m }),
                );
                let h = self.lc_release(lc, &key);
                self.lc_fail(&key, "released:R0638", h);
                return Err(rt(
                    "R0638",
                    format!("`{name}`: {m}, entre a reserva e o envio; a reserva foi liberada e nada saiu"),
                ));
            }
            match lc.reg.append(&lc.scope, st.len, "send", json!({ "key": key, "time": att.to_json() })) {
                Ok(e) => {
                    self.journal.push(
                        self.clock,
                        "cleared",
                        json!({ "tool": name, "key": key, "t": att.t, "entry": e.hash }),
                    );
                    lc.sent.insert(k, (name.to_string(), ah, grants));
                    return Ok(());
                }
                Err(AppendError::Conflict) => continue,
                Err(AppendError::Refused(m)) => {
                    let h = self.lc_release(lc, &key);
                    self.lc_fail(&key, "released:anchor", h);
                    return Err(rt("L0007", format!("`{name}`: a âncora recusou a autorização de envio: {m}")));
                }
                Err(AppendError::Unreachable(m)) => {
                    // a reserva fica; sem `send`, a recuperação a libera
                    self.lc_fail(&key, "refused:L0005", None);
                    return Err(rt("L0005", format!("a âncora não respondeu: {m}; nenhum efeito")));
                }
            }
        }
        Err(rt("L0005", "a âncora não aceitou a autorização de envio depois de 8 conflitos"))
    }

    /// **O desfecho ancorado.** Chamado ao fechar uma operação com envio autorizado: o recibo do
    /// provedor é conferido e acrescentado à âncora, e o `close` do WAL o cita. Sem recibo, o
    /// `close` fica sem evidência ancorada (e a recuperação não o toma por desfecho). Uma
    /// operação que o provedor não chegou a receber (`unsent`) é retirada (`withdraw`): o
    /// executor declara que não a reenviará.
    pub(crate) fn lc_settle(&mut self, k: u64, status: &str, via: &str) -> Option<(&'static str, String)> {
        let mut lc = self.lc.take()?;
        let r = self.lc_settle_in(&mut lc, k, status, via);
        self.lc = Some(lc);
        r
    }

    fn lc_settle_in(&mut self, lc: &mut Lc, k: u64, status: &str, via: &str) -> Option<(&'static str, String)> {
        let (tool, ah, _) = lc.sent.get(&k)?.clone();
        let key = format!("k{k}");
        let wire = format!("{}:{key}", self.run_id);
        let (kind, body) = match self.host.receipt(&wire).and_then(|j| Receipt::from_json(&j)) {
            Some(r) => {
                let agrees = (status == "done") == (r.status == "done");
                if !r.verify(&lc.outcomes)
                    || r.wire != wire
                    || r.tool != tool
                    || (r.args != ah && r.args != "-")
                    || !agrees
                {
                    self.journal.push(self.clock, "receipt_refused", json!({ "key": key, "status": status }));
                    return None;
                }
                ("outcome", json!({ "key": key, "receipt": r.to_json() }))
            }
            None if status == "failed" && via == "unsent" => ("withdraw", json!({ "key": key })),
            None => return None,
        };
        for _ in 0..8 {
            let st = self.lc_state(lc).ok()?;
            match lc.reg.append(&lc.scope, st.len, kind, body.clone()) {
                Ok(e) => {
                    lc.sent.remove(&k);
                    return Some((kind, e.hash));
                }
                Err(AppendError::Conflict) => continue,
                Err(e) => {
                    self.journal.push(
                        self.clock,
                        "outcome_not_anchored",
                        json!({ "key": key, "error": format!("{e:?}") }),
                    );
                    return None;
                }
            }
        }
        None
    }

    /// **A última conferência, imediatamente antes do efeito sair** (`Step.emit` em
    /// `Lifecycle.lean`): a janela do instante real, a partir da atestação do `send` e do relógio
    /// monotônico desde ela, ainda cabe no prazo de cada concessão. Sem rede: só o tempo que
    /// passou desde a última leitura. Se não cabe, o executor retira a operação (`withdraw`) e
    /// nada sai.
    pub(crate) fn lc_final(&mut self, k: u64) -> Result<(), RtError> {
        let Some(mut lc) = self.lc.take() else { return Ok(()) };
        let r = (|| {
            let Some((_, _, grants)) = lc.sent.get(&k).cloned() else { return Ok(()) };
            let window = lc.time.bounds().ok_or_else(|| rt("T0005", "sem leitura de tempo antes do efeito"))?;
            if let Err(m) = grants.iter().try_for_each(|g| grant_valid(&lc.meta, g, window).map(|_| ())) {
                let key = format!("k{k}");
                if let Ok(st) = self.lc_state(&mut lc) {
                    if let Ok(e) = lc.reg.append(&lc.scope, st.len, "withdraw", json!({ "key": key })) {
                        lc.sent.remove(&k);
                        self.lc_fail(&key, "withdrawn:R0638", None);
                        self.journal.push(self.clock, "withdrawn", json!({ "key": key, "entry": e.hash }));
                    }
                }
                return Err(rt(
                    "R0638",
                    format!("k{k}: {m}, no instante do envio; a operação foi retirada e nada saiu"),
                ));
            }
            Ok(())
        })();
        self.lc = Some(lc);
        r
    }

    /// Antes de um reenvio na reconciliação dentro da execução: o mesmo tempo autenticado e a
    /// mesma validade que a fronteira exige.
    pub(crate) fn lc_recheck(&mut self, k: u64) -> Result<(), RtError> {
        let Some(mut lc) = self.lc.take() else { return Ok(()) };
        let r = (|| {
            let Some((_, _, grants)) = lc.sent.get(&k).cloned() else { return Ok(()) };
            let st = self.lc_state(&mut lc)?;
            let att = lc.time.attest(&lc.scope, &st.head).map_err(time_err)?;
            for g in &grants {
                grant_valid(&lc.meta, g, att.window(lc.meta.tolerance))
                    .map_err(|m| rt("R0638", format!("reenvio de k{k}: {m}")))?;
            }
            Ok(())
        })();
        self.lc = Some(lc);
        r
    }

    /// **A recuperação a partir do registro ancorado.** Antes de qualquer contato com o
    /// provedor, o registro inteiro é reaceito (`L0006`), o tempo é atestado contra o piso
    /// (`T000x`) e o WAL é confrontado com a âncora (`W0008`): um desfecho local que cita um
    /// registro que a âncora não tem, ou que diz outra coisa, é recusado. Depois, para cada
    /// operação ancorada:
    ///
    /// * com desfecho ancorado: o `close` do WAL é restaurado dele, sem consultar o provedor;
    /// * reservada **sem** `send`: provadamente não saiu; a reserva é liberada na âncora e a
    ///   concessão volta a estar livre;
    /// * com `send` e sem desfecho: pode ter saído; a obrigação segue o protocolo do provedor.
    ///   `idempotent` só reenvia se as concessões ainda valem no tempo atestado agora (um reenvio
    ///   pode ser o primeiro envio que chega); `queryable` consulta; `opaque` (e o que o executor
    ///   retirou) fica escalado: o estado desconhecido é preservado, não resolvido.
    ///
    /// Uma intenção do WAL sem reserva ancorada provadamente não saiu (a reserva precede o envio).
    pub fn lc_recover(&mut self, fuel: u32) -> Result<(), String> {
        let mut lc = self.lc.take().ok_or(R0639)?;
        let r = self.lc_recover_in(&mut lc, fuel);
        self.lc = Some(lc);
        r
    }

    fn lc_recover_in(&mut self, lc: &mut Lc, fuel: u32) -> Result<(), String> {
        let msg = |e: RtError| format!("{}: {}", e.code, e.msg);
        let st = self.lc_state(lc).map_err(msg)?;
        let att = lc.time.attest(&lc.scope, &st.head).map_err(|e| e.to_string())?;
        self.journal.push(self.clock, "time", json!({ "t": att.t, "floor": st.floor, "anchored": st.len }));
        let entries: Vec<Json> = self.wal.as_ref().map(|w| w.log.entries.clone()).unwrap_or_default();
        let detail = |e: &Json| e["detail"].clone();
        // 1. o WAL contra a âncora
        for e in entries.iter().filter(|e| e["kind"] == "close") {
            let d = detail(e);
            let key = d["key"].as_str().unwrap_or_default();
            let op = st.ops.get(key);
            for field in ["outcome", "release", "withdraw"] {
                let Some(h) = d[field].as_str() else { continue };
                let anchored = op.and_then(|o| match field {
                    "outcome" => o.outcome.as_ref().map(|x| x.0.clone()),
                    "release" => o.release.clone(),
                    _ => o.withdraw.clone(),
                });
                if anchored.as_deref() != Some(h) {
                    return Err(format!(
                        "W0008: o desfecho local de {key} cita o registro ancorado #{} que a âncora não tem: desfecho fabricado ou alterado. Nada foi enviado",
                        short(h)
                    ));
                }
                if let (true, Some((_, r))) = (field == "outcome", op.and_then(|o| o.outcome.as_ref())) {
                    if (d["status"] == "done") != (r.status == "done") {
                        return Err(format!(
                            "W0008: o desfecho local de {key} diz `{}` e o recibo ancorado diz `{}`. Nada foi enviado",
                            d["status"], r.status
                        ));
                    }
                }
            }
        }
        for e in entries.iter().filter(|e| e["kind"] == "intent") {
            let d = detail(e);
            let key = d["key"].as_str().unwrap_or_default();
            if let Some(op) = st.ops.get(key) {
                let ids: Vec<&str> = d["grants"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect();
                let anchored: Vec<&str> = op.grants.iter().map(|g| g.id.as_str()).collect();
                if d["tool"].as_str() != Some(&op.tool) || ids != anchored {
                    return Err(format!(
                        "W0008: a intenção local de {key} não é a operação ancorada. Nada foi enviado"
                    ));
                }
            }
        }
        let cited = |field: &str, key: &str, h: &str| {
            entries.iter().any(|e| e["kind"] == "close" && e["detail"]["key"] == key && e["detail"][field] == h)
        };
        // 2. o que a âncora sabe
        for key in st.order.clone() {
            let op = st.ops[&key].clone();
            self.lc_handled.insert(key.clone());
            let ids: Vec<String> = op.grants.iter().map(|g| g.id.clone()).collect();
            if let Some((h, r)) = &op.outcome {
                if !cited("outcome", &key, h) {
                    let status = if r.status == "done" { "done" } else { "failed" };
                    self.wlog(
                        "close",
                        json!({ "key": key, "status": status, "evidence": "anchor:outcome", "outcome": h }),
                    );
                }
                continue;
            }
            if let Some(h) = &op.release {
                if !cited("release", &key, h) {
                    self.wlog("close", json!({ "key": key, "status": "failed", "evidence": "anchor:released", "release": h, "released": ids }));
                }
                continue;
            }
            if op.send.is_none() {
                let h = self
                    .lc_release(lc, &key)
                    .ok_or_else(|| format!("L0005: a âncora não aceitou a liberação de {key}"))?;
                self.wlog("close", json!({ "key": key, "status": "failed", "evidence": "anchor:unsent", "release": h, "released": ids }));
                continue;
            }
            self.lc_resolve(lc, &op, fuel);
        }
        // 3. intenções que não chegaram a reservar
        let wst = match &self.wal {
            Some(w) => crate::wal::WalState::of(&w.log),
            None => crate::wal::WalState::default(),
        };
        for (key, d) in wst.intents {
            let cites = d["grants"].as_array().is_some_and(|a| !a.is_empty());
            if cites && !st.ops.contains_key(&key) && !wst.outcome.contains_key(&key) {
                self.lc_handled.insert(key.clone());
                self.wlog("close", json!({ "key": key, "status": "failed", "evidence": "anchor:unreserved" }));
            }
        }
        Ok(())
    }

    /// Uma operação com `send` ancorado e sem desfecho: pode ter saído.
    fn lc_resolve(&mut self, lc: &mut Lc, op: &Op, fuel: u32) {
        let key = op.key.clone();
        let escalate = |me: &mut Self, reason: String, attempts: u32| {
            me.wlog("escalate", json!({ "key": key, "reason": reason, "attempts": attempts }));
        };
        let Some(decl) = self.tools.get(op.tool.as_str()).copied() else {
            return escalate(self, "ferramenta ausente do programa".into(), 0);
        };
        let kn: u64 = key.strip_prefix('k').and_then(|x| x.parse().ok()).unwrap_or_default();
        let wire = format!("{}:{key}", self.run_id);
        match (decl.provider, op.withdraw.is_some()) {
            (ProviderClass::Opaque, _) => {
                return escalate(
                    self,
                    "provedor opaco: se a operação saiu não é decidível a partir da evidência durável".into(),
                    0,
                )
            }
            (ProviderClass::Idempotent, true) => {
                return escalate(self, "retirada pelo executor: não é reenviada; o provedor não tem consulta".into(), 0)
            }
            _ => {}
        }
        // os argumentos, reconstruídos do registro ancorado
        let mut plain = op.args.iter();
        let mut args = Vec::new();
        for p in &decl.params {
            let v = match p.ty.unlabeled() {
                TypeExpr::Cap(..) => self.arg_from_json(&json!({ "cap": [] }), &p.ty),
                t if t.base_name() == "IdemKey" => Some(Value::Key(kn)),
                _ => plain.next().and_then(|j| self.arg_from_json(j, &p.ty)),
            };
            match v {
                Some(v) => args.push(v),
                None => return escalate(self, "argumentos ancorados ilegíveis".into(), 0),
            }
        }
        for attempt in 1..=fuel.max(1) {
            if decl.provider == ProviderClass::Idempotent {
                // um reenvio pode ser o primeiro envio que chega: exige validade agora
                let st = match self.lc_state(lc) {
                    Ok(s) => s,
                    Err(e) => return escalate(self, format!("{}: {}", e.code, e.msg), attempt - 1),
                };
                let window = match lc.time.attest(&lc.scope, &st.head) {
                    Ok(a) => a.window(lc.meta.tolerance),
                    Err(e) => return escalate(self, e.to_string(), attempt - 1),
                };
                if let Err(m) = op.grants.iter().try_for_each(|g| grant_valid(&lc.meta, g, window).map(|_| ())) {
                    return escalate(
                        self,
                        format!("R0638: {m}; o reenvio seria um efeito novo sem autorização válida"),
                        attempt - 1,
                    );
                }
                // a última conferência, sem rede, imediatamente antes do reenvio
                let now = lc.time.bounds().unwrap_or(window);
                if op.grants.iter().any(|g| grant_valid(&lc.meta, g, now).is_err()) {
                    return escalate(
                        self,
                        "R0638: o prazo passou entre a leitura de tempo e o reenvio".into(),
                        attempt - 1,
                    );
                }
                match self.host.call(&op.tool, &args) {
                    Ok(_) | Err(HostError::Failed(_)) => {}
                    Err(_) => continue,
                }
            } else {
                if matches!(self.host.status(&op.tool, kn).0, Status::Unknown) {
                    continue;
                }
            }
            let Some(r) = self.host.receipt(&wire).and_then(|j| Receipt::from_json(&j)) else { continue };
            for _ in 0..8 {
                let Ok(st) = self.lc_state(lc) else { break };
                match lc.reg.append(&lc.scope, st.len, "outcome", json!({ "key": key, "receipt": r.to_json() })) {
                    Ok(e) => {
                        let status = if r.status == "done" { "done" } else { "failed" };
                        let ev = if decl.provider == ProviderClass::Idempotent {
                            "recovery:resend"
                        } else {
                            "recovery:status"
                        };
                        self.wlog("close", json!({ "key": key, "status": status, "evidence": ev, "attempts": attempt, "outcome": e.hash }));
                        return;
                    }
                    Err(AppendError::Conflict) => continue,
                    Err(e) => return escalate(self, format!("a âncora recusou o recibo: {e:?}"), attempt),
                }
            }
        }
        escalate(self, "sem desfecho com evidência depois das tentativas".into(), fuel)
    }
}
