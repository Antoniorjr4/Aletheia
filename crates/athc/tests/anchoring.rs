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
