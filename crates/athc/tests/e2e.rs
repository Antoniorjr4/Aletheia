//! Testes ponta a ponta do Marco Zero: compilar, rejeitar, executar, endereçar e auditar.

use arx::host::{Host, SimWorld};
use arx::journal::Journal;
use arx::model::Scripted;
use arx::{Arx, Meter, Value};
use ath_check::Stratum;
use athc::compile;
use std::path::PathBuf;
use std::process::Command;

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(examples().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn json(rel: &str) -> serde_json::Value {
    serde_json::from_str(&read(rel)).unwrap()
}

fn world() -> SimWorld {
    SimWorld::from_json(&json("fixtures/world.json")).unwrap()
}

fn script(responses: &[&str]) -> Scripted {
    Scripted::from_json(&serde_json::json!({ "planner": responses })).unwrap()
}

fn kinds(j: &Journal) -> Vec<String> {
    j.entries.iter().map(|e| e["kind"].as_str().unwrap().to_string()).collect()
}

// ------------------------------------------------------------------ compilação

#[test]
fn accepts_remediate_and_proves_budget() {
    let c = compile(&read("remediate.ath")).expect("programa válido deve compilar");
    let r = &c.check.reports[0];
    assert_eq!((r.agent.as_str(), r.event.as_str()), ("Remediator", "alert"));
    // 2 (logs) + 3 × 9,5 (infer) + 1 (apply) + 1 (page) = 32,5 s
    assert!((r.worst.wall - 32.5).abs() < 1e-9, "wall = {}", r.worst.wall);
    assert_eq!(r.worst.tok, 3 * (2030 + 400));
    assert!(r.worst.wall <= r.wall.unwrap());
    let effs: Vec<&str> = c.check.facts.effects.iter().map(String::as_str).collect();
    assert_eq!(effs, ["Cluster.Read", "Cluster.Write", "Infer", "Page"]);
}

#[test]
fn rejects_each_error_example_in_the_expected_stratum() {
    let cases = [
        ("e0412_injection.ath", "E0412", Stratum::E3),
        ("e0412_implicit_flow.ath", "E0412", Stratum::E3),
        ("e0415_prompt_injection.ath", "E0415", Stratum::E3),
        ("e1104_key_reuse.ath", "E1104", Stratum::E1),
        ("e0502_capability.ath", "E0502", Stratum::E2),
        ("e0501_effect_row.ath", "E0501", Stratum::E2),
        ("t0301_commit.ath", "T0301", Stratum::E2),
        ("b0110_unbounded_prompt.ath", "B0110", Stratum::E6),
        ("b0207_wall_budget.ath", "B0207", Stratum::E6),
    ];
    for (file, code, stratum) in cases {
        let diags = match compile(&read(&format!("errors/{file}"))) {
            Ok(_) => panic!("{file}: deveria ser rejeitado"),
            Err(d) => d,
        };
        assert_eq!(diags[0].code, code, "{file}");
        assert_eq!(diags[0].stratum, stratum, "{file}");
    }
}

#[test]
fn implicit_flow_points_at_the_untrusted_condition() {
    let src = read("errors/e0412_implicit_flow.ath");
    let d = &compile(&src).err().unwrap()[0];
    let (span, label) = &d.secondary[0];
    assert_eq!(&src[span.lo as usize..span.hi as usize], "l == \"ok\"");
    assert!(label.contains("Untrusted"));
}

#[test]
fn injection_diagnostic_traces_the_origin() {
    let d = &compile(&read("errors/e0412_injection.ath")).err().unwrap()[0];
    assert!(d.notes[0].contains("logs(…) devolve ^ Untrusted → infer planner → sample(…)"), "{:?}", d.notes);
}

// ------------------------------------------------------------------ execução

#[test]
fn runs_remediate_end_to_end_and_rejects_the_injected_plan() {
    let c = compile(&read("remediate.ath")).unwrap();
    let models = Scripted::from_json(&json("fixtures/models.json")).unwrap();
    let mut arx = Arx::new(&c.module, Box::new(world()), Box::new(models));
    let out = arx.run("Remediator", "alert", vec![Value::Text("api".into())]).unwrap();
    assert_eq!(out, Value::Text("resolvido".into()));

    let k = kinds(&arx.journal);
    let reject = k.iter().position(|s| s == "policy_reject").expect("a 1ª proposta deve ser rejeitada");
    let endorse = k.iter().position(|s| s == "endorse").expect("a 2ª proposta deve ser endossada");
    assert!(reject < endorse);
    assert_eq!(arx.journal.entries[reject]["detail"]["value"]["Scale"]["deploy"], "payments");

    let world = arx.host.summary().join("; ");
    assert!(world.contains("api reiniciado 1×"), "{world}");
    assert!(world.contains("payments: 6 réplicas"), "a injeção não pode alterar payments: {world}");
    assert!(Journal::verify(&arx.journal.entries).is_ok());
    assert!(arx.meter.wall <= c.check.reports[0].worst.wall, "o runtime nunca excede o pior caso estático");
}

#[test]
fn escalates_when_no_plan_passes_the_policy() {
    let c = compile(&read("remediate.ath")).unwrap();
    let bad = script(&["Scale { deploy: \"payments\", to: 0 }", "não é um valor Aletheia", "Restart { deploy: \"db\" }"]);
    let mut arx = Arx::new(&c.module, Box::new(world()), Box::new(bad));
    let out = arx.run("Remediator", "alert", vec![Value::Text("api".into())]).unwrap();
    assert_eq!(out, Value::Text("escalado".into()));
    let k = kinds(&arx.journal);
    assert_eq!(k.iter().filter(|s| *s == "infer").count(), 3);
    assert!(k.contains(&"decode_reject".to_string()), "saída fora do tipo é descartada");
    assert!(k.contains(&"refine_exhausted".to_string()));
    let world = arx.host.summary().join("; ");
    assert!(!world.contains("reiniciado"), "nenhuma ação sem endosso: {world}");
    assert!(world.contains("nenhum plano seguro"), "{world}");
}

#[test]
fn contract_breach_aborts_and_is_journaled() {
    let c = compile(&read("remediate.ath")).unwrap();
    let mut w = json("fixtures/world.json");
    w["latency"]["apply"] = serde_json::json!(3.0); // contrato Δ declara 1,0 s
    let models = Scripted::from_json(&json("fixtures/models.json")).unwrap();
    let mut arx = Arx::new(&c.module, Box::new(SimWorld::from_json(&w).unwrap()), Box::new(models));
    let err = arx.run("Remediator", "alert", vec![Value::Text("api".into())]).unwrap_err();
    assert_eq!(err.code, "R0701");
    let k = kinds(&arx.journal);
    assert_eq!(&k[k.len() - 2..], ["contract_breach", "abort"]);
}

#[test]
fn reservation_refuses_to_start_a_call_that_cannot_be_paid() {
    let m = Meter { lim_wall: Some(5.0), wall: 4.0, ..Meter::default() };
    let e = m.reserve(0, 0.0, 2.0, "apply").unwrap_err();
    assert_eq!(e.code, "R0801");
    assert!(m.reserve(0, 0.0, 1.0, "apply").is_ok());
}

#[test]
fn idempotency_key_never_repeats_an_effect() {
    let mut w = world();
    let restart = Value::Adt { ty: "Action".into(), variant: "Restart".into(), fields: vec![("deploy".into(), Value::Text("api".into()))] };
    let args = vec![Value::Cap(Default::default()), restart, Value::Key(7)];
    w.call("apply", &args).unwrap();
    w.call("apply", &args).unwrap(); // repetição (retry) com a mesma chave
    assert_eq!(w.restarts["api"], 1);
}

#[test]
fn tampering_with_the_journal_is_detected() {
    let c = compile(&read("remediate.ath")).unwrap();
    let models = Scripted::from_json(&json("fixtures/models.json")).unwrap();
    let mut arx = Arx::new(&c.module, Box::new(world()), Box::new(models));
    arx.run("Remediator", "alert", vec![Value::Text("api".into())]).unwrap();
    let mut entries = arx.journal.entries.clone();
    let i = entries.iter().position(|e| e["kind"] == "policy_reject").unwrap();
    entries[i]["kind"] = serde_json::json!("endorse"); // reescrever a história
    assert_eq!(Journal::verify(&entries), Err(i));
}

// ------------------------------------------------------------------ Codex

#[test]
fn codex_hashes_ignore_names() {
    let src = read("remediate.ath");
    let renamed = src.replace("SafeRemediation", "PoliticaSegura").replace("receipt", "recibo");
    let a = compile(&src).unwrap().codex;
    let b = compile(&renamed).unwrap().codex;
    assert_eq!(a.hash_of("SafeRemediation"), b.hash_of("PoliticaSegura"));
    assert_eq!(a.hash_of("Remediator"), b.hash_of("Remediator"), "renomear dependência e local não muda o agente");
    assert_ne!(a.root, b.root, "a raiz liga nomes a hashes, então muda");
}

#[test]
fn codex_hashes_track_semantics() {
    let src = read("remediate.ath");
    let a = compile(&src).unwrap().codex;
    let b = compile(&src.replace("retry 3", "retry 2")).unwrap().codex;
    assert_ne!(a.hash_of("Remediator"), b.hash_of("Remediator"));
    assert_eq!(a.hash_of("SafeRemediation"), b.hash_of("SafeRemediation"));
}

// ------------------------------------------------------------------ CLI

#[test]
fn cli_exit_codes_and_diagnostics() {
    let bin = env!("CARGO_BIN_EXE_athc");
    let ok = Command::new(bin).args(["check"]).arg(examples().join("remediate.ath")).env("NO_COLOR", "1").output().unwrap();
    assert!(ok.status.success());
    assert!(String::from_utf8_lossy(&ok.stdout).contains("E6 orçamento"));

    let bad = Command::new(bin).args(["check"]).arg(examples().join("errors/e0412_injection.ath")).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    let out = String::from_utf8_lossy(&bad.stdout);
    assert!(out.contains("error[E0412]: fluxo de integridade violado"), "{out}");
    assert!(out.contains("^^^^^^^^^^^^^ integridade Untrusted"), "{out}");
}
