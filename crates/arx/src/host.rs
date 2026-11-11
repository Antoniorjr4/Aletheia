//! Ligações de host: implementações reais das ferramentas declaradas em `.ath`.
//! O Marco Zero traz um mundo simulado (cluster + pager) carregado de JSON, com falhas
//! injetáveis para exercitar sagas e reconciliação.

use crate::value::Value;
use serde_json::Value as Json;
use std::collections::{BTreeMap, HashMap};

/// Fotografia imutável e confiável do inventário/telemetria, produzida pelo host,
/// jamais pelos logs nem pelo modelo. Uma fotografia vazia falha fechado.
#[derive(Clone, Debug, Default)]
pub struct TrustedSnapshot {
    pub managed: Vec<String>,
    pub replicas: BTreeMap<String, i64>,
    pub cpu_pct: BTreeMap<String, i64>,
    /// Inventários confiáveis nomeados (`s.inventory("k")`, `s.member("k", x)`), ordenados e
    /// sem repetição. Um inventário ausente é vazio: a política falha fechado.
    pub inventories: BTreeMap<String, Vec<String>>,
}

/// Um decimal não negativo (`123`, `98.70`, `1,050.00`) em milionésimos; `None` se ilegível.
/// O runtime Python usa a mesma leitura (`aletheia_runtime.runtime.decimal`).
pub fn decimal(x: &str) -> Option<i128> {
    let t: String = x.trim().chars().filter(|&ch| ch != ',').collect();
    let (int, frac) = t.split_once('.').unwrap_or((&t, ""));
    if int.is_empty() || int.len() > 18 || frac.len() > 6 {
        return None;
    }
    if !int.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut v: i128 = int.parse().ok()?;
    v *= 1_000_000;
    let f = format!("{frac:0<6}");
    v += f.parse::<i128>().ok()?;
    Some(v)
}

impl TrustedSnapshot {
    pub fn inventory_text(&self, name: &str) -> String {
        self.inventories.get(name).map(|v| v.join(", ")).unwrap_or_default()
    }

    pub fn member(&self, name: &str, x: &str) -> bool {
        self.inventories.get(name).is_some_and(|v| v.iter().any(|s| s == x))
    }

    /// `s.within("k", x)`: `x` (decimal) não passa do limite confiável `k` (o primeiro item do
    /// inventário). Inventário ausente ou número ilegível: falha fechado.
    pub fn within(&self, name: &str, x: &str) -> bool {
        match (self.inventories.get(name).and_then(|v| v.first()).and_then(|l| decimal(l)), decimal(x)) {
            (Some(l), Some(v)) => v <= l,
            _ => false,
        }
    }

    /// `s.mentions("k", x)`: `x`, não vazio, aparece literalmente num texto confiável `k`.
    pub fn mentions(&self, name: &str, x: &str) -> bool {
        !x.is_empty() && self.inventories.get(name).is_some_and(|v| v.iter().any(|s| s.contains(x)))
    }

    pub fn managed_text(&self) -> String {
        self.managed.join(", ")
    }

    pub fn telemetry_text(&self) -> String {
        self.managed
            .iter()
            .map(|d| {
                let cpu = self.cpu_pct.get(d).map(|n| n.to_string()).unwrap_or_else(|| "n/d".into());
                let n = self.replicas.get(d).copied().unwrap_or(0);
                format!("{d}: cpu_pct={cpu}, replicas={n}")
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn is_managed(&self, name: &str) -> bool {
        self.managed.iter().any(|s| s == name)
    }

    /// Escalonamento exige telemetria independente, CPU alta e aumento limitado
    /// ao dobro da capacidade observada. Sem CPU observada: negar.
    pub fn scale_justified(&self, name: &str, to: i64) -> bool {
        if !self.is_managed(name) {
            return false;
        }
        match (self.replicas.get(name), self.cpu_pct.get(name)) {
            (Some(&current), Some(&cpu)) => {
                (0..=100).contains(&cpu)
                    && cpu >= 85
                    && current > 0
                    && to > current
                    && to <= current.saturating_mul(2)
                    && to <= 64
            }
            _ => false,
        }
    }
}

/// Falha de uma chamada remota.
#[derive(Clone, Debug, PartialEq)]
pub enum HostError {
    /// O provedor respondeu com erro: o efeito não aconteceu.
    Failed(String),
    /// O prazo expirou sem resposta: o efeito pode ou não ter acontecido.
    Timeout { elapsed: f64 },
}

/// Resposta do protocolo de consulta de status (`provider queryable`).
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Done(Value),
    NotApplied,
    Unknown,
}

pub trait Host {
    /// Executa a ferramenta; devolve o valor e a latência observada em segundos.
    fn call(&mut self, tool: &str, args: &[Value]) -> Result<(Value, f64), HostError>;
    /// Consulta o estado da operação identificada pela chave (provedores `queryable`).
    fn status(&mut self, _tool: &str, _key: u64) -> (Status, f64) {
        (Status::Unknown, 0.0)
    }
    fn summary(&self) -> Vec<String>;
    /// Estado observável do mundo, em JSON (para relatórios e benchmarks).
    fn state(&self) -> Json {
        Json::Null
    }
    /// Origem da autoridade usada tanto no prompt quanto na política.
    fn trusted_snapshot(&self) -> TrustedSnapshot {
        TrustedSnapshot::default()
    }
}

/// Falha injetada: na `nth`-ésima chamada a `tool` (ou em todas, sem `nth`).
#[derive(Clone, Debug)]
pub struct Fault {
    pub tool: String,
    pub nth: Option<u32>,
    pub kind: FaultKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FaultKind {
    /// Timeout depois de aplicar o efeito.
    TimeoutApplied,
    /// Timeout sem aplicar o efeito.
    TimeoutLost,
    /// Erro do provedor.
    Error,
    /// As próximas `n` consultas de status desta ferramenta respondem "desconhecido".
    UnknownStatus(u32),
}

#[derive(Default)]
pub struct SimWorld {
    latency: HashMap<String, f64>,
    logs: HashMap<String, String>,
    pub replicas: BTreeMap<String, i64>,
    managed: Vec<String>,
    cpu_pct: BTreeMap<String, i64>,
    initial: BTreeMap<String, i64>,
    pub health: BTreeMap<String, bool>,
    pub restarts: BTreeMap<String, u32>,
    pub outbox: Vec<String>,
    faults: Vec<Fault>,
    calls: HashMap<String, u32>,
    unknown_status: HashMap<String, u32>,
    /// (ferramenta, chave) → recibo. Repetir a chave não repete o efeito.
    applied: HashMap<(String, u64), Value>,
    /// chave de um `scale` → (deployment, réplicas antes): a pré-imagem que a compensação restaura.
    preimage: HashMap<u64, (String, i64)>,
    /// Ferramentas sumidouro: aceitam qualquer argumento, devolvem `()` e só registram a chamada.
    /// Servem aos programas do núcleo gerados pelo teste diferencial triplo.
    sinks: Vec<String>,
    /// (ferramenta, argumentos) de cada chamada a um sumidouro, em ordem.
    pub sunk: Vec<(String, Vec<Value>)>,
    inventories: BTreeMap<String, Vec<String>>,
    /// O mundo de células do núcleo (`World := Nat → Int` no Lean): o valor de cada célula.
    pub cells: BTreeMap<u64, i64>,
    /// Ferramentas de célula: somam `sign · v` à célula; uma compensação pode recusar.
    cell_tools: HashMap<String, CellTool>,
    /// Sondas de célula: `lo ≤ célula ≤ hi`, sem argumento e sem efeito.
    probes: HashMap<String, CellProbe>,
}

#[derive(Clone, Debug)]
struct CellTool {
    cell: u64,
    sign: i64,
    fails: bool,
}

#[derive(Clone, Debug)]
struct CellProbe {
    cell: u64,
    lo: i64,
    hi: i64,
}

fn key_of(args: &[Value]) -> Option<u64> {
    args.iter().find_map(|v| if let Value::Key(k) = v { Some(*k) } else { None })
}

fn text_arg(args: &[Value]) -> Option<String> {
    args.iter().find_map(|v| if let Value::Text(s) = v { Some(s.clone()) } else { None })
}

fn int_arg(args: &[Value]) -> Option<i64> {
    args.iter().find_map(|v| if let Value::Int(n) = v { Some(*n) } else { None })
}

/// Deployment e alvo de uma ação: argumentos soltos ou um ADT `Restart`/`Scale`.
fn target(args: &[Value]) -> (Option<String>, Option<i64>, Option<String>) {
    if let Some(Value::Adt { variant, fields, .. }) = args.iter().find(|v| matches!(v, Value::Adt { .. })) {
        let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let d = match get("deploy") {
            Some(Value::Text(d)) => Some(d),
            _ => None,
        };
        let to = match get("to") {
            Some(Value::Int(n)) => Some(n),
            _ => None,
        };
        return (d, to, Some(variant.clone()));
    }
    (text_arg(args), int_arg(args), None)
}

impl SimWorld {
    pub fn from_json(j: &Json) -> Result<Self, String> {
        let mut w = SimWorld::default();
        if let Some(m) = j.get("latency").and_then(|v| v.as_object()) {
            for (k, v) in m {
                w.latency.insert(k.clone(), v.as_f64().ok_or("latência deve ser número")?);
            }
        }
        if let Some(m) = j.get("logs").and_then(|v| v.as_object()) {
            for (k, v) in m {
                let text = match v {
                    Json::String(s) => s.clone(),
                    Json::Array(lines) => lines.iter().filter_map(|l| l.as_str()).collect::<Vec<_>>().join("\n"),
                    _ => return Err("logs devem ser texto ou lista de linhas".into()),
                };
                w.logs.insert(k.clone(), text);
            }
        }
        if let Some(m) = j.get("replicas").and_then(|v| v.as_object()) {
            for (k, v) in m {
                w.replicas.insert(k.clone(), v.as_i64().ok_or("réplicas devem ser inteiros")?);
            }
        }
        // Somente campo explícito e confiável de configuração fornece o inventário.
        // Não inferir autorização de replicas nem do conteúdo de logs.
        if let Some(names) = j.get("managed") {
            let arr = names.as_array().ok_or("`managed` deve ser uma lista de nomes")?;
            let mut managed = Vec::new();
            for value in arr {
                let name = value.as_str().ok_or("nome de `managed` não textual")?;
                if name.is_empty() || name.chars().count() > 32 {
                    return Err("nome de `managed` vazio ou longo demais".into());
                }
                if !w.replicas.contains_key(name) {
                    return Err(format!("deployment gerenciado `{name}` não existe no estado"));
                }
                managed.push(name.to_string());
            }
            managed.sort();
            managed.dedup();
            if managed.join(", ").chars().count() > 256 {
                return Err("inventário ultrapassa o limite de 256 caracteres".into());
            }
            w.managed = managed;
        }
        if let Some(cpu) = j.get("telemetry").and_then(|v| v.get("cpu_pct")) {
            let obj = cpu.as_object().ok_or("`telemetry.cpu_pct` deve ser um objeto")?;
            for (name, val) in obj {
                let pct = val.as_i64().ok_or("cpu_pct deve ser inteiro")?;
                if !(0..=100).contains(&pct) {
                    return Err(format!("cpu_pct inválido para `{name}`"));
                }
                w.cpu_pct.insert(name.clone(), pct);
            }
        }
        w.initial = w.replicas.clone();
        if let Some(m) = j.get("inventories") {
            let obj = m.as_object().ok_or("`inventories` deve ser um objeto de listas")?;
            for (k, v) in obj {
                let mut items = Vec::new();
                for x in v.as_array().ok_or("inventário deve ser uma lista")? {
                    items.push(match x {
                        Json::String(s) => s.clone(),
                        Json::Number(n) => n.to_string(),
                        _ => return Err(format!("item não textual no inventário `{k}`")),
                    });
                }
                items.sort();
                items.dedup();
                w.inventories.insert(k.clone(), items);
            }
        }
        if let Some(list) = j.get("sinks") {
            let arr = list.as_array().ok_or("`sinks` deve ser uma lista de nomes de ferramenta")?;
            for v in arr {
                w.sinks.push(v.as_str().ok_or("nome de sumidouro não textual")?.to_string());
            }
        }
        if let Some(c) = j.get("cells") {
            let cell = |v: &Json| v.get("cell").and_then(|x| x.as_u64()).ok_or("célula sem índice");
            for (k, v) in c.get("tools").and_then(|x| x.as_object()).into_iter().flatten() {
                let sign = v.get("sign").and_then(|x| x.as_i64()).unwrap_or(1);
                let fails = v.get("fails").and_then(|x| x.as_bool()).unwrap_or(false);
                w.cell_tools.insert(k.clone(), CellTool { cell: cell(v)?, sign, fails });
            }
            for (k, v) in c.get("probes").and_then(|x| x.as_object()).into_iter().flatten() {
                let lo = v.get("lo").and_then(|x| x.as_i64()).ok_or("sonda sem `lo`")?;
                let hi = v.get("hi").and_then(|x| x.as_i64()).ok_or("sonda sem `hi`")?;
                w.probes.insert(k.clone(), CellProbe { cell: cell(v)?, lo, hi });
            }
        }
        if let Some(m) = j.get("health").and_then(|v| v.as_object()) {
            for (k, v) in m {
                w.health.insert(k.clone(), v.as_bool().ok_or("saúde deve ser booleana")?);
            }
        }
        if let Some(list) = j.get("faults").and_then(|v| v.as_array()) {
            for f in list {
                let tool = f["tool"].as_str().ok_or("falha sem `tool`")?.to_string();
                let nth = f["nth"].as_u64().map(|n| n as u32);
                let kind = match f["kind"].as_str().ok_or("falha sem `kind`")? {
                    "timeout_applied" => FaultKind::TimeoutApplied,
                    "timeout_lost" => FaultKind::TimeoutLost,
                    "error" => FaultKind::Error,
                    "unknown_status" => FaultKind::UnknownStatus(f["times"].as_u64().unwrap_or(1) as u32),
                    other => return Err(format!("tipo de falha desconhecido `{other}`")),
                };
                if let FaultKind::UnknownStatus(n) = kind {
                    w.unknown_status.insert(tool.clone(), n);
                } else {
                    w.faults.push(Fault { tool, nth, kind });
                }
            }
        }
        Ok(w)
    }

    fn lat(&self, tool: &str) -> f64 {
        self.latency.get(tool).copied().unwrap_or(0.5)
    }

    fn fault(&self, tool: &str, n: u32) -> Option<FaultKind> {
        self.faults.iter().find(|f| f.tool == tool && f.nth.is_none_or(|k| k == n)).map(|f| f.kind.clone())
    }

    fn apply(&mut self, tool: &str, args: &[Value]) -> Result<Value, HostError> {
        let (deploy, to, variant) = target(args);
        let key = key_of(args);
        let need = |d: Option<String>| d.ok_or_else(|| HostError::Failed(format!("`{tool}` sem deployment")));
        Ok(match (tool, variant.as_deref()) {
            ("logs", _) => {
                let d = need(deploy)?;
                Value::Text(self.logs.get(&d).cloned().unwrap_or_else(|| format!("(sem logs para {d})")))
            }
            ("restart", _) | ("apply", Some("Restart")) => {
                let d = need(deploy)?;
                *self.restarts.entry(d.clone()).or_default() += 1;
                Value::Text(format!("restart {d}"))
            }
            ("scale", _) | ("apply", Some("Scale")) => {
                let d = need(deploy)?;
                let to = to.ok_or_else(|| HostError::Failed("scale sem réplicas".into()))?;
                let from = self.replicas.insert(d.clone(), to).unwrap_or(0);
                if let Some(k) = key {
                    self.preimage.insert(k, (d.clone(), from));
                }
                Value::Text(format!("scale {d} {from}→{to}"))
            }
            // Compensação de `scale`: restaura a pré-imagem registrada pela chave da operação original.
            // Total sobre "aconteceu" e "não aconteceu": sem pré-imagem, não há o que desfazer.
            ("unscale", _) => match key.and_then(|k| self.preimage.remove(&k)) {
                Some((d, before)) => {
                    let now = self.replicas.insert(d.clone(), before).unwrap_or(0);
                    Value::Text(format!("unscale {d} {now}→{before}"))
                }
                None => Value::Text("unscale: nada a restaurar".into()),
            },
            ("healthy", _) => Value::Bool(self.health.get(&need(deploy)?).copied().unwrap_or(true)),
            ("restored", _) => {
                let d = need(deploy)?;
                Value::Bool(self.replicas.get(&d) == self.initial.get(&d))
            }
            ("page", _) => {
                self.outbox.push(text_arg(args).unwrap_or_default());
                Value::Unit
            }
            (other, _) if self.cell_tools.contains_key(other) => {
                let ct = self.cell_tools[other].clone();
                if ct.fails {
                    return Err(HostError::Failed(format!("`{other}` recusada pelo provedor")));
                }
                *self.cells.entry(ct.cell).or_default() += ct.sign * int_arg(args).unwrap_or(0);
                Value::Unit
            }
            (other, _) if self.probes.contains_key(other) => {
                let p = &self.probes[other];
                let v = self.cells.get(&p.cell).copied().unwrap_or(0);
                Value::Bool(p.lo <= v && v <= p.hi)
            }
            (other, _) if self.sinks.iter().any(|s| s == other) => {
                self.sunk.push((other.to_string(), args.to_vec()));
                Value::Unit
            }
            (other, _) => return Err(HostError::Failed(format!("ferramenta `{other}` sem ligação no host"))),
        })
    }
}

impl Host for SimWorld {
    fn trusted_snapshot(&self) -> TrustedSnapshot {
        TrustedSnapshot {
            managed: self.managed.clone(),
            replicas: self.replicas.clone(),
            cpu_pct: self.cpu_pct.clone(),
            inventories: self.inventories.clone(),
        }
    }

    fn call(&mut self, tool: &str, args: &[Value]) -> Result<(Value, f64), HostError> {
        let lat = self.lat(tool);
        let key = key_of(args);
        if let Some(k) = key {
            if let Some(prev) = self.applied.get(&(tool.to_string(), k)) {
                return Ok((prev.clone(), lat));
            }
        }
        let n = {
            let c = self.calls.entry(tool.to_string()).or_default();
            *c += 1;
            *c
        };
        match self.fault(tool, n) {
            Some(FaultKind::Error) => return Err(HostError::Failed(format!("`{tool}` falhou no provedor"))),
            Some(FaultKind::TimeoutLost) => return Err(HostError::Timeout { elapsed: lat * 4.0 }),
            Some(FaultKind::TimeoutApplied) => {
                let out = self.apply(tool, args)?;
                if let Some(k) = key {
                    self.applied.insert((tool.to_string(), k), out);
                }
                return Err(HostError::Timeout { elapsed: lat * 4.0 });
            }
            _ => {}
        }
        let out = self.apply(tool, args)?;
        if let Some(k) = key {
            self.applied.insert((tool.to_string(), k), out.clone());
        }
        Ok((out, lat))
    }

    fn status(&mut self, tool: &str, key: u64) -> (Status, f64) {
        let lat = self.lat(tool);
        if let Some(n) = self.unknown_status.get_mut(tool) {
            if *n > 0 {
                *n -= 1;
                return (Status::Unknown, lat);
            }
        }
        match self.applied.get(&(tool.to_string(), key)) {
            Some(v) => (Status::Done(v.clone()), lat),
            None => (Status::NotApplied, lat),
        }
    }

    fn state(&self) -> Json {
        serde_json::json!({ "replicas": self.replicas, "restarts": self.restarts, "outbox": self.outbox })
    }

    fn summary(&self) -> Vec<String> {
        let mut s = Vec::new();
        for (d, n) in &self.restarts {
            s.push(format!("{d} reiniciado {n}×"));
        }
        for (d, n) in &self.replicas {
            s.push(format!("{d}: {n} réplicas"));
        }
        for m in &self.outbox {
            s.push(format!("pager ← {m:?}"));
        }
        s
    }
}
