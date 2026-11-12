//! Autenticidade (v1.0-RC2): assinaturas Ed25519 da **autoridade** que o programa declara.
//!
//! ```text
//! authority treasury { key = "ed25519:<chave pública, 64 hex>" }
//! ```
//!
//! Um módulo que declara uma autoridade só aceita, como fonte de autoridade, o que ela assinou:
//!
//! * cada **concessão** (`s.authorizes`): a assinatura cobre a identidade da concessão, que é o
//!   BLAKE3 de execução, operação, argumentos, origem e nonce;
//! * a **fotografia confiável** (`s.member`, `s.inventory`, …): a assinatura cobre o BLAKE3 dos
//!   bytes do arquivo;
//! * os **argumentos do evento** rotulados `Trusted`: a assinatura cobre a execução e os
//!   argumentos, de modo que um pedido assinado não vale em outra execução.
//!
//! A chave pública faz parte do programa e, portanto, do seu hash (Codex): o WAL de uma execução
//! só é recuperado pelo programa com a mesma autoridade. A chave privada nunca chega ao runtime.
//!
//! A mensagem assinada é `aletheia/v1/<tipo>\n<carga>`: o tipo separa os domínios, e uma
//! assinatura de concessão não serve de assinatura de fotografia.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

pub const PREFIX: &str = "ed25519:";

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn message(kind: &str, payload: &str) -> Vec<u8> {
    format!("aletheia/v1/{kind}\n{payload}").into_bytes()
}

/// `ed25519:<64 hex>` → a chave pública.
pub fn parse_public(s: &str) -> Result<[u8; 32], String> {
    let h = s.strip_prefix(PREFIX).ok_or("a chave de uma autoridade começa com `ed25519:`")?;
    let b = unhex(h).filter(|b| b.len() == 32).ok_or("a chave pública tem 64 dígitos hexadecimais")?;
    let k: [u8; 32] = b.try_into().map_err(|_| "chave pública inválida")?;
    VerifyingKey::from_bytes(&k).map_err(|_| "a chave pública não é um ponto válido da curva")?;
    Ok(k)
}

/// A assinatura `sig` (`ed25519:<128 hex>`) de `public` sobre a carga do tipo `kind` confere.
pub fn verify(public: &[u8; 32], kind: &str, payload: &str, sig: &str) -> bool {
    let Some(b) = sig.strip_prefix(PREFIX).and_then(unhex).filter(|b| b.len() == 64) else { return false };
    let (Ok(vk), Ok(sb)) = (VerifyingKey::from_bytes(public), <[u8; 64]>::try_from(b)) else { return false };
    vk.verify_strict(&message(kind, payload), &Signature::from_bytes(&sb)).is_ok()
}

/// A chave privada lida de um arquivo de `athc keygen` (64 hex).
pub fn parse_secret(text: &str) -> Result<[u8; 32], String> {
    unhex(text.trim())
        .filter(|b| b.len() == 32)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| "a chave privada tem 64 dígitos hexadecimais".to_string())
}

pub fn public_of(secret: &[u8; 32]) -> String {
    format!("{PREFIX}{}", hex(SigningKey::from_bytes(secret).verifying_key().as_bytes()))
}

pub fn sign(secret: &[u8; 32], kind: &str, payload: &str) -> String {
    format!("{PREFIX}{}", hex(&SigningKey::from_bytes(secret).sign(&message(kind, payload)).to_bytes()))
}

/// A carga da fotografia confiável: o BLAKE3 dos bytes do arquivo.
pub fn trusted_payload(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// A carga dos argumentos do evento: a execução e o BLAKE3 dos argumentos como foram escritos.
pub fn args_payload(run: &str, raw: &[String]) -> String {
    format!("{run}\n{}", blake3::hash(raw.join("\u{1f}").as_bytes()).to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 8032, §7.1, vetor de teste 1: um par público, não uma credencial.
    const SECRET: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const PUBLIC: &str = "ed25519:d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

    #[test]
    fn a_signature_binds_the_key_the_kind_and_the_payload() {
        let sk = parse_secret(SECRET).unwrap();
        assert_eq!(public_of(&sk), PUBLIC);
        let pk = parse_public(PUBLIC).unwrap();
        let s = sign(&sk, "grant", "abc");
        assert!(verify(&pk, "grant", "abc", &s));
        assert!(!verify(&pk, "grant", "abd", &s), "outra carga");
        assert!(!verify(&pk, "trusted", "abc", &s), "outro domínio");
        let other = parse_secret(&"11".repeat(32)).unwrap();
        assert!(!verify(&parse_public(&public_of(&other)).unwrap(), "grant", "abc", &s), "outra chave");
        assert!(!verify(&pk, "grant", "abc", "ed25519:00"));
        assert!(!verify(&pk, "grant", "abc", &s.replace(PREFIX, "")));
        assert!(parse_public("ed25519:zz").is_err() && parse_public(&PUBLIC[8..]).is_err());
    }
}

// ------------------------------------------------------------------ v1.0-RC3: o que se assina tem contexto e prazo

/// **O carimbo** de uma assinatura com vínculos: para que programa (raiz do Codex), para que
/// execução, e em que intervalo (segundos Unix) ela vale. A assinatura cobre a carga **e** o
/// carimbo: mudar qualquer campo a invalida.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub program: String,
    pub run: String,
    pub issued: u64,
    pub expires: u64,
    pub sig: String,
}

/// A carga com o carimbo: o que a autoridade de fato assina (tipos `grant2`, `trusted2`, `args2`).
pub fn stamped(base: &str, program: &str, run: &str, issued: u64, expires: u64) -> String {
    format!("{base}\n{program}\n{run}\n{issued}\n{expires}")
}

impl Stamp {
    pub fn sign(
        secret: &[u8; 32],
        kind: &str,
        base: &str,
        program: &str,
        run: &str,
        issued: u64,
        expires: u64,
    ) -> Stamp {
        Stamp {
            program: program.into(),
            run: run.into(),
            issued,
            expires,
            sig: sign(secret, kind, &stamped(base, program, run, issued, expires)),
        }
    }

    pub fn verify(&self, public: &[u8; 32], kind: &str, base: &str) -> bool {
        verify(public, kind, &stamped(base, &self.program, &self.run, self.issued, self.expires), &self.sig)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "v": 2, "program": self.program, "run": self.run, "issued": self.issued,
                            "expires": self.expires, "sig": self.sig })
    }

    pub fn from_json(j: &serde_json::Value) -> Option<Stamp> {
        Some(Stamp {
            program: j["program"].as_str()?.to_string(),
            run: j["run"].as_str()?.to_string(),
            issued: j["issued"].as_u64()?,
            expires: j["expires"].as_u64()?,
            sig: j["sig"].as_str()?.to_string(),
        })
    }

    /// Uma atestação apresentada como texto: o JSON de um carimbo, ou (formato do RC2) só a
    /// assinatura `ed25519:…`.
    pub fn parse(text: &str) -> (Option<String>, Option<Stamp>) {
        let t = text.trim();
        match serde_json::from_str::<serde_json::Value>(t) {
            Ok(j) if j.is_object() => (None, Stamp::from_json(&j)),
            _ => (Some(t.to_string()), None),
        }
    }
}

#[cfg(test)]
mod stamp_tests {
    use super::*;

    #[test]
    fn a_stamp_binds_program_run_and_validity() {
        let sk = parse_secret("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60").unwrap();
        let pk = parse_public(&public_of(&sk)).unwrap();
        let s = Stamp::sign(&sk, "trusted2", "digest", "prog", "r1", 100, 200);
        assert!(s.verify(&pk, "trusted2", "digest"));
        for forged in [
            Stamp { program: "outro".into(), ..s.clone() },
            Stamp { run: "r2".into(), ..s.clone() },
            Stamp { issued: 0, ..s.clone() },
            Stamp { expires: 9_999, ..s.clone() },
        ] {
            assert!(!forged.verify(&pk, "trusted2", "digest"));
        }
        assert!(!s.verify(&pk, "args2", "digest") && !s.verify(&pk, "trusted2", "outro"));
        assert_eq!(Stamp::parse(&s.to_json().to_string()), (None, Some(s.clone())));
        assert_eq!(Stamp::parse("ed25519:ab\n"), (Some("ed25519:ab".into()), None));
    }
}
