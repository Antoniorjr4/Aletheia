//! Rev7: ancoragem dinâmica tipada. O inventário que o modelo recebe e o inventário que a
//! política aplica vêm da mesma fotografia confiável do host, e o compilador veta qualquer
//! descompasso entre os dois, sem depender de nomes de política, tipo ou campo.

use athc::compile;
use std::path::PathBuf;

fn source() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/remediate.ath");
    std::fs::read_to_string(p).unwrap()
}

/// Códigos dos diagnósticos: o verificador só relata o primeiro estrato que falha.
fn errors(src: &str) -> Vec<String> {
    match compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.into_iter().map(|d| d.code).collect(),
    }
}

const ANCHOR: &str = "estado = s.managed(),";
const POLICY: &str = "Restart { deploy } => s.is_managed(deploy),";

#[test]
fn the_reference_program_is_anchored() {
    let src = source();
    assert!(src.contains(ANCHOR) && src.contains(POLICY));
    assert!(errors(&src).is_empty(), "{:?}", errors(&src));
}

#[test]
fn forged_inventory_in_the_prompt_is_e0421() {
    let src = source().replace(ANCHOR, r#"estado = "api, worker","#);
    assert_eq!(errors(&src), ["E0421"]);
}

#[test]
fn missing_inventory_in_the_prompt_is_e0421() {
    let src = source().replace(ANCHOR, "");
    assert_eq!(errors(&src), ["E0421"]);
}

#[test]
fn policy_with_handwritten_inventory_against_an_anchored_prompt_is_e0422() {
    let src = source().replace(POLICY, r#"Restart { deploy } => deploy == "api" || deploy == "worker","#);
    assert_eq!(errors(&src), ["E0422"]);
}

#[test]
fn anchoring_does_not_depend_on_the_field_name() {
    let src = source().replace(ANCHOR, "inventario = s.managed(),");
    assert!(errors(&src).is_empty(), "{:?}", errors(&src));
}

#[test]
fn anchoring_does_not_depend_on_the_policy_name() {
    let src = source().replace("SafeRemediation", "Guarda");
    assert!(errors(&src).is_empty(), "{:?}", errors(&src));
    let forged = src.replace(ANCHOR, r#"estado = "api","#);
    assert_eq!(errors(&forged), ["E0421"]);
}

#[test]
fn a_local_binding_named_s_cannot_forge_the_snapshot() {
    // `s.managed()` sempre lê a fotografia do host; uma variável local `s` não a intercepta.
    let src =
        source().replace("let draft = infer planner", "let s = \"api, worker, db\"\n    let draft = infer planner");
    assert!(src.contains("let s = "));
    assert!(errors(&src).is_empty(), "{:?}", errors(&src));
}

#[test]
fn contextual_probe_outside_a_policy_is_e0423() {
    let src =
        source().replace("let draft = infer planner", "let ok = s.is_managed(service)\n    let draft = infer planner");
    assert!(src.contains("let ok = s.is_managed"));
    assert_eq!(errors(&src), ["E0423"]);
}

// ------------------------------------------------------------------ inventários nomeados

/// Um programa mínimo de inventário nomeado: o prompt recebe `s.inventory("payees")` e a
/// política decide por `s.member("payees", …)`, a mesma fonte.
fn payees(prompt_field: &str, policy: &str) -> String {
    format!(
        "module bank.mini
type Pay = | Pay {{ to: Text[..34] }}
model m {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
tool pay(c: Cap[Bank.Transfer], to: Text[..34] ^ Endorsed) -> Text ! {{ Bank.Transfer }} irrevocable latency 1.0
policy Known(p: Pay) -> Bool = match p {{ Pay {{ to }} => {policy} }}
agent A(bank: Cap[Bank.Transfer]) budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {{
  on go(req: Text[..200] ^ Trusted) -> Text ! {{ Infer, Bank.Transfer }} {{
    let d = infer m -> Pay {{ request = req{prompt_field} }}
    let p = refine d by Known else {{ return \"recusado\" }}
    match p {{ Pay {{ to }} => commit pay(bank, to) }}
  }}
}}
"
    )
}

#[test]
fn named_inventory_anchors_prompt_and_policy() {
    let src = payees(", payees = s.inventory(\"payees\")", "s.member(\"payees\", to)");
    assert!(errors(&src).is_empty(), "{:?}", errors(&src));
}

#[test]
fn named_inventory_missing_from_the_prompt_is_e0421() {
    assert_eq!(errors(&payees("", "s.member(\"payees\", to)")), ["E0421"]);
    // o prompt recebe OUTRO inventário: a política decide por `payees`, que o modelo não viu
    let other = payees(", ids = s.inventory(\"scheduled\")", "s.member(\"payees\", to)");
    assert_eq!(errors(&other), ["E0421"]);
}

#[test]
fn named_inventory_with_hand_written_literals_is_e0422() {
    let src = payees(", payees = s.inventory(\"payees\")", "to == \"US133000000121212121212\"");
    assert_eq!(errors(&src), ["E0422"]);
}

#[test]
fn inventory_name_must_be_a_literal_and_member_stays_in_policy() {
    let src = payees(", payees = s.inventory(req)", "s.member(\"payees\", to)");
    assert!(errors(&src).contains(&"E0424".to_string()), "{:?}", errors(&src));
    let src = payees(", payees = s.inventory(\"payees\"), ok = s.member(\"payees\", req)", "s.member(\"payees\", to)");
    assert!(errors(&src).contains(&"E0423".to_string()), "{:?}", errors(&src));
}
