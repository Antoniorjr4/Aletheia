//! Gramática GBNF (llama.cpp) da codificação canônica.
//!
//! Intervalos inteiros com até 256 valores viram alternância exata de literais;
//! intervalos maiores usam o padrão genérico de inteiro e dependem da validação
//! do ARX para os limites (anotado num comentário da própria gramática).

use crate::Shape;
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub fn to_gbnf(shape: &Shape) -> String {
    let mut rules: BTreeMap<String, String> = BTreeMap::new();
    let mut notes = Vec::new();
    let top = rule(shape, &mut rules, &mut notes);
    let mut out = String::from("# gerada por athc a partir do tipo de saída\n");
    for n in notes {
        let _ = writeln!(out, "# {n}");
    }
    let _ = writeln!(out, "root ::= {} {} {}", quote("{\"value\":"), top, quote("}"));
    for (name, body) in &rules {
        let _ = writeln!(out, "{name} ::= {body}");
    }
    out
}

fn quote(s: &str) -> String {
    let mut q = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => q.push_str("\\\""),
            '\\' => q.push_str("\\\\"),
            c => q.push(c),
        }
    }
    q.push('"');
    q
}

fn rule(shape: &Shape, rules: &mut BTreeMap<String, String>, notes: &mut Vec<String>) -> String {
    match shape {
        Shape::Bool => {
            rules.insert("bool".into(), format!("{} | {}", quote("true"), quote("false")));
            "bool".into()
        }
        Shape::Text { max_chars } => {
            rules.insert("char".into(), r#"[^"\\\x00-\x1F]"#.into());
            match max_chars {
                Some(n) => {
                    let name = format!("text-{n}");
                    rules.insert(name.clone(), format!("{} char{{0,{n}}} {}", quote("\""), quote("\"")));
                    name
                }
                None => {
                    rules.insert("text".into(), format!("{} char* {}", quote("\""), quote("\"")));
                    "text".into()
                }
            }
        }
        Shape::Int { min, max } => match (min, max) {
            (Some(a), Some(b)) if (b - a) <= 256 => {
                let sig = |n: i64| if n < 0 { format!("m{}", n.unsigned_abs()) } else { n.to_string() };
                let name = format!("int-{}-{}", sig(*a), sig(*b));
                let alts: Vec<String> = (*a..=*b).rev().map(|n| quote(&n.to_string())).collect();
                rules.insert(name.clone(), alts.join(" | "));
                name
            }
            _ => {
                notes.push(format!(
                    "int [{}..{}]: padrão genérico; o ARX valida os limites na decodificação",
                    min.map(|n| n.to_string()).unwrap_or_default(),
                    max.map(|n| n.to_string()).unwrap_or_default()
                ));
                rules.insert("int".into(), r#""-"? ("0" | [1-9] [0-9]{0,18})"#.into());
                "int".into()
            }
        },
        Shape::Adt { name, variants } => {
            let mut alts = Vec::new();
            for v in variants {
                let vname = format!("{}-{}", name.to_lowercase(), v.name.to_lowercase());
                let mut body = quote(&format!("{{\"kind\":\"{}\"", v.name));
                for (f, fs) in &v.fields {
                    let r = rule(fs, rules, notes);
                    let _ = write!(body, " {} {}", quote(&format!(",\"{f}\":")), r);
                }
                let _ = write!(body, " {}", quote("}"));
                rules.insert(vname.clone(), body);
                alts.push(vname);
            }
            let tname = name.to_lowercase();
            rules.insert(tname.clone(), alts.join(" | "));
            tname
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::action_shape;

    #[test]
    fn gbnf_enumerates_small_ranges_and_variants() {
        let g = super::to_gbnf(&action_shape());
        assert!(g.contains(r#"root ::= "{\"value\":" action "}""#), "{g}");
        assert!(g.contains("action ::= action-restart | action-scale"), "{g}");
        assert!(
            g.contains(r#"action-scale ::= "{\"kind\":\"Scale\"" ",\"deploy\":" text-32 ",\"to\":" int-2-64 "}""#),
            "{g}"
        );
        assert!(g.contains(r#"int-2-64 ::= "64" | "63""#), "{g}");
        assert!(!g.contains(r#""1" |"#), "1 está fora do intervalo: {g}");
        assert!(g.contains(r#"text-32 ::= "\"" char{0,32} "\"""#), "{g}");
    }
}
