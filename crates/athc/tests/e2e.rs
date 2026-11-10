//! Testes ponta a ponta: compilar, rejeitar, executar (inclusive sagas e reconciliação),
//! endereçar, auditar e falar com um servidor compatível com OpenAI.

use arx::host::{Host, SimWorld};
use arx::journal::Journal;
use arx::model::{Constraint, Masked, ModelProvider, OpenAiCompat, Request, Scripted};
use arx::{Arx, Meter, Value};
use ath_check::Stratum;
use athc::compile;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
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

fn world_named(name: &str) -> SimWorld {
    SimWorld::from_json(&json(&format!("fixtures/{name}.json"))).unwrap()
}

fn world() -> SimWorld {
    world_named("world")
}

fn script(responses: serde_json::Value) -> Scripted {
    Scripted::from_json(&json!({ "planner": responses })).unwrap()
}

/// O caminho padrão do `athc run`: o roteiro é a proposta, a máscara do tipo decide.
fn masked(responses: serde_json::Value) -> Box<dyn ModelProvider> {
    Box::new(Masked { inner: Box::new(script(responses)) })
}

fn fixture_models() -> serde_json::Value {
    json("fixtures/models.json")["planner"].clone()
}

fn scale_api() -> serde_json::Value {
    json!([{ "kind": "Scale", "deploy": "api", "to": 6 }])
}

fn restart_api() -> serde_json::Value {
    json!([{ "kind": "Restart", "deploy": "api" }])
}

fn kinds(j: &Journal) -> Vec<String> {
    j.entries.iter().map(|e| e["kind"].as_str().unwrap().to_string()).collect()
}

fn has(j: &Journal, kind: &str) -> bool {
    kinds(j).iter().any(|k| k == kind)
}

fn api() -> Vec<Value> {
    vec![Value::Text("api".into())]
}

/// Executa o agente do exemplo num mundo e com um provedor; devolve o resultado e o runtime.
fn run_in<'m>(
    c: &'m athc::Compiled,
    world: SimWorld,
    provider: Box<dyn ModelProvider>,
) -> (Result<Value, arx::RtError>, Arx<'m>) {
    let mut arx = Arx::new(&c.module, Box::new(world), provider);
    let out = arx.run("Remediator", "alert", api());
    (out, arx)
}

fn text(v: &Value) -> &str {
    match v {
        Value::Text(s) => s,
        other => panic!("esperado texto, veio {}", other.show()),
    }
}

// ------------------------------------------------------------------ compilação

#[test]
fn accepts_remediate_and_proves_budget_including_the_saga_failure_path() {
    let c = compile(&read("remediate.ath")).expect("programa válido deve compilar");
    let r = &c.check.reports[0];
    assert_eq!((r.agent.as_str(), r.event.as_str()), ("Remediator", "alert"));
    // saída ≤ 174 tok do tipo Action + 512 de raciocínio declarados no contrato:
    // infer = 2,0 + 686 × 0,01 = 8,86 s. Pior caminho: o da falha da saga:
    // 2 (logs) + 3 × 8,86 + 1 (scale) + 1 (healthy) + 20 (await) + 1 (unscale) + 1 (restored) + 1 (page)
    let ic = c.check.facts.infer_costs.values().next().unwrap();
    assert_eq!((ic.tok_out, ic.tok_reasoning, ic.out_by_type), (686, 512, true));
    assert!((r.worst.wall - 53.58).abs() < 1e-9, "wall = {}", r.worst.wall);
    // Inventário e telemetria dinâmicos têm tetos finitos: o checker calcula
    // o pior caso do contrato Δ incluindo as três inferências e o fit dos logs.
    assert!(r.worst.tok >= 3 * (686 + 2000), "tok = {}", r.worst.tok);
    assert!(r.worst.tok <= 20_000, "budget = {}", r.worst.tok);
    assert!(r.worst.path().contains("unscale↩"), "{}", r.worst.path());
    let effs: Vec<&str> = c.check.facts.effects.iter().map(String::as_str).collect();
    assert_eq!(effs, ["Cluster.Read", "Cluster.Write", "CompensationFailed", "Infer", "Page", "TimeoutUnknown"]);
    let comps: Vec<_> = c.check.facts.compensations.iter().collect();
    assert_eq!(comps, [&("scale".to_string(), "unscale".to_string())]);
}

#[test]
fn rejects_each_error_example_in_the_expected_stratum() {
    let cases = [
        ("e0412_injection.ath", "E0412", Stratum::E3),
        ("e0412_implicit_flow.ath", "E0412", Stratum::E3),
        ("e0415_prompt_injection.ath", "E0415", Stratum::E3),
        ("e0417_untrusted_postcondition.ath", "E0417", Stratum::E3),
        ("e1104_key_reuse.ath", "E1104", Stratum::E1),
        ("e0042_own_missing.ath", "E0042", Stratum::E1),
        ("e1107_pending_dropped.ath", "E1107", Stratum::E1),
        ("e0502_capability.ath", "E0502", Stratum::E2),
        ("e0501_effect_row.ath", "E0501", Stratum::E2),
        ("t0301_commit.ath", "T0301", Stratum::E2),
        ("t0302_irrevocable_in_atomic.ath", "T0302", Stratum::E2),
        ("e0503_compensation_unhandled.ath", "E0503", Stratum::E2),
        ("e0503_timeout_unhandled.ath", "E0503", Stratum::E2),
        ("e0505_compensator_signature.ath", "E0505", Stratum::E2),
        ("e0451_refinement_literal.ath", "E0451", Stratum::E5),
        ("b0110_unbounded_prompt.ath", "B0110", Stratum::E6),
        ("b0207_wall_budget.ath", "B0207", Stratum::E6),
        ("e0452_list_bounds.ath", "E0452", Stratum::E5),
        ("e0450_policy_domain.ath", "E0450", Stratum::E5),
        ("e1104_key_in_loop.ath", "E1104", Stratum::E1),
        ("e0412_untrusted_iteration.ath", "E0412", Stratum::E3),
        ("b0302_overhead_schema.ath", "B0302", Stratum::E6),
        ("b0207_loop_budget.ath", "B0207", Stratum::E6),
        ("e0030_contract_without_id.ath", "E0030", Stratum::E0),
    ];
    let on_disk = std::fs::read_dir(examples().join("errors"))
        .unwrap()
        .filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "ath"));
    assert_eq!(on_disk.count(), cases.len(), "toda variante em examples/errors precisa de um caso aqui");
    for (file, code, stratum) in cases {
        let diags = match compile(&read(&format!("errors/{file}"))) {
            Ok(_) => panic!("{file}: deveria ser rejeitado"),
            Err(d) => d,
        };
        assert_eq!(diags[0].code, code, "{file}: {}", diags[0].msg);
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
    // o prompt cita o evento (`alerta = quote(service)`) e os logs, ambos Untrusted: a cadeia
    // de origem parte do primeiro deles e atravessa o `infer` e a amostra crua
    let d = &compile(&read("errors/e0412_injection.ath")).err().unwrap()[0];
    assert!(d.notes[0].starts_with("origem: evento `alert(service)` ^ Untrusted"), "{:?}", d.notes);
    assert!(d.notes[0].ends_with("→ infer planner → sample(…)"), "{:?}", d.notes);
}

#[test]
fn untrusted_postcondition_names_the_event() {
    let d = &compile(&read("errors/e0417_untrusted_postcondition.ath")).err().unwrap()[0];
    assert!(d.notes[0].contains("evento `alert(service)` ^ Untrusted → healthy(…)"), "{:?}", d.notes);
}

// ------------------------------------------------------------------ execução

#[test]
fn runs_remediate_end_to_end_and_rejects_the_injected_plan() {
    let c = compile(&read("remediate.ath")).unwrap();
    let (out, arx) = run_in(&c, world(), masked(fixture_models()));
    assert_eq!(out.unwrap(), Value::Text("resolvido".into()));

    let k = kinds(&arx.journal);
    let reject = k.iter().position(|s| s == "policy_reject").expect("a 1ª proposta deve ser rejeitada");
    let endorse = k.iter().position(|s| s == "endorse").expect("a 2ª proposta deve ser endossada");
    assert!(reject < endorse);
    assert_eq!(arx.journal.entries[reject]["detail"]["value"]["Scale"]["deploy"], "payments");
    assert!(has(&arx.journal, "saga_commit"));

    let world = arx.host.summary().join("; ");
    assert!(world.contains("api: 6 réplicas"), "{world}");
    assert!(world.contains("payments: 6 réplicas"), "a injeção não pode alterar payments: {world}");
    assert!(Journal::verify(&arx.journal.entries).is_ok());
    assert!(arx.meter.wall <= c.check.reports[0].worst.wall, "o runtime nunca excede o pior caso estático");
}

#[test]
fn escalates_when_no_plan_passes_the_policy() {
    let c = compile(&read("remediate.ath")).unwrap();
    // provedor sem restrição: o ARX valida cada saída contra o tipo
    let bad = script(json!([
        { "kind": "Scale", "deploy": "payments", "to": 0 },
        "não é JSON",
        { "kind": "Restart", "deploy": "db" },
    ]));
    let (out, arx) = run_in(&c, world(), Box::new(bad));
    assert_eq!(out.unwrap(), Value::Text("escalado".into()));
    let k = kinds(&arx.journal);
    assert_eq!(k.iter().filter(|s| *s == "infer").count(), 3);
    assert!(k.contains(&"refine_exhausted".to_string()));
    let world = arx.host.summary().join("; ");
    assert!(!world.contains("reiniciado"), "nenhuma ação sem endosso: {world}");
    assert!(world.contains("nenhum plano seguro"), "{world}");
}

#[test]
fn contract_breach_outside_a_saga_aborts_and_is_journaled() {
    let c = compile(&read("remediate.ath")).unwrap();
    let mut w = json("fixtures/world.json");
    w["latency"]["restart"] = json!(3.0); // o contrato Δ declara 1,0 s
    let (out, arx) = run_in(&c, SimWorld::from_json(&w).unwrap(), masked(restart_api()));
    assert_eq!(out.unwrap_err().code, "R0701");
    let k = kinds(&arx.journal);
    assert_eq!(&k[k.len() - 2..], ["contract_breach", "abort"]);
}

#[test]
fn contract_breach_inside_a_saga_is_compensated() {
    let c = compile(&read("remediate.ath")).unwrap();
    let mut w = json("fixtures/world.json");
    w["latency"]["scale"] = json!(3.0);
    let (out, arx) = run_in(&c, SimWorld::from_json(&w).unwrap(), masked(scale_api()));
    let r = out.unwrap();
    assert!(text(&r).starts_with("compensado: R0701 ContractBreach"), "{}", r.show());
    assert!(has(&arx.journal, "saga_recovered"));
    assert!(arx.host.summary().join(";").contains("api: 3 réplicas"));
}

#[test]
fn saga_compensates_when_the_postcondition_is_not_observed() {
    let c = compile(&read("remediate.ath")).unwrap();
    let (out, arx) = run_in(&c, world_named("world-saga-compensa"), masked(scale_api()));
    let r = out.unwrap();
    assert!(text(&r).starts_with("compensado: R0910"), "{}", r.show());
    let k = kinds(&arx.journal);
    let abort = k.iter().position(|s| s == "saga_abort").unwrap();
    let comp = k.iter().position(|s| s == "compensate").unwrap();
    let rec = k.iter().position(|s| s == "saga_recovered").unwrap();
    assert!(abort < comp && comp < rec);
    let e = &arx.journal.entries[comp]["detail"];
    assert_eq!((e["of"].as_str(), e["tool"].as_str(), e["key"].as_str()), (Some("scale"), Some("unscale"), Some("k1")));
    assert!(arx.host.summary().join(";").contains("api: 3 réplicas"), "réplicas restauradas à pré-imagem");
}

#[test]
fn compensation_failure_reaches_its_handler() {
    let c = compile(&read("remediate.ath")).unwrap();
    let (out, arx) = run_in(&c, world_named("world-saga-falha"), masked(scale_api()));
    let r = out.unwrap();
    assert!(text(&r).starts_with("intervenção humana"), "{}", r.show());
    assert!(has(&arx.journal, "compensation_failed"));
    assert!(!has(&arx.journal, "saga_recovered"));
    let world = arx.host.summary().join(";");
    assert!(world.contains("api: 6 réplicas") && world.contains("intervenção humana"), "{world}");
}

#[test]
fn timeout_unknown_is_reconciled_by_key_without_repeating_the_effect() {
    let c = compile(&read("remediate.ath")).unwrap();
    let (out, arx) = run_in(&c, world_named("world-timeout-reconcilia"), masked(scale_api()));
    assert_eq!(out.unwrap(), Value::Text("resolvido".into()));
    let k = kinds(&arx.journal);
    let t = k.iter().position(|s| s == "timeout_unknown").unwrap();
    let r = k.iter().position(|s| s == "resolved").unwrap();
    assert!(t < r);
    assert_eq!(arx.journal.entries[r]["detail"]["status"], "done");
    assert_eq!(arx.journal.entries[t + 1]["detail"]["provider"], "queryable");
    assert!(arx.host.summary().join(";").contains("api: 6 réplicas"));
}

#[test]
fn idempotent_provider_reconciles_by_resending_the_same_key() {
    let c = compile(&read("remediate.ath")).unwrap();
    let mut w = json("fixtures/world.json");
    w["faults"] = json!([{ "tool": "restart", "nth": 1, "kind": "timeout_lost" }]);
    let (out, arx) = run_in(&c, SimWorld::from_json(&w).unwrap(), masked(restart_api()));
    assert_eq!(out.unwrap(), Value::Text("resolvido".into()));
    let rec = arx.journal.entries.iter().find(|e| e["kind"] == "reconcile").unwrap();
    assert_eq!(rec["detail"]["provider"], "idempotent");
    assert!(arx.host.summary().join(";").contains("api reiniciado 1×"), "reenvio com a mesma chave aplica uma vez");
}

#[test]
fn exhausted_reconciliation_transfers_the_obligation_and_the_saga_compensates() {
    let c = compile(&read("remediate.ath")).unwrap();
    let (out, arx) = run_in(&c, world_named("world-timeout-transfere"), masked(scale_api()));
    assert_eq!(out.unwrap(), Value::Text("pendente".into()));
    let k = kinds(&arx.journal);
    assert_eq!(k.iter().filter(|s| *s == "reconcile").count(), 3, "fuel 3");
    let order: Vec<usize> = ["reconcile_exhausted", "transfer", "saga_abort", "compensate", "saga_recovered"]
        .iter()
        .map(|x| k.iter().position(|s| s == x).unwrap_or_else(|| panic!("{x} ausente")))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    let end = arx.journal.entries.last().unwrap();
    assert_eq!(end["detail"]["aborted_by_handler"], true);
}

#[test]
fn reservation_refuses_to_start_a_call_that_cannot_be_paid() {
    let m = Meter { lim_wall: Some(5.0), wall: 4.0, ..Meter::default() };
    let e = m.reserve(0, 0.0, 2.0, "restart").unwrap_err();
    assert_eq!(e.code, "R0801");
    assert!(m.reserve(0, 0.0, 1.0, "restart").is_ok());
}

#[test]
fn idempotency_key_never_repeats_an_effect() {
    let mut w = world();
    let args = vec![Value::Cap(Default::default()), Value::Text("api".into()), Value::Key(7)];
    w.call("restart", &args).unwrap();
    w.call("restart", &args).unwrap(); // repetição (retry) com a mesma chave
    assert_eq!(w.restarts["api"], 1);
}

#[test]
fn tampering_with_the_journal_is_detected() {
    let c = compile(&read("remediate.ath")).unwrap();
    let (_, arx) = run_in(&c, world(), masked(fixture_models()));
    let mut entries = arx.journal.entries.clone();
    let i = entries.iter().position(|e| e["kind"] == "policy_reject").unwrap();
    entries[i]["kind"] = json!("endorse"); // reescrever a história
    assert_eq!(Journal::verify(&entries), Err(i));
}

#[test]
fn verify_journal_enforces_t9_no_open_keys() {
    let dir = std::env::temp_dir().join(format!("ath-t9-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let verify = |j: &Journal, name: &str| {
        let p = dir.join(name);
        std::fs::write(&p, j.to_jsonl()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_athc")).arg("verify-journal").arg(&p).env("NO_COLOR", "1").output().unwrap()
    };
    let mut j = Journal::default();
    j.push(0.0, "mint", json!({ "key": "k1" }));
    j.push(1.0, "timeout_unknown", json!({ "tool": "scale", "key": "k1" }));
    let open = verify(&j, "open.jsonl");
    assert_eq!(open.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&open.stdout).contains("em aberto sem destino: k1"));
    j.push(2.0, "transfer", json!({ "tool": "scale", "key": "k1" }));
    let closed = verify(&j, "closed.jsonl");
    assert!(closed.status.success(), "{}", String::from_utf8_lossy(&closed.stdout));
}

// ------------------------------------------------------------------ Codex

#[test]
fn codex_hashes_ignore_names() {
    let src = read("remediate.ath");
    let renamed = src
        .replace("SafeRemediation", "PoliticaSegura")
        .replace("let r =", "let resultado =")
        .replace("\" · \" ++ r,", "\" · \" ++ resultado,");
    let renamed = renamed.replace("        r\n      },", "        resultado\n      },");
    let a = compile(&src).unwrap().codex;
    let b = compile(&renamed).unwrap().codex;
    assert_eq!(a.hash_of("SafeRemediation"), b.hash_of("PoliticaSegura"));
    assert_eq!(a.hash_of("Remediator"), b.hash_of("Remediator"), "renomear dependência e local não muda o agente");
    assert_ne!(a.root, b.root, "a raiz liga nomes a hashes, então muda");
}

#[test]
fn codex_hashes_track_semantics_and_compensators() {
    let src = read("remediate.ath");
    let a = compile(&src).unwrap().codex;
    let b = compile(&src.replace("retry 3", "retry 2")).unwrap().codex;
    assert_ne!(a.hash_of("Remediator"), b.hash_of("Remediator"));
    assert_eq!(a.hash_of("SafeRemediation"), b.hash_of("SafeRemediation"));
    // o compensador faz parte da identidade da ferramenta
    let c = compile(&src.replace(
        "! { Cluster.Write } reversible provider idempotent latency 1.0\n\n-- reiniciar",
        "! { Cluster.Write } reversible provider idempotent latency 2.0\n\n-- reiniciar",
    ))
    .unwrap()
    .codex;
    assert_ne!(a.hash_of("unscale"), c.hash_of("unscale"));
    assert_ne!(a.hash_of("scale"), c.hash_of("scale"), "scale referencia unscale pelo hash");
}

// ------------------------------------------------------------------ decodificação restrita

fn infer_entries(j: &Journal) -> Vec<&serde_json::Value> {
    j.entries.iter().filter(|e| e["kind"] == "infer").map(|e| &e["detail"]).collect()
}

#[test]
fn masked_decoding_forces_the_injected_output_back_into_the_type() {
    let c = compile(&read("remediate.ath")).unwrap();
    let (_, arx) = run_in(&c, world(), masked(fixture_models()));
    let first = infer_entries(&arx.journal)[0];
    assert_eq!(first["output"], r#"{"value":{"kind":"Scale","deploy":"payments","to":10}}"#);
    assert!(first["note"].as_str().unwrap().contains("o modelo queria `0`"), "{}", first["note"]);
    assert!(first["invalid"].is_null(), "saída mascarada é sempre válida");
}

#[test]
fn a_delete_cannot_even_be_emitted() {
    let c = compile(&read("remediate.ath")).unwrap();
    let wants_delete = json!([{ "kind": "Delete", "deploy": "db" }, { "kind": "Restart", "deploy": "api" }]);

    // sem restrição: a saída chega, é rejeitada como fora do tipo e não executa nada
    let (_, raw) = run_in(&c, world(), Box::new(script(wants_delete.clone())));
    assert!(infer_entries(&raw.journal)[0]["invalid"].as_str().unwrap().contains("`Delete` não é variante"));

    // com a máscara: o primeiro caractere já é forçado para `R` ou `S`
    let (_, m) = run_in(&c, world(), masked(wants_delete));
    let note = infer_entries(&m.journal)[0]["note"].as_str().unwrap().to_string();
    assert!(note.contains("queria `D`") && note.contains("R | S"), "{note}");
    for a in [&raw, &m] {
        assert!(!a.host.summary().join(";").contains("db"), "nenhum efeito sobre db");
    }
}

#[test]
fn refinement_without_proof_is_rejected() {
    let src = read("remediate.ath")
        .replace("scale(cluster, deploy, to, keys.mint())", "scale(cluster, deploy, 1 + 1, keys.mint())");
    let d = &compile(&src).err().unwrap()[0];
    assert_eq!((d.code.as_str(), d.stratum), ("E0450", Stratum::E5));
}

#[test]
fn event_arguments_are_checked_against_their_refinement() {
    let c = compile(&read("remediate.ath")).unwrap();
    let mut arx = Arx::new(&c.module, Box::new(world()), masked(fixture_models()));
    let err = arx.run("Remediator", "alert", vec![Value::Text("um-nome-de-servico-com-mais-de-32-caracteres".into())]);
    assert_eq!(err.unwrap_err().code, "R0003");
}

// ------------------------------------------------------------------ provedor HTTP compatível com OpenAI

/// Servidor HTTP mínimo: responde, em ordem, a cada conexão com (status, cabeçalhos extras, corpo)
/// e devolve as requisições recebidas.
fn mock_openai(replies: Vec<(u16, &'static str, String)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for (status, headers, reply) in replies {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 8192];
            let head_end = loop {
                let n = sock.read(&mut tmp).unwrap();
                buf.extend_from_slice(&tmp[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let lower = head.to_ascii_lowercase();
            let body = if let Some(len) = lower.lines().find_map(|l| l.strip_prefix("content-length:")) {
                let len: usize = len.trim().parse().unwrap();
                while buf.len() < head_end + len {
                    let n = sock.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                }
                String::from_utf8_lossy(&buf[head_end..head_end + len]).to_string()
            } else {
                while !buf.ends_with(b"0\r\n\r\n") {
                    let n = sock.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                }
                let mut rest = &buf[head_end..];
                let mut out = Vec::new();
                loop {
                    let nl = rest.windows(2).position(|w| w == b"\r\n").unwrap();
                    let size = usize::from_str_radix(std::str::from_utf8(&rest[..nl]).unwrap().trim(), 16).unwrap();
                    if size == 0 {
                        break;
                    }
                    out.extend_from_slice(&rest[nl + 2..nl + 2 + size]);
                    rest = &rest[nl + 2 + size + 2..];
                }
                String::from_utf8_lossy(&out).to_string()
            };
            let resp = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            sock.write_all(resp.as_bytes()).unwrap();
            seen.push(format!("{head}{body}"));
        }
        seen
    });
    (url, handle)
}

fn chat_reply(content: &str) -> String {
    json!({
        "id": "chatcmpl-teste-1",
        "choices": [{
            "message": { "role": "assistant", "content": content, "reasoning": "O log mostra o pool esgotado; reiniciar a api." },
            "finish_reason": "stop",
        }],
        "usage": { "prompt_tokens": 121, "completion_tokens": 64, "completion_tokens_details": { "reasoning_tokens": 49 } },
    })
    .to_string()
}

#[test]
fn openai_compatible_backend_sends_the_type_as_a_strict_core_schema_and_survives_a_429() {
    let c = compile(&read("remediate.ath")).unwrap();
    let rate_limited =
        r#"{"error":{"message":"Rate limit reached ... Please try again in 0.2s.","code":"rate_limit_exceeded"}}"#;
    let (url, server) = mock_openai(vec![
        (429, "retry-after: 0\r\n", rate_limited.to_string()),
        (200, "x-request-id: req_cabecalho\r\n", chat_reply(r#"{"value":{"kind":"Restart","deploy":"api"}}"#)),
    ]);
    let provider =
        OpenAiCompat::new(&url, "openai/gpt-oss-120b", Some("chave-de-teste".into()), Constraint::JsonSchema);
    let (out, arx) = run_in(&c, world(), Box::new(provider));
    assert_eq!(out.unwrap(), Value::Text("resolvido".into()));

    let raw = server.join().unwrap();
    assert_eq!(raw.len(), 2, "uma nova tentativa após o 429");
    assert!(raw[1].starts_with("POST /v1/chat/completions"), "{}", raw[1]);
    assert!(raw[1].to_ascii_lowercase().contains("authorization: bearer chave-de-teste"));
    let body: serde_json::Value = serde_json::from_str(&raw[1][raw[1].find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(body["model"], "openai/gpt-oss-120b");
    assert_eq!(body["max_completion_tokens"], 174 + 512, "teto do tipo + raciocínio do contrato");
    assert_eq!(body["reasoning_effort"], "low");
    let schema = &body["response_format"]["json_schema"];
    assert_eq!(schema["strict"], true);
    let scale = &schema["schema"]["properties"]["value"]["anyOf"][1];
    assert_eq!(scale["properties"]["kind"]["enum"][0], "Scale");
    assert_eq!(scale["properties"]["to"]["enum"].as_array().unwrap().len(), 63, "2..64 como enum exato");
    assert!(scale["properties"]["to"].get("minimum").is_none());

    // tokens, latência e identificador do servidor entram no journal e na liquidação
    let inf = infer_entries(&arx.journal)[0];
    assert_eq!((inf["tok_in"].as_u64(), inf["tok_out"].as_u64()), (Some(121), Some(64)));
    assert_eq!(inf["tok_reasoning"], 49);
    assert_eq!(inf["tok_source"], "server");
    assert_eq!(inf["retries"], 1);
    assert!(inf["queued_s"].as_f64().unwrap() >= 0.5);
    assert!(inf["latency_s"].as_f64().unwrap() > 0.0);
    assert_eq!(inf["request_id"], "chatcmpl-teste-1");
    assert!(inf["reasoning"].as_str().unwrap().contains("pool esgotado"));
    assert!(inf["provider"].as_str().unwrap().starts_with("openai-compat(openai/gpt-oss-120b"));
}

/// O que a Groq fez na primeira execução real (gpt-oss-20b, modo estrito): o modelo obedeceu à
/// injeção e gerou `to: 0`; o servidor não restringiu a decodificação ao enum, mas validou depois
/// e respondeu 400 `json_validate_failed` com a geração em `failed_generation`. O ARX valida de
/// novo, descarta, esgota o `refine` e escala; sem `usage`, os tokens são estimados.
#[test]
fn server_side_schema_rejection_is_an_invalid_sample_with_estimated_tokens() {
    let c = compile(&read("remediate.ath")).unwrap();
    let failed = json!({"error": {
        "message": "Generated JSON does not match the expected schema. Please adjust your prompt. See 'failed_generation' for more details.",
        "type": "invalid_request_error",
        "code": "json_validate_failed",
        "failed_generation": r#"{"value":{"kind":"Scale","deploy":"payments","to":0}}"#,
    }})
    .to_string();
    let (url, server) = mock_openai(
        (0..3).map(|i| (400, if i == 0 { "x-request-id: req_400\r\n" } else { "" }, failed.clone())).collect(),
    );
    let provider = OpenAiCompat::new(&url, "openai/gpt-oss-20b", None, Constraint::JsonSchema);
    let (out, arx) = run_in(&c, world(), Box::new(provider));
    assert_eq!(server.join().unwrap().len(), 3, "uma requisição por tentativa do refine");
    assert_eq!(out.unwrap(), Value::Text("escalado".into()));
    let infers = infer_entries(&arx.journal);
    assert_eq!(infers.len(), 3);
    for inf in &infers {
        assert_eq!(inf["note"], "servidor: json_validate_failed");
        assert_eq!(inf["tok_source"], "estimate");
        assert!(inf["invalid"].as_str().unwrap().contains("fora do refinamento [2..64]"), "{inf}");
    }
    assert_eq!(infers[0]["request_id"], "req_400");
    assert!(has(&arx.journal, "refine_exhausted"));
    assert!(arx.journal.entries.iter().all(|e| e["detail"]["tool"] != "scale"), "nenhum scale executado");
}

#[test]
fn reasoning_beyond_the_contract_is_a_contract_breach() {
    let c = compile(&read("remediate.ath")).unwrap();
    let over = json!({
        "id": "chatcmpl-x",
        "choices": [{ "message": { "content": r#"{"value":{"kind":"Restart","deploy":"api"}}"# }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 121, "completion_tokens": 900 },
    });
    let (url, server) = mock_openai(vec![(200, "", over.to_string())]);
    let provider = OpenAiCompat::new(&url, "m", None, Constraint::JsonSchema);
    let (out, arx) = run_in(&c, world(), Box::new(provider));
    server.join().unwrap();
    assert_eq!(out.unwrap_err().code, "R0702");
    let breach = arx.journal.entries.iter().find(|e| e["kind"] == "contract_breach").unwrap();
    assert_eq!(breach["detail"]["code"], "R0702");
    assert_eq!(breach["detail"]["request_id"], "chatcmpl-x");
    assert_eq!(breach["detail"]["observed_tokens"], 900);
    // a chamada aconteceu e custou: o consumo entra no orçamento antes do veredito
    assert_eq!(arx.meter.tok, 121 + 900);
}

#[test]
fn provider_is_configured_from_the_environment() {
    // único teste que lê estas variáveis; os demais constroem o provedor diretamente
    std::env::set_var("OPENAI_BASE_URL", "https://api.groq.com/openai/v1");
    std::env::set_var("ALETHEIA_MODEL", "openai/gpt-oss-20b");
    std::env::set_var("ALETHEIA_REASONING_EFFORT", "medium");
    std::env::set_var("ALETHEIA_EXTRA_BODY", r#"{"temperature": null, "seed": 7}"#);
    let p = OpenAiCompat::from_env(None, None).unwrap();
    assert_eq!((p.base_url.as_str(), p.model_id.as_str()), ("https://api.groq.com/openai/v1", "openai/gpt-oss-20b"));
    let over = OpenAiCompat::from_env(Some("openai/gpt-oss-120b"), None).unwrap();
    assert_eq!(over.model_id, "openai/gpt-oss-120b", "--model tem precedência sobre ALETHEIA_MODEL");

    let c = compile(&read("remediate.ath")).unwrap();
    let te = ath_syntax::parse_module("module p\ntool p(x: Action) -> Unit ! {}").unwrap();
    let ath_syntax::ast::Item::Tool(t) = &te.items[0] else { unreachable!() };
    let shape = ath_grammar::shape_of(&c.module, &t.params[0].ty).unwrap();
    let body = p.body(&Request { model: "planner", prompt: "…", shape: &shape, max_tokens: 174, reasoning: 512 });
    assert_eq!(body["reasoning_effort"], "medium");
    assert_eq!(body["seed"], 7);
    assert!(body.get("temperature").is_none(), "`null` em ALETHEIA_EXTRA_BODY remove o campo");
    for k in ["OPENAI_BASE_URL", "ALETHEIA_MODEL", "ALETHEIA_REASONING_EFFORT", "ALETHEIA_EXTRA_BODY"] {
        std::env::remove_var(k);
    }
}

#[test]
fn gbnf_constraint_ships_the_grammar() {
    let c = compile(&read("remediate.ath")).unwrap();
    let te = ath_syntax::parse_module("module p\ntool p(x: Action) -> Unit ! {}").unwrap();
    let ath_syntax::ast::Item::Tool(t) = &te.items[0] else { unreachable!() };
    let shape = ath_grammar::shape_of(&c.module, &t.params[0].ty).unwrap();
    let p = OpenAiCompat::new("http://127.0.0.1:1/v1", "local", None, Constraint::Gbnf);
    let body = p.body(&Request { model: "planner", prompt: "…", shape: &shape, max_tokens: 174, reasoning: 0 });
    assert!(body["grammar"].as_str().unwrap().contains("int-2-64 ::="));
    assert_eq!(body["max_tokens"], 174);
    assert!(body.get("response_format").is_none() && body.get("reasoning_effort").is_none());
}

// ------------------------------------------------------------------ CLI

#[test]
fn cli_exit_codes_and_diagnostics() {
    let bin = env!("CARGO_BIN_EXE_athc");
    let ok =
        Command::new(bin).args(["check"]).arg(examples().join("remediate.ath")).env("NO_COLOR", "1").output().unwrap();
    assert!(ok.status.success());
    let s = String::from_utf8_lossy(&ok.stdout);
    assert!(s.contains("E6 orçamento") && s.contains("scale ↩ unscale"), "{s}");

    let bad = Command::new(bin)
        .args(["check"])
        .arg(examples().join("errors/e0412_injection.ath"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(1));
    let out = String::from_utf8_lossy(&bad.stdout);
    assert!(out.contains("error[E0412]: fluxo de integridade violado"), "{out}");
    assert!(out.contains("^^^^^^ integridade Untrusted"), "{out}");
}

#[test]
fn grammar_command_prints_schema_gbnf_and_mask_trace() {
    let out = Command::new(env!("CARGO_BIN_EXE_athc"))
        .args(["grammar"])
        .arg(examples().join("remediate.ath"))
        .args(["--type", "Action", "--decode", r#"{"value":{"kind":"Delete","deploy":"db"}}"#])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("\"anyOf\""), "{s}");
    assert!(s.contains("action-scale ::="), "{s}");
    assert!(s.contains("teto da codificação: 174 bytes"), "{s}");
    assert!(s.contains("o tipo só admitia R | S"), "{s}");
}

// ------------------------------------------------------------------ deriva do contrato Δ

fn rollout_reply(model: &str, prompt_tokens: u64) -> String {
    json!({
        "id": "chatcmpl-deriva",
        "model": model,
        "choices": [{
            "message": { "role": "assistant", "content": r#"{"value":[{"kind":"Restart","deploy":"api"}]}"# },
            "finish_reason": "stop",
        }],
        "usage": { "prompt_tokens": prompt_tokens, "completion_tokens": 40, "completion_tokens_details": { "reasoning_tokens": 14 } },
    })
    .to_string()
}

fn run_rollout_against(reply: String) -> (Result<Value, arx::RtError>, Vec<serde_json::Value>) {
    let c = compile(&read("rollout.ath")).unwrap();
    let (url, server) = mock_openai(vec![(200, "", reply)]);
    let provider = OpenAiCompat::new(&url, "openai/gpt-oss-20b", None, Constraint::JsonSchema);
    let mut arx = Arx::new(&c.module, Box::new(world()), Box::new(provider));
    arx.contracts = athc::contract_hashes(&c);
    let out = arx.run("Rollout", "alert", api());
    server.join().unwrap();
    (out, arx.journal.entries.clone())
}

#[test]
fn contract_drift_served_model_is_r0704() {
    let (out, j) = run_rollout_against(rollout_reply("openai/gpt-oss-120b", 700));
    let e = out.unwrap_err();
    assert_eq!(e.code, "R0704");
    assert!(
        e.msg.contains("serviu `openai/gpt-oss-120b`; o contrato de `planner` é `groq:openai/gpt-oss-20b@2026-11-09`"),
        "{}",
        e.msg
    );
    let b = j.iter().find(|x| x["kind"] == "contract_breach").unwrap();
    assert_eq!(
        (b["detail"]["code"].as_str(), b["detail"]["served_model"].as_str()),
        (Some("R0704"), Some("openai/gpt-oss-120b"))
    );
    assert_eq!(b["detail"]["contract"]["version"], "2026-11-09");
    assert!(
        !j.iter().any(|x| x["kind"] == "tool" && x["detail"]["tool"] == "restart"),
        "nenhum efeito depois da deriva"
    );
}

#[test]
fn contract_drift_input_above_prompt_plus_overhead_is_r0705() {
    let (out, j) = run_rollout_against(rollout_reply("openai/gpt-oss-20b", 99_999));
    let e = out.unwrap_err();
    assert_eq!(e.code, "R0705");
    let b = j.iter().find(|x| x["kind"] == "contract_breach").unwrap();
    let bound = b["detail"]["tok_in_contract"].as_u64().unwrap();
    assert!(e.msg.contains(&format!("recebeu 99999 tokens de entrada; o contrato admite {bound}")), "{}", e.msg);
    assert!(e.msg.contains("overhead 768"), "{}", e.msg);
    // a amostra que violou o contrato foi cobrada
    let end_or_abort = j.last().unwrap();
    assert_eq!(end_or_abort["kind"], "abort");
}

#[test]
fn contract_within_bounds_runs_and_records_identity() {
    let (out, j) = run_rollout_against(rollout_reply("openai/gpt-oss-20b", 700));
    assert_eq!(out.unwrap(), Value::Text("concluído".into()));
    let inf = j.iter().find(|x| x["kind"] == "infer").unwrap();
    assert_eq!(inf["detail"]["served_model"], "openai/gpt-oss-20b");
    assert_eq!(inf["detail"]["tok_source"], "server");
    assert_eq!(inf["detail"]["tok_in"], 700);
    assert!(inf["detail"]["tok_in_contract"].as_u64().unwrap() >= 700);
}
