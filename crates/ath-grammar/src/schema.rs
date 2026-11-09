//! JSON Schema estrito para provedores com saída estruturada.
//!
//! Formato compatível com `response_format: {"type": "json_schema", "strict": true}`
//! (API OpenAI, vLLM, llama.cpp): raiz objeto, `additionalProperties: false` e todos os
//! campos obrigatórios em todo objeto. Variantes viram `anyOf` discriminado por `kind`.
//! Provedores que ignorem `minimum`, `maximum` ou `maxLength` são cobertos pela
//! validação do ARX, que trata a saída fora do tipo como amostra rejeitada.

use crate::Shape;
use serde_json::{json, Value as Json};

pub fn to_json_schema(shape: &Shape) -> Json {
    json!({
        "type": "object",
        "properties": { "value": schema(shape) },
        "required": ["value"],
        "additionalProperties": false,
    })
}

fn schema(shape: &Shape) -> Json {
    match shape {
        Shape::Int { min, max } => {
            let mut s = json!({ "type": "integer" });
            if let Some(m) = min {
                s["minimum"] = json!(m);
            }
            if let Some(m) = max {
                s["maximum"] = json!(m);
            }
            s
        }
        Shape::Bool => json!({ "type": "boolean" }),
        Shape::Text { max_chars } => {
            let mut s = json!({ "type": "string" });
            if let Some(m) = max_chars {
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
                        props.insert(f.clone(), schema(fs));
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
    use crate::tests::action_shape;

    #[test]
    fn schema_is_strict_and_discriminated() {
        let s = super::to_json_schema(&action_shape());
        assert_eq!(s["additionalProperties"], false);
        let variants = s["properties"]["value"]["anyOf"].as_array().unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[1]["properties"]["kind"]["enum"][0], "Scale");
        assert_eq!(variants[1]["properties"]["to"]["minimum"], 2);
        assert_eq!(variants[1]["properties"]["to"]["maximum"], 64);
        assert_eq!(variants[1]["properties"]["deploy"]["maxLength"], 32);
        assert_eq!(variants[1]["required"], serde_json::json!(["kind", "deploy", "to"]));
    }
}
