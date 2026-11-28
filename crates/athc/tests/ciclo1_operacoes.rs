//! **Origem das operações endossadas (protótipo de 27/11; E0453 desde 28/11/2026).**
//!
//! `ath_syntax::operations` responde se, numa chamada de ferramenta, os argumentos vindos de
//! valores endossados formam uma só operação aprovada. Desde 28/11 o achado `Mistura` é a E0453:
//! o programa não compila. Estes testes leem a análise sobre a AST (sem compilar, para ver também
//! o achado dos programas recusados) e conferem a recusa na compilação.

use ath_syntax::operations::{analyze, Finding};
use athc::compile;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// (ferramenta, achado resumido) de cada sítio que tem origem endossada.
fn sitios(src: &str) -> Vec<(String, &'static str)> {
    let m = ath_syntax::parser::parse_module(src).unwrap_or_else(|e| panic!("{e:?}"));
    analyze(&m)
        .into_iter()
        .filter_map(|s| {
            let k = match s.finding {
                Finding::SemOrigem => return None,
                Finding::Uma => "uma",
                Finding::Transformada => "transformada",
                Finding::Agregacao(_) => "agregação",
                Finding::Mistura(ref ops) if ops.len() >= 2 => "mistura",
                Finding::Mistura(_) => unreachable!(),
            };
            Some((s.tool, k))
        })
        .collect()
}

/// Os códigos com que a compilação recusa o programa (vazio: aceito).
fn codigos(src: &str) -> Vec<String> {
    compile(src).err().unwrap_or_default().into_iter().map(|d| d.code).collect()
}

fn arquivo(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap()
}

#[test]
fn a_base_do_m4_nao_mistura_operacoes() {
    let s = sitios(&arquivo("bench/agentdojo/banking_v2_keeping.ath"));
    assert!(s.iter().all(|(_, k)| *k == "uma"), "{s:?}");
    assert_eq!(s.len(), 7);
}

#[test]
fn a_mutacao_x2_mistura_duas_operacoes_aprovadas() {
    let base = arquivo("bench/agentdojo/banking_v2_keeping.ath");
    let x2 = base.replace(
        "                Send { recipient, amount, subject, date } => commit send_money(bank, recipient, amount, subject, date),",
        "                Send { recipient, subject, date } => {\n                  for w2 in writes { match w2 { Send { amount } => commit send_money(bank, recipient, amount, subject, date), _ => \"\" } }\n                  \"\"\n                },",
    );
    assert_ne!(base, x2);
    let s = sitios(&x2);
    assert!(s.contains(&("send_money".to_string(), "mistura")), "{s:?}");
    assert_eq!(s.iter().filter(|(_, k)| *k == "mistura").count(), 1, "{s:?}");
    assert!(codigos(&base).is_empty());
    assert_eq!(codigos(&x2), vec!["E0453"]);
}

#[test]
fn contrato_relacional_e_composicoes() {
    let x = |f: &str| sitios(&arquivo(&format!("bench/agentdojo/x2/{f}.ath")));
    assert_eq!(x("relacional/correto"), vec![("send_money".to_string(), "uma")]);
    assert_eq!(x("relacional/misturado"), vec![("send_money".to_string(), "mistura")]);
    // a operação aprovada mais dado confiável e literal: uma operação
    assert_eq!(x("composicao/c1_operacao_mais_confiavel"), vec![("send_money".to_string(), "uma")]);
    // a composição proposta como operação própria e aprovada pela política: uma operação
    assert_eq!(x("composicao/c2_composicao_proposta"), vec![("send_money".to_string(), "uma")]);
    // a composição feita pelo programa com campos de duas operações: mistura
    assert_eq!(x("composicao/c3_composicao_do_programa"), vec![("send_money".to_string(), "mistura")]);
    let c = |f: &str| codigos(&arquivo(&format!("bench/agentdojo/x2/{f}.ath")));
    for f in ["relacional/correto", "composicao/c1_operacao_mais_confiavel", "composicao/c2_composicao_proposta"] {
        assert!(c(f).is_empty(), "{f}: {:?}", c(f));
    }
    for f in ["relacional/misturado", "composicao/c3_composicao_do_programa"] {
        assert_eq!(c(f), vec!["E0453"], "{f}");
    }
}

#[test]
fn agregar_o_valor_endossado_inteiro_nao_e_mistura_mas_misturar_com_um_elemento_e() {
    let base = r#"module t
type Op = | Pay { to: Text[..16], memo: Text[..16] }
type Plan = | Plan { ops: List[Op, 0..3], note: Text[..40] }
model planner { price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 4000  reasoning = 0  ttft = 1.0  per_tok = 0.01 }
tool pay(c: Cap[Bank.Pay], to: Text ^ Endorsed, memo: Text) -> Unit ! { Bank.Pay } reversible latency 1.0
policy P(p: Plan) -> Bool = true
agent A(bank: Cap[Bank.Pay]) budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {
  on go(req: Text[..200] ^ Trusted) -> Text ! { Infer, Bank.Pay } {
    let p = refine (infer planner -> Plan { request = req }) by P else { return "none" }
    match p { Plan { ops, note } => { BODY
      "ok" } }
  }
}
"#;
    // um campo do plano inteiro (`note`) com um campo de um elemento: duas operações
    let mix = base.replace("BODY", "for o in ops { match o { Pay { to } => pay(bank, to, note) } }");
    assert_eq!(sitios(&mix), vec![("pay".to_string(), "mistura")]);
    assert_eq!(codigos(&mix), vec!["E0453"]);
    // só campos do elemento, com o memo transformado: uma operação, transformada
    let tr = base.replace("BODY", "for o in ops { match o { Pay { to, memo } => pay(bank, to, memo ++ \"!\") } }");
    assert_eq!(sitios(&tr), vec![("pay".to_string(), "transformada")]);
    assert!(codigos(&tr).is_empty());
}
