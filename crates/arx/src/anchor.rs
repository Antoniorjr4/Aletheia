//! A **âncora** do registro de consumo (v1.0-RC3): um estado monotônico fora do disco do executor.
//!
//! Uma cadeia de hashes detecta alteração e perda **no meio** do registro; não detecta o
//! retrocesso integral (um prefixo íntegro é uma cadeia íntegra). Para isso é preciso um ponto
//! fixo que quem escreve no disco do executor não consiga fazer voltar. A âncora guarda, por
//! escopo (programa e execução), a **marca** do registro: quantos consumos existem e o hash do
//! último. Ela só sabe fazer duas coisas:
//!
//! * `read`: a marca atual;
//! * `advance(de, para)`: troca a marca **se** a atual é `de` (compare-and-swap). Nunca volta.
//!
//! O protocolo (`arx::grant`): o consumo é gravado no registro, depois a âncora avança, e só
//! então o efeito é enviado. Logo, **todo efeito enviado tem o seu consumo ancorado**, e um
//! registro que não chega à marca da âncora foi retrocedido (`L0002`); um que não passa por
//! ela foi bifurcado (`L0003`). O registro pode estar **um** consumo à frente da âncora (queda
//! entre a gravação e o avanço): esse consumo ainda não enviou nada, e a âncora é adiantada.
//!
//! **Modelo de confiança.** O adversário escreve no disco do executor (registro, WAL,
//! concessões, cópias de segurança restauradas). Não controla a âncora, nem a chave privada da
//! autoridade. A âncora é confiada para ser monotônica e durável; não é confiada para conceder:
//! ela não vê concessões, só hashes.

use serde_json::{json, Value as Json};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// A marca do registro: o número de consumos e o hash do último (`ZERO` para o registro vazio).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub seq: u64,
    pub head: String,
}

impl Mark {
    pub fn zero() -> Mark {
        Mark { seq: 0, head: ZERO.to_string() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorError {
    /// a âncora não respondeu: sem ela não se consome (falha fechado)
    Unreachable(String),
    /// a marca atual não é a esperada: outro executor avançou, ou o registro local não é o real
    Conflict(Mark),
}

impl std::fmt::Display for AnchorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnchorError::Unreachable(m) => write!(f, "a âncora não respondeu: {m}"),
            AnchorError::Conflict(m) => {
                write!(f, "a âncora está em {} consumo(s), cabeça #{}", m.seq, &m.head[..12.min(m.head.len())])
            }
        }
    }
}

pub trait Anchor: std::fmt::Debug {
    fn read(&mut self, scope: &str) -> Result<Mark, AnchorError>;
    /// Avança de `from` para `to`. Repetir um avanço já feito (resposta perdida) é sucesso.
    fn advance(&mut self, scope: &str, from: &Mark, to: &Mark) -> Result<(), AnchorError>;
}

/// O escopo de uma âncora: o programa (Codex) e a execução a que as concessões estão ligadas.
pub fn scope(program: &str, run: &str) -> String {
    blake3::hash(format!("aletheia/v1/anchor\n{program}\n{run}").as_bytes()).to_hex().to_string()
}

/// A âncora em memória: para testes de unidade e para o modelo (`Aletheia/Anchor.lean`).
#[derive(Debug, Default, Clone)]
pub struct MemAnchor {
    pub marks: std::collections::BTreeMap<String, Mark>,
    /// `Some(n)`: as próximas `n` chamadas falham como inalcançável
    pub down: u32,
}

impl Anchor for MemAnchor {
    fn read(&mut self, scope: &str) -> Result<Mark, AnchorError> {
        if self.down > 0 {
            self.down -= 1;
            return Err(AnchorError::Unreachable("fora do ar".into()));
        }
        Ok(self.marks.get(scope).cloned().unwrap_or_else(Mark::zero))
    }

    fn advance(&mut self, scope: &str, from: &Mark, to: &Mark) -> Result<(), AnchorError> {
        let cur = self.read(scope)?;
        if &cur == to {
            return Ok(());
        }
        if &cur != from || to.seq != from.seq + 1 {
            return Err(AnchorError::Conflict(cur));
        }
        self.marks.insert(scope.to_string(), to.clone());
        Ok(())
    }
}

/// A âncora num serviço (`ath-world serve`, operação `anchor`): uma linha JSON por conexão.
#[derive(Debug)]
pub struct TcpAnchor {
    addr: SocketAddr,
    timeout: Duration,
}

impl TcpAnchor {
    pub fn new(addr: &str, timeout: f64) -> Result<TcpAnchor, String> {
        let addr = addr
            .to_socket_addrs()
            .map_err(|e| format!("âncora `{addr}`: {e}"))?
            .next()
            .ok_or_else(|| format!("âncora `{addr}`: endereço sem destino"))?;
        Ok(TcpAnchor { addr, timeout: Duration::from_secs_f64(timeout.max(0.05)) })
    }

    fn call(&self, req: Json) -> Result<Json, AnchorError> {
        let un = |e: std::io::Error| AnchorError::Unreachable(e.to_string());
        let mut s = TcpStream::connect_timeout(&self.addr, self.timeout).map_err(un)?;
        s.set_read_timeout(Some(self.timeout)).map_err(un)?;
        s.set_write_timeout(Some(self.timeout)).map_err(un)?;
        s.write_all(format!("{req}\n").as_bytes()).map_err(un)?;
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).map_err(un)?;
        let j: Json =
            serde_json::from_str(&line).map_err(|e| AnchorError::Unreachable(format!("resposta ilegível: {e}")))?;
        if j["ok"] == true || j["conflict"] == true {
            Ok(j)
        } else {
            Err(AnchorError::Unreachable(j["error"].as_str().unwrap_or("resposta sem `ok`").to_string()))
        }
    }
}

fn mark_of(j: &Json) -> Result<Mark, AnchorError> {
    match (j["seq"].as_u64(), j["head"].as_str()) {
        (Some(seq), Some(head)) => Ok(Mark { seq, head: head.to_string() }),
        _ => Err(AnchorError::Unreachable("resposta sem marca".into())),
    }
}

impl Anchor for TcpAnchor {
    fn read(&mut self, scope: &str) -> Result<Mark, AnchorError> {
        mark_of(&self.call(json!({ "op": "anchor", "do": "read", "scope": scope }))?)
    }

    fn advance(&mut self, scope: &str, from: &Mark, to: &Mark) -> Result<(), AnchorError> {
        let j = self.call(json!({ "op": "anchor", "do": "advance", "scope": scope,
                                  "from": { "seq": from.seq, "head": from.head },
                                  "to": { "seq": to.seq, "head": to.head } }))?;
        if j["conflict"] == true {
            return Err(AnchorError::Conflict(mark_of(&j)?));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_anchor_only_moves_forward_one_step_from_the_expected_mark() {
        let mut a = MemAnchor::default();
        let (m1, m2) = (Mark { seq: 1, head: "a".into() }, Mark { seq: 2, head: "b".into() });
        assert_eq!(a.read("s").unwrap(), Mark::zero());
        a.advance("s", &Mark::zero(), &m1).unwrap();
        // repetir é sucesso; voltar, pular ou partir de outra marca é conflito
        a.advance("s", &Mark::zero(), &m1).unwrap();
        assert_eq!(a.advance("s", &m1, &Mark::zero()), Err(AnchorError::Conflict(m1.clone())));
        assert_eq!(a.advance("s", &Mark::zero(), &m2), Err(AnchorError::Conflict(m1.clone())));
        assert_eq!(a.advance("s", &m1, &Mark { seq: 3, head: "c".into() }), Err(AnchorError::Conflict(m1.clone())));
        a.advance("s", &m1, &m2).unwrap();
        assert_eq!(a.read("s").unwrap(), m2);
        assert_eq!(a.read("outro").unwrap(), Mark::zero());
        assert_ne!(scope("p", "r1"), scope("p", "r2"));
        assert_ne!(scope("p1", "r"), scope("p2", "r"));
    }
}
