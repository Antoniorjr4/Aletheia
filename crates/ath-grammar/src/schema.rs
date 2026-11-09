//! JSON Schema estrito para provedores com saída estruturada.
//!
//! Formato compatível com `response_format: {"type": "json_schema", "strict": true}`
//! (API OpenAI, Groq, vLLM, llama.cpp): raiz objeto, `additionalProperties: false` e
//! todos os campos obrigatórios em todo objeto. Variantes viram `anyOf` discriminado
//! por `kind`.
//!
//! Dois dialetos:
//!
//! - [`Dialect::Full`] usa `minimum`, `maximum` e `maxLength`.
//! - [`Dialect::Core`] usa só o núcleo que todo modo estrito documenta (`type`,
//!   `properties`, `required`, `additionalProperties`, `enum`, `anyOf`). Intervalos
//!   inteiros com até [`ENUM_LIMIT`] valores viram `enum` exato, e a restrição continua
//!   imposta pelo servidor. Intervalos maiores e tetos de texto saem do esquema e são
//!   impostos pela validação do ARX, que trata a saída fora do tipo como amostra rejeitada.

use crate::Shape;
use serde_json::{json, Value as Json};

/// Maior intervalo inteiro convertido em `enum` no dialeto núcleo.
pub const ENUM_LIMIT: i64 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Full,
    Core,
}

impl Dialect {
    pub fn parse(s: &str) -> Option<Dialect> {
        match s {
            "full" => Some(Dialect::Full),
            "core" => Some(Dialect::Core),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Dialect::Full => "full",
            Dialect::Core => "core",
        }
    }
}

pub fn to_json_schema(shape: &Shape) -> Json {
    to_json_schema_with(shape, Dialect::Full)
}

pub fn to_json_schema_with(shape: &Shape, dialect: Dialect) -> Json {
    json!({
        "type": "object",
        "properties": { "value": schema(shape, dialect) },
        "required": ["value"],
        "additionalProperties": false,
    })
}

/// Restrições do tipo que o esquema do dialeto não carrega (impostas só pela validação do ARX).
pub fn unenforced(shape: &Shape, dialect: Dialect) -> Vec<String> {
    let mut out = Vec::new();
    collect_unenforced(shape, dialect, "value", &mut out);
    out
}

fn collect_unenforced(shape: &Shape, dialect: Dialect, path: &str, out: &mut Vec<String>) {
    if dialect == Dialect::Full {
        return;
    }
    match shape {
        Shape::Int { min: Some(a), max: Some(b) } if b - a <= ENUM_LIMIT => {}
        Shape::Int { min: None, max: None } | Shape::Bool => {}
        Shape::Int { .. } => out.push(format!("{path}: limites inteiros")),
        Shape::Text { max_chars: Some(n) } => out.push(format!("{path}: até {n} caracteres")),
        Shape::Text { max_chars: None } => {}
        Shape::List { elem, .. } => collect_unenforced(elem, dialect, &format!("{path}[]"), out),
        Shape::Adt { variants, .. } => {
            for v in variants {
                for (f, fs) in &v.fields {
                    collect_unenforced(fs, dialect, &format!("{path}.{}.{f}", v.name), out);
                }
            }
        }
    }
}

fn schema(shape: &Shape, dialect: Dialect) -> Json {
    match shape {
        Shape::Int { min, max } => match (dialect, min, max) {
            (Dialect::Core, Some(a), Some(b)) if b - a <= ENUM_LIMIT => {
                json!({ "type": "integer", "enum": (*a..=*b).collect::<Vec<i64>>() })
            }
            (Dialect::Core, ..) => json!({ "type": "integer" }),
            (Dialect::Full, ..) => {
                let mut s = json!({ "type": "integer" });
                if let Some(m) = min {
                    s["minimum"] = json!(m);
                }
                if let Some(m) = max {
                    s["maximum"] = json!(m);
                }
                s
            }
        },
        Shape::Bool => json!({ "type": "boolean" }),
        // `minItems`/`maxItems` nos dois dialetos: fazem parte do núcleo das saídas estruturadas
        // (OpenAI, vLLM); onde o servidor não os impõe, a validação do ARX rejeita a amostra.
        Shape::List { elem, min, max } => json!({
            "type": "array",
            "items": schema(elem, dialect),
            "minItems": min,
            "maxItems": max,
        }),
        Shape::Text { max_chars } => {
            let mut s = json!({ "type": "string" });
            if let (Dialect::Full, Some(m)) = (dialect, max_chars) {
                s["maxLength"] = json!(m);
            }
            s
        }
        Shape::Adt { variants, .. } => {
            let objs: Vec<Json> = variants
                .iter()
                .map(|v| {
                    let mut props = serde_json::Map::new();
                    props.insert("kind".into(), json!({ "type": "string", "enum": [v.name] }));
                    let mut required = vec![json!("kind")];
                    for (f, fs) in &v.fields {
                        props.insert(f.clone(), schema(fs, dialect));
                        required.push(json!(f));
                    }
                    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
                })
                .collect();
            if objs.len() == 1 {
                objs.into_iter().next().unwrap()
            } else {
                json!({ "anyOf": objs })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::action_shape;

    #[test]
    fn schema_is_strict_and_discriminated() {
        let s = to_json_schema(&action_shape());
        assert_eq!(s["additionalProperties"], false);
        let variants = s["properties"]["value"]["anyOf"].as_array().unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[1]["properties"]["kind"]["enum"][0], "Scale");
        assert_eq!(variants[1]["properties"]["to"]["minimum"], 2);
        assert_eq!(variants[1]["properties"]["to"]["maximum"], 64);
        assert_eq!(variants[1]["properties"]["deploy"]["maxLength"], 32);
        assert_eq!(variants[1]["required"], serde_json::json!(["kind", "deploy", "to"]));
    }

    #[test]
    fn lists_carry_min_and_max_items() {
        for d in [Dialect::Full, Dialect::Core] {
            let s = to_json_schema_with(&crate::tests::plan_shape(), d);
            let v = &s["properties"]["value"];
            assert_eq!(
                (v["type"].as_str(), v["minItems"].as_u64(), v["maxItems"].as_u64()),
                (Some("array"), Some(1), Some(3))
            );
            assert_eq!(v["items"]["anyOf"].as_array().unwrap().len(), 2);
        }
    }

    #[test]
    fn core_dialect_turns_small_ranges_into_exact_enums() {
        let s = to_json_schema_with(&action_shape(), Dialect::Core);
        let to = &s["properties"]["value"]["anyOf"][1]["properties"]["to"];
        let allowed: Vec<i64> = to["enum"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
        assert_eq!(allowed, (2..=64).collect::<Vec<_>>(), "0 e 1 não existem no esquema");
        assert!(to.get("minimum").is_none());
        let text = serde_json::to_string(&s).unwrap();
        assert!(!text.contains("maxLength") && !text.contains("minimum"), "{text}");
        assert_eq!(
            unenforced(&action_shape(), Dialect::Core),
            ["value.Restart.deploy: até 32 caracteres", "value.Scale.deploy: até 32 caracteres"]
        );
        assert!(unenforced(&action_shape(), Dialect::Full).is_empty());
    }
}
