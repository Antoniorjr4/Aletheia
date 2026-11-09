//! Autômato de prefixo viável e máscara de tokens.
//!
//! [`viable`] decide, em tempo linear no tamanho do prefixo, se ele ainda pode ser
//! completado numa codificação canônica válida da forma. A máscara de um passo de
//! decodificação é só isto: um token é permitido se `viable(prefixo + token)` não é
//! `Dead`. Como todo prefixo aceito é viável e toda forma tem ao menos um valor,
//! a decodificação mascarada sempre termina numa saída válida.

use crate::{Shape, VariantShape, ROOT_CLOSE, ROOT_OPEN};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viability {
    /// Nenhuma continuação torna o prefixo válido.
    Dead,
    /// Viável, mas ainda incompleto.
    Partial,
    /// Documento completo e válido (não admite continuação).
    Complete,
}

enum Step {
    Ok(usize),
    Eof,
    Bad,
}

fn then(r: Step, f: impl FnOnce(usize) -> Step) -> Step {
    match r {
        Step::Ok(i) => f(i),
        other => other,
    }
}

fn lit(s: &[char], i: usize, w: &str) -> Step {
    let mut j = i;
    for wc in w.chars() {
        if j == s.len() {
            return Step::Eof;
        }
        if s[j] != wc {
            return Step::Bad;
        }
        j += 1;
    }
    Step::Ok(j)
}

pub fn viable(shape: &Shape, prefix: &str) -> Viability {
    let s: Vec<char> = prefix.chars().collect();
    let r = then(lit(&s, 0, ROOT_OPEN), |i| then(value(shape, &s, i), |i| lit(&s, i, ROOT_CLOSE)));
    match r {
        Step::Ok(i) if i == s.len() => Viability::Complete,
        Step::Ok(_) | Step::Bad => Viability::Dead,
        Step::Eof => Viability::Partial,
    }
}

fn value(shape: &Shape, s: &[char], i: usize) -> Step {
    match shape {
        Shape::Bool => {
            let a = lit(s, i, "true");
            if let Step::Ok(_) = a {
                return a;
            }
            let b = lit(s, i, "false");
            match (a, b) {
                (_, Step::Ok(j)) => Step::Ok(j),
                (Step::Eof, _) | (_, Step::Eof) => Step::Eof,
                _ => Step::Bad,
            }
        }
        Shape::Int { min, max } => int(s, i, *min, *max),
        Shape::Text { max_chars } => text(s, i, *max_chars),
        Shape::Adt { variants, .. } => adt(variants, s, i),
        Shape::List { elem, min, max } => list(elem, *min, *max, s, i),
    }
}

/// `[e1,…,en]` com `min ≤ n ≤ max`: cada vírgula exige espaço sob o teto, e `]` exige o mínimo.
fn list(elem: &Shape, min: u32, max: u32, s: &[char], i: usize) -> Step {
    let mut j = match lit(s, i, "[") {
        Step::Ok(j) => j,
        other => return other,
    };
    if j == s.len() {
        return Step::Eof;
    }
    if s[j] == ']' {
        return if min == 0 { Step::Ok(j + 1) } else { Step::Bad };
    }
    if max == 0 {
        return Step::Bad;
    }
    let mut n = 0u32;
    loop {
        j = match value(elem, s, j) {
            Step::Ok(k) => k,
            other => return other,
        };
        n += 1;
        if j == s.len() {
            return Step::Eof;
        }
        match s[j] {
            ']' if n >= min => return Step::Ok(j + 1),
            ',' if n < max => j += 1,
            _ => return Step::Bad,
        }
    }
}

fn text(s: &[char], i: usize, max: Option<u32>) -> Step {
    let mut j = match lit(s, i, "\"") {
        Step::Ok(j) => j,
        other => return other,
    };
    let mut n = 0u32;
    loop {
        if j == s.len() {
            return Step::Eof;
        }
        let c = s[j];
        if c == '"' {
            return Step::Ok(j + 1);
        }
        if c == '\\' || (c as u32) < 0x20 {
            return Step::Bad;
        }
        n += 1;
        if max.is_some_and(|m| n > m) {
            return Step::Bad;
        }
        j += 1;
    }
}

fn int(s: &[char], i: usize, min: Option<i64>, max: Option<i64>) -> Step {
    let mut j = i;
    while j < s.len() && (s[j].is_ascii_digit() || (j == i && s[j] == '-')) {
        j += 1;
    }
    let run: String = s[i..j].iter().collect();
    if j == s.len() {
        return if int_prefix_viable(&run, min, max) { Step::Eof } else { Step::Bad };
    }
    if int_complete(&run, min, max) {
        Step::Ok(j)
    } else {
        Step::Bad
    }
}

fn split_sign(run: &str) -> (bool, &str) {
    match run.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, run),
    }
}

fn canonical_digits(neg: bool, d: &str) -> bool {
    !d.is_empty()
        && d.len() <= 19
        && d.chars().all(|c| c.is_ascii_digit())
        && !(d.len() > 1 && d.starts_with('0'))
        && !(neg && d == "0")
}

fn int_complete(run: &str, min: Option<i64>, max: Option<i64>) -> bool {
    let (neg, d) = split_sign(run);
    if !canonical_digits(neg, d) {
        return false;
    }
    let Ok(v) = d.parse::<i128>() else { return false };
    let v = if neg { -v } else { v };
    v >= min.unwrap_or(i64::MIN) as i128 && v <= max.unwrap_or(i64::MAX) as i128
}

/// Existe inteiro no intervalo cuja forma decimal canônica começa com `run`?
fn int_prefix_viable(run: &str, min: Option<i64>, max: Option<i64>) -> bool {
    let lo = min.unwrap_or(i64::MIN) as i128;
    let hi = max.unwrap_or(i64::MAX) as i128;
    if run.is_empty() {
        return lo <= hi;
    }
    let (neg, d) = split_sign(run);
    if d.is_empty() {
        return lo <= -1;
    }
    if !canonical_digits(neg, d) {
        return false;
    }
    let v: i128 = d.parse().unwrap_or(i128::MAX);
    if d == "0" {
        return lo <= 0 && 0 <= hi;
    }
    for k in 0..=(19 - d.len() as u32) {
        let p = 10i128.pow(k);
        let (a, b) = (v * p, v * p + p - 1);
        let (a, b) = if neg { (-b, -a) } else { (a, b) };
        if a <= hi && b >= lo {
            return true;
        }
    }
    false
}

fn adt(variants: &[VariantShape], s: &[char], i: usize) -> Step {
    let mut j = match lit(s, i, "{\"kind\":\"") {
        Step::Ok(j) => j,
        other => return other,
    };
    let start = j;
    while j < s.len() && s[j] != '"' {
        j += 1;
    }
    let name: String = s[start..j].iter().collect();
    if j == s.len() {
        return if variants.iter().any(|v| v.name.starts_with(&name)) { Step::Eof } else { Step::Bad };
    }
    let Some(v) = variants.iter().find(|v| v.name == name) else { return Step::Bad };
    j += 1;
    for (f, fs) in &v.fields {
        j = match lit(s, j, &format!(",\"{f}\":")) {
            Step::Ok(k) => k,
            other => return other,
        };
        j = match value(fs, s, j) {
            Step::Ok(k) => k,
            other => return other,
        };
    }
    lit(s, j, "}")
}

/// Alfabeto de caracteres do decodificador local: ASCII imprimível e letras acentuadas do português.
pub fn alphabet() -> Vec<char> {
    let mut a: Vec<char> = (0x20u8..=0x7e).map(|b| b as char).collect();
    a.extend("áàâãéêíóôõúçÁÀÂÃÉÊÍÓÔÕÚÇ".chars());
    a
}

/// Caracteres que mantêm o prefixo viável.
pub fn allowed(shape: &Shape, prefix: &str, alphabet: &[char]) -> Vec<char> {
    let mut buf = String::from(prefix);
    let base = buf.len();
    let mut out = Vec::new();
    for &c in alphabet {
        buf.truncate(base);
        buf.push(c);
        if viable(shape, &buf) != Viability::Dead {
            out.push(c);
        }
    }
    out
}

/// Máscara de um passo sobre um vocabulário de tokens (como um processador de logits):
/// `true` onde o token mantém o prefixo viável.
pub fn mask(shape: &Shape, prefix: &str, vocab: &[&str]) -> Vec<bool> {
    let mut buf = String::from(prefix);
    let base = buf.len();
    vocab
        .iter()
        .map(|t| {
            buf.truncate(base);
            buf.push_str(t);
            !t.is_empty() && viable(shape, &buf) != Viability::Dead
        })
        .collect()
}

/// Primeiro ponto em que a máscara impediu o que o modelo queria emitir.
#[derive(Clone, Debug, PartialEq)]
pub struct Deviation {
    pub pos: usize,
    pub wanted: Option<char>,
    pub allowed: Vec<char>,
    pub prefix: String,
}

#[derive(Clone, Debug)]
pub struct Trace {
    pub text: String,
    pub deviation: Option<Deviation>,
}

/// Decodificação mascarada que segue uma proposta (a saída "desejada" do modelo)
/// enquanto ela for viável. No primeiro caractere proibido, a proposta é abandonada e
/// o documento é fechado pelo caminho válido mais curto (fecha textos, objetos,
/// escolhe dígitos do intervalo). O resultado é sempre um documento completo e válido.
pub fn decode_following(shape: &Shape, proposal: &str, limit: usize) -> Result<Trace, String> {
    let alpha = alphabet();
    let prop: Vec<char> = proposal.chars().collect();
    let mut out = String::new();
    let mut k = 0;
    let mut deviation: Option<Deviation> = None;
    for pos in 0..limit {
        if viable(shape, &out) == Viability::Complete {
            return Ok(Trace { text: out, deviation });
        }
        let ok = allowed(shape, &out, &alpha);
        if ok.is_empty() {
            return Err(format!("gramática sem continuação após {out:?}"));
        }
        let c = if deviation.is_none() && k < prop.len() && ok.contains(&prop[k]) {
            k += 1;
            prop[k - 1]
        } else {
            if deviation.is_none() {
                let mut shown = ok.clone();
                shown.truncate(12);
                deviation = Some(Deviation { pos, wanted: prop.get(k).copied(), allowed: shown, prefix: out.clone() });
            }
            fallback(&ok)
        };
        out.push(c);
    }
    Err(format!("limite de {limit} caracteres atingido sem completar a saída"))
}

fn fallback(ok: &[char]) -> char {
    // `]` antes de `,`: fechar a lista assim que o mínimo de elementos permite.
    for p in "\"}],:0123456789".chars() {
        if ok.contains(&p) {
            return p;
        }
    }
    *ok.iter().min().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::action_shape;
    use crate::{check, max_bytes};

    fn valid(shape: &Shape, s: &str) -> bool {
        serde_json::from_str(s).ok().is_some_and(|j| check(shape, &j).is_ok())
    }

    #[test]
    fn prefixes_of_valid_documents_are_viable() {
        let s = action_shape();
        let doc = r#"{"value":{"kind":"Scale","deploy":"api","to":12}}"#;
        for (i, _) in doc.char_indices() {
            assert_ne!(viable(&s, &doc[..i]), Viability::Dead, "{}", &doc[..i]);
        }
        assert_eq!(viable(&s, doc), Viability::Complete);
    }

    #[test]
    fn the_type_has_no_room_for_a_delete() {
        let s = action_shape();
        assert_eq!(viable(&s, r#"{"value":{"kind":"D"#), Viability::Dead);
        let next = allowed(&s, r#"{"value":{"kind":""#, &alphabet());
        assert_eq!(next, vec!['R', 'S']);
    }

    #[test]
    fn integer_ranges_are_enforced_digit_by_digit() {
        let s = action_shape();
        let p = r#"{"value":{"kind":"Scale","deploy":"api","to":"#;
        let first = allowed(&s, p, &alphabet());
        assert_eq!(first, "123456789".chars().collect::<Vec<_>>(), "0 e negativos estão fora de 2..64");
        assert_eq!(viable(&s, &format!("{p}1}}}}")), Viability::Dead, "1 < 2");
        assert_eq!(viable(&s, &format!("{p}65")), Viability::Dead, "65 > 64");
        assert_eq!(viable(&s, &format!("{p}64}}}}")), Viability::Complete);
    }

    #[test]
    fn text_ceiling_is_enforced() {
        let s = action_shape();
        let p = format!(r#"{{"value":{{"kind":"Restart","deploy":"{}"#, "a".repeat(32));
        assert_eq!(allowed(&s, &p, &alphabet()), vec!['"']);
    }

    #[test]
    fn token_mask_over_a_vocabulary() {
        let s = action_shape();
        let vocab = ["Delete", "Restart", "Scale", "Rest", "\"", "rm -rf"];
        let m = mask(&s, r#"{"value":{"kind":""#, &vocab);
        assert_eq!(m, [false, true, true, true, false, false]);
    }

    #[test]
    fn injected_proposal_is_forced_back_into_the_type() {
        let s = action_shape();
        let t = decode_following(&s, r#"{"value":{"kind":"Delete","deploy":"db"}}"#, 400).unwrap();
        assert!(valid(&s, &t.text), "{}", t.text);
        let d = t.deviation.unwrap();
        assert_eq!(d.wanted, Some('D'));
        assert_eq!(d.allowed, vec!['R', 'S']);

        let t = decode_following(&s, r#"{"value":{"kind":"Scale","deploy":"payments","to":0}}"#, 400).unwrap();
        assert_eq!(t.text, r#"{"value":{"kind":"Scale","deploy":"payments","to":10}}"#);
    }

    #[test]
    fn valid_proposals_pass_through_untouched() {
        let s = action_shape();
        let p = r#"{"value":{"kind":"Restart","deploy":"api"}}"#;
        let t = decode_following(&s, p, 400).unwrap();
        assert_eq!(t.text, p);
        assert!(t.deviation.is_none());
    }

    #[test]
    fn list_lengths_are_enforced_by_the_automaton() {
        let s = crate::tests::plan_shape();
        let one = r#"{"kind":"Restart","deploy":"a"}"#;
        assert_eq!(viable(&s, r#"{"value":[]"#), Viability::Dead, "o mínimo é 1");
        assert_eq!(viable(&s, &format!(r#"{{"value":[{one}]}}"#)), Viability::Complete);
        assert_eq!(viable(&s, &format!(r#"{{"value":[{one},{one},{one}]}}"#)), Viability::Complete, "teto exato");
        assert_eq!(viable(&s, &format!(r#"{{"value":[{one},{one},{one},"#)), Viability::Dead, "acima do teto");
        let t = decode_following(&s, &format!(r#"{{"value":[{one},{one},{one},{one}]}}"#), 2000).unwrap();
        assert_eq!(t.text, format!(r#"{{"value":[{one},{one},{one}]}}"#));
        let t = decode_following(&s, r#"{"value":[]}"#, 2000).unwrap();
        assert!(check(&s, &serde_json::from_str(&t.text).unwrap()).is_ok(), "{}", t.text);
    }

    /// Propriedade: qualquer proposta — inclusive lixo adversarial — vira uma saída válida
    /// que cabe no teto de bytes derivado do tipo.
    #[test]
    fn any_proposal_yields_a_valid_bounded_output() {
        let s = action_shape();
        let alpha = alphabet();
        let mut seed: u64 = 0x5eed_a1e7_4e1a_0001;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let fragments = [
            r#"{"value":{"kind":""#,
            "Scale",
            "Restart",
            "Delete",
            r#"","deploy":""#,
            r#"","to":"#,
            "IGNORE",
            "}}",
            "-",
            "999",
        ];
        for _ in 0..300 {
            let mut p = String::new();
            for _ in 0..(next() % 12) {
                if next() % 2 == 0 {
                    p.push_str(fragments[(next() % fragments.len() as u64) as usize]);
                } else {
                    p.push(alpha[(next() % alpha.len() as u64) as usize]);
                }
            }
            let t = decode_following(&s, &p, 400).unwrap();
            assert!(valid(&s, &t.text), "proposta {p:?} → {}", t.text);
            assert!(t.text.len() as u64 <= max_bytes(&s).unwrap());
        }
    }
}
