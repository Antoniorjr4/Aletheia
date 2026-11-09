use ath_syntax::ast::TypeExpr;
use ath_syntax::Span;
use serde_json::json;
use std::collections::BTreeSet;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct Draft {
    pub model: String,
    /// Nome do tipo de saída (para mensagens).
    pub out: String,
    /// Tipo de saída completo, com refinamentos: dele sai a gramática da decodificação.
    pub out_ty: TypeExpr,
    pub prompt: String,
    pub site: Span,
}

#[derive(Clone, Debug)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Text(String),
    Unit,
    Adt {
        ty: String,
        variant: String,
        fields: Vec<(String, Value)>,
    },
    Cap(BTreeSet<String>),
    Key(u64),
    Draft(Rc<Draft>),
    /// Obrigação em aberto: a chamada `tool(args)` com a chave `key` terminou em `TimeoutUnknown`.
    Pending {
        key: u64,
        tool: String,
        args: Vec<Value>,
    },
}

impl PartialEq for Value {
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Text(a), Value::Text(b)) => a == b,
            (Value::Unit, Value::Unit) => true,
            (Value::Adt { variant: va, fields: fa, .. }, Value::Adt { variant: vb, fields: fb, .. }) => {
                va == vb && fa == fb
            }
            (Value::Key(a), Value::Key(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    pub fn show(&self) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Text(s) => {
                let one_line = s.replace('\n', " ⏎ ");
                let short: String = one_line.chars().take(48).collect();
                if one_line.chars().count() > 48 {
                    format!("{short:?}…")
                } else {
                    format!("{short:?}")
                }
            }
            Value::Unit => "()".into(),
            Value::Adt { variant, fields, .. } => {
                if fields.is_empty() {
                    variant.clone()
                } else {
                    let fs: Vec<String> = fields.iter().map(|(k, v)| format!("{k}: {}", v.show())).collect();
                    format!("{variant} {{ {} }}", fs.join(", "))
                }
            }
            Value::Cap(_) => "⟨cap⟩".into(),
            Value::Key(k) => format!("k{k}"),
            Value::Draft(d) => format!("⟨rascunho {}⟩", d.model),
            Value::Pending { key, tool, .. } => format!("Pending[k{key}]({tool})"),
        }
    }

    /// Texto bruto para prompts (sem aspas).
    pub fn prompt_text(&self) -> String {
        match self {
            Value::Text(s) => s.clone(),
            other => other.show(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::Int(n) => json!(n),
            Value::Bool(b) => json!(b),
            Value::Text(s) => json!(s),
            Value::Unit => json!(null),
            Value::Adt { variant, fields, .. } => {
                let mut m = serde_json::Map::new();
                for (k, v) in fields {
                    m.insert(k.clone(), v.to_json());
                }
                json!({ variant: m })
            }
            Value::Cap(c) => json!({ "cap": c.iter().cloned().collect::<Vec<_>>() }),
            Value::Key(k) => json!(format!("k{k}")),
            Value::Draft(d) => json!({ "draft": d.model }),
            Value::Pending { key, tool, .. } => json!({ "pending": format!("k{key}"), "tool": tool }),
        }
    }
}
