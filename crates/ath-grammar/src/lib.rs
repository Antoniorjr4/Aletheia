//! Decodificação restrita derivada de tipos.
//!
//! O tipo de saída de um `infer` é compilado em uma [`Shape`]. Dela saem, sem
//! nenhuma anotação extra do programador:
//!
//! - a **codificação canônica** da saída: `{"value": V}`, com ADTs como objetos
//!   `{"kind":"Variante", campo: …}` em ordem fixa;
//! - um **JSON Schema** estrito, para provedores com saída estruturada
//!   (`response_format` da API OpenAI, vLLM, llama.cpp);
//! - uma **gramática GBNF**, para o `grammar` do llama.cpp;
//! - um **autômato de prefixo** ([`viable`]) que decide, para qualquer prefixo,
//!   se ele ainda pode ser completado numa saída válida. A máscara de tokens
//!   ([`mask`]) e o decodificador local ([`decode_following`]) são construídos
//!   sobre ele: a saída é válida **por construção**, qualquer que seja o modelo;
//! - um **teto de bytes** ([`max_bytes`]) que limita os tokens de saída no orçamento.

pub mod gbnf;
pub mod mask;
pub mod schema;

pub use gbnf::to_gbnf;
pub use mask::{allowed, decode_following, mask, viable, Deviation, Trace, Viability};
pub use schema::{to_json_schema, to_json_schema_with, unenforced, Dialect};

use ath_syntax::ast::{Item, Module, TypeExpr};
use serde_json::Value as Json;

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Int { min: Option<i64>, max: Option<i64> },
    Bool,
    Text { max_chars: Option<u32> },
    Adt { name: String, variants: Vec<VariantShape> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct VariantShape {
    pub name: String,
    pub fields: Vec<(String, Shape)>,
}

/// Compila um tipo de saída numa forma. Falha para tipos que não podem sair de um modelo
/// (capacidades, chaves, `Unit`) e para tipos recursivos.
pub fn shape_of(m: &Module, ty: &TypeExpr) -> Result<Shape, String> {
    shape_rec(m, ty, &mut Vec::new())
}

fn shape_rec(m: &Module, ty: &TypeExpr, stack: &mut Vec<String>) -> Result<Shape, String> {
    match ty {
        TypeExpr::Labeled(inner, ..) | TypeExpr::Own(inner, _) => shape_rec(m, inner, stack),
        TypeExpr::Cap(..) => Err("capacidades não podem ser produzidas por um modelo".into()),
        TypeExpr::Refined { base, lo, hi, .. } => match base.name.as_str() {
            "Int" => Ok(Shape::Int { min: *lo, max: *hi }),
            "Text" => Ok(Shape::Text { max_chars: hi.map(|h| h as u32) }),
            other => Err(format!("refinamento não suportado em `{other}`")),
        },
        TypeExpr::Named(id) => match id.name.as_str() {
            "Int" => Ok(Shape::Int { min: None, max: None }),
            "Bool" => Ok(Shape::Bool),
            "Text" => Ok(Shape::Text { max_chars: None }),
            "Unit" | "IdemKey" | "Pending" => Err(format!("`{}` não pode ser produzido por um modelo", id.name)),
            name => {
                let td = m
                    .items
                    .iter()
                    .find_map(|i| match i {
                        Item::Type(t) if t.name.name == name => Some(t),
                        _ => None,
                    })
                    .ok_or_else(|| format!("tipo desconhecido `{name}`"))?;
                if stack.iter().any(|s| s == name) {
                    return Err(format!("tipo recursivo `{name}` não tem gramática finita no Marco Zero"));
                }
                stack.push(name.to_string());
                let mut variants = Vec::new();
                for v in &td.variants {
                    let mut fields = Vec::new();
                    for f in &v.fields {
                        fields.push((f.name.name.clone(), shape_rec(m, &f.ty, stack)?));
                    }
                    variants.push(VariantShape { name: v.name.name.clone(), fields });
                }
                stack.pop();
                Ok(Shape::Adt { name: name.to_string(), variants })
            }
        },
    }
}

const ROOT_OPEN: &str = "{\"value\":";
const ROOT_CLOSE: &str = "}";

fn int_digits(n: i64) -> u64 {
    n.to_string().len() as u64
}

/// Maior número de bytes UTF-8 da codificação canônica de qualquer valor da forma
/// (com o envelope `{"value": …}`). Como todo token de um tokenizer BPE cobre ao menos
/// um byte, este número é um teto seguro de tokens de saída.
pub fn max_bytes(shape: &Shape) -> Option<u64> {
    Some(ROOT_OPEN.len() as u64 + value_bytes(shape)? + ROOT_CLOSE.len() as u64)
}

fn value_bytes(shape: &Shape) -> Option<u64> {
    match shape {
        Shape::Int { min, max } => Some(int_digits(min.unwrap_or(i64::MIN)).max(int_digits(max.unwrap_or(i64::MAX)))),
        Shape::Bool => Some(5),
        Shape::Text { max_chars } => max_chars.map(|n| 2 + 4 * n as u64),
        Shape::Adt { variants, .. } => {
            let mut best = 0;
            for v in variants {
                let mut n = format!("{{\"kind\":\"{}\"", v.name).len() as u64 + 1;
                for (f, fs) in &v.fields {
                    n += format!(",\"{f}\":").len() as u64 + value_bytes(fs)?;
                }
                best = best.max(n);
            }
            Some(best)
        }
    }
}

/// Codifica um valor JSON na ordem canônica da forma: `kind` primeiro, depois os campos
/// na ordem declarada. Variantes ou campos desconhecidos vão em ordem alfabética: o
/// resultado é uma *proposta*, que pode ser inválida.
pub fn proposal_from_json(shape: &Shape, j: &Json) -> String {
    format!("{ROOT_OPEN}{}{ROOT_CLOSE}", encode_loose(Some(shape), j))
}

fn encode_loose(shape: Option<&Shape>, j: &Json) -> String {
    match j {
        Json::Object(obj) => {
            let kind = obj.get("kind").and_then(|k| k.as_str());
            let variant = match (shape, kind) {
                (Some(Shape::Adt { variants, .. }), Some(k)) => variants.iter().find(|v| v.name == k),
                _ => None,
            };
            let mut parts = Vec::new();
            if let Some(k) = kind {
                parts.push(format!("\"kind\":{}", Json::String(k.to_string())));
            }
            let mut seen = vec!["kind".to_string()];
            if let Some(v) = variant {
                for (f, fs) in &v.fields {
                    if let Some(x) = obj.get(f) {
                        parts.push(format!("{}:{}", Json::String(f.clone()), encode_loose(Some(fs), x)));
                        seen.push(f.clone());
                    }
                }
            }
            for (k, x) in obj {
                if !seen.contains(k) {
                    parts.push(format!("{}:{}", Json::String(k.clone()), encode_loose(None, x)));
                }
            }
            format!("{{{}}}", parts.join(","))
        }
        other => other.to_string(),
    }
}

/// Validação estrita de um documento `{"value": …}` contra a forma (defesa em profundidade
/// para provedores que não impõem a gramática por inteiro).
pub fn check(shape: &Shape, doc: &Json) -> Result<(), String> {
    let obj = doc.as_object().ok_or("a saída deve ser um objeto `{\"value\": …}`")?;
    if obj.len() != 1 {
        return Err("a saída deve ter exatamente a chave `value`".into());
    }
    let v = obj.get("value").ok_or("chave `value` ausente")?;
    check_value(shape, v, "value")
}

fn check_value(shape: &Shape, v: &Json, path: &str) -> Result<(), String> {
    match shape {
        Shape::Int { min, max } => {
            let n = v.as_i64().ok_or_else(|| format!("`{path}` deve ser inteiro"))?;
            if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                return Err(format!("`{path}` = {n} fora do refinamento [{}..{}]", show(*min), show(*max)));
            }
            Ok(())
        }
        Shape::Bool => v.as_bool().map(|_| ()).ok_or_else(|| format!("`{path}` deve ser booleano")),
        Shape::Text { max_chars } => {
            let s = v.as_str().ok_or_else(|| format!("`{path}` deve ser texto"))?;
            let n = s.chars().count() as u32;
            if max_chars.is_some_and(|m| n > m) {
                return Err(format!("`{path}` tem {n} caracteres; o refinamento admite até {}", max_chars.unwrap()));
            }
            Ok(())
        }
        Shape::Adt { name, variants } => {
            let obj = v.as_object().ok_or_else(|| format!("`{path}` deve ser um objeto `{name}`"))?;
            let kind = obj.get("kind").and_then(|k| k.as_str()).ok_or_else(|| format!("`{path}.kind` ausente"))?;
            let var = variants
                .iter()
                .find(|x| x.name == kind)
                .ok_or_else(|| format!("`{kind}` não é variante de `{name}`"))?;
            if obj.len() != var.fields.len() + 1 {
                return Err(format!("`{path}` deve ter exatamente os campos de `{kind}`"));
            }
            for (f, fs) in &var.fields {
                let x = obj.get(f).ok_or_else(|| format!("campo `{path}.{f}` ausente"))?;
                check_value(fs, x, &format!("{path}.{f}"))?;
            }
            Ok(())
        }
    }
}

fn show(b: Option<i64>) -> String {
    b.map(|n| n.to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ath_syntax::parse_module;

    pub(crate) fn action_shape() -> Shape {
        let m = parse_module(
            "module t\ntype Action = | Restart { deploy: Text[..32] } | Scale { deploy: Text[..32], to: Int[2..64] }",
        )
        .unwrap();
        let ty = TypeExpr::Named(ath_syntax::ast::Ident { name: "Action".into(), span: Default::default() });
        shape_of(&m, &ty).unwrap()
    }

    #[test]
    fn max_bytes_bounds_the_encoding() {
        let s = action_shape();
        // pior caso: 32 caracteres de 4 bytes no `deploy` e o maior `to`
        let worst = format!("{{\"value\":{{\"kind\":\"Scale\",\"deploy\":\"{}\",\"to\":64}}}}", "😀".repeat(32));
        assert_eq!(max_bytes(&s), Some(worst.len() as u64));
        assert_eq!(max_bytes(&s), Some(174));
    }

    #[test]
    fn check_accepts_valid_and_rejects_out_of_type() {
        let s = action_shape();
        let ok: Json = serde_json::from_str(r#"{"value":{"kind":"Scale","deploy":"api","to":3}}"#).unwrap();
        assert!(check(&s, &ok).is_ok());
        for bad in [
            r#"{"value":{"kind":"Delete","deploy":"db"}}"#,
            r#"{"value":{"kind":"Scale","deploy":"api","to":0}}"#,
            r#"{"value":{"kind":"Restart","deploy":"api","extra":1}}"#,
            r#"{"value":{"kind":"Restart"}}"#,
        ] {
            assert!(check(&s, &serde_json::from_str(bad).unwrap()).is_err(), "{bad}");
        }
    }

    #[test]
    fn proposal_puts_kind_first() {
        let s = action_shape();
        let j: Json = serde_json::from_str(r#"{"to":0,"deploy":"payments","kind":"Scale"}"#).unwrap();
        assert_eq!(proposal_from_json(&s, &j), r#"{"value":{"kind":"Scale","deploy":"payments","to":0}}"#);
    }
}
