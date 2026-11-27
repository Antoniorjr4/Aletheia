//! **Protótipo de diagnóstico: integridade da operação endossada (27/11/2026).**
//!
//! `ath_syntax::operations` não muda a aceitação. Ele responde se, numa chamada de ferramenta, os
//! argumentos vindos de valores endossados formam uma só operação aprovada. Estes testes fixam o
//! comportamento nos exemplos da proposta de arquitetura (`bench/agentdojo/x2`).

use ath_syntax::operations::{analyze, Finding};
use athc::compile;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// (ferramenta, achado resumido) de cada sítio que tem origem endossada.
fn sitios(src: &str) -> Vec<(String, &'static str)> {
    let c = compile(src).unwrap_or_else(|d| panic!("{:?}", d.iter().map(|x| &x.msg).collect::<Vec<_>>()));
    analyze(&c.module)
        .into_iter()
        .filter_map(|s| {
            let k = match s.finding {
                Finding::SemOrigem => return None,
                Finding::Uma => "uma",
                Finding::Transformada => "transformada",
                Finding::Mistura(ref ops) if ops.len() >= 2 => "mistura",
                Finding::Mistura(_) => unreachable!(),
            };
            Some((s.tool, k))
        })
        .collect()
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
    // só campos do elemento, com o memo transformado: uma operação, transformada
    let tr = base.replace("BODY", "for o in ops { match o { Pay { to, memo } => pay(bank, to, memo ++ \"!\") } }");
    assert_eq!(sitios(&tr), vec![("pay".to_string(), "transformada")]);
}
