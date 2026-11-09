//! Testes ponta a ponta do Marco Zero: compilar, rejeitar, executar, endereçar e auditar.

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

fn world() -> SimWorld {
    SimWorld::from_json(&json("fixtures/world.json")).unwrap()
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

fn kinds(j: &Journal) -> Vec<String> {
    j.entries.iter().map(|e| e["kind"].as_str().unwrap().to_string()).collect()
}

// ------------------------------------------------------------------ compilação

#[test]
fn accepts_remediate_and_proves_budget() {
    let c = compile(&read("remediate.ath")).expect("programa válido deve compilar");
    let r = &c.check.reports[0];
    assert_eq!((r.agent.as_str(), r.event.as_str()), ("Remediator", "alert"));
    // saída ≤ 174 tok, derivada do tipo Action (e não os 400 de max_out):
    // 2 (logs) + 3 × (1,5 + 174 × 0,02) (infer) + 1 (apply) + 1 (page) = 18,94 s
    let ic = c.check.facts.infer_costs.values().next().unwrap();
    assert_eq!((ic.tok_out, ic.out_by_type), (174, true));
    assert!((r.worst.wall - 18.94).abs() < 1e-9, "wall = {}", r.worst.wall);
    assert_eq!(r.worst.tok, 3 * (2030 + 174));
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
        ("e0451_refinement_literal.ath", "E0451", Stratum::E5),
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
    let mut arx = Arx::new(&c.module, Box::new(world()), masked(fixture_models()));
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
    // provedor sem restrição: o ARX valida cada saída contra o tipo
    let bad = script(json!([
        { "kind": "Scale", "deploy": "payments", "to": 0 },
        "não é JSON",
        { "kind": "Restart", "deploy": "db" },
    ]));
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
    let mut arx = Arx::new(&c.module, Box::new(SimWorld::from_json(&w).unwrap()), masked(fixture_models()));
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
    let restart = Value::Adt {
        ty: "Action".into(),
        variant: "Restart".into(),
        fields: vec![("deploy".into(), Value::Text("api".into()))],
    };
    let args = vec![Value::Cap(Default::default()), restart, Value::Key(7)];
    w.call("apply", &args).unwrap();
    w.call("apply", &args).unwrap(); // repetição (retry) com a mesma chave
    assert_eq!(w.restarts["api"], 1);
}

#[test]
fn tampering_with_the_journal_is_detected() {
    let c = compile(&read("remediate.ath")).unwrap();
    let mut arx = Arx::new(&c.module, Box::new(world()), masked(fixture_models()));
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
    let ok =
        Command::new(bin).args(["check"]).arg(examples().join("remediate.ath")).env("NO_COLOR", "1").output().unwrap();
    assert!(ok.status.success());
    assert!(String::from_utf8_lossy(&ok.stdout).contains("E6 orçamento"));

    let bad = Command::new(bin)
        .args(["check"])
        .arg(examples().join("errors/e0412_injection.ath"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(1));
    let out = String::from_utf8_lossy(&bad.stdout);
    assert!(out.contains("error[E0412]: fluxo de integridade violado"), "{out}");
    assert!(out.contains("^^^^^^^^^^^^^ integridade Untrusted"), "{out}");
}

// ------------------------------------------------------------------ decodificação restrita

fn infer_entries(j: &Journal) -> Vec<&serde_json::Value> {
    j.entries.iter().filter(|e| e["kind"] == "infer").map(|e| &e["detail"]).collect()
}

#[test]
fn masked_decoding_forces_the_injected_output_back_into_the_type() {
    let c = compile(&read("remediate.ath")).unwrap();
    let mut arx = Arx::new(&c.module, Box::new(world()), masked(fixture_models()));
    arx.run("Remediator", "alert", vec![Value::Text("api".into())]).unwrap();
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
    let mut raw = Arx::new(&c.module, Box::new(world()), Box::new(script(wants_delete.clone())));
    raw.run("Remediator", "alert", vec![Value::Text("api".into())]).unwrap();
    assert!(infer_entries(&raw.journal)[0]["invalid"].as_str().unwrap().contains("`Delete` não é variante"));

    // com a máscara: o primeiro caractere já é forçado para `R` ou `S`
    let mut m = Arx::new(&c.module, Box::new(world()), masked(wants_delete));
    m.run("Remediator", "alert", vec![Value::Text("api".into())]).unwrap();
    let note = infer_entries(&m.journal)[0]["note"].as_str().unwrap().to_string();
    assert!(note.contains("queria `D`") && note.contains("R | S"), "{note}");
    for a in [&raw, &m] {
        assert!(!a.host.summary().join(";").contains("db"), "nenhum efeito sobre db");
    }
}

#[test]
fn refinement_without_proof_is_rejected() {
    let src = read("remediate.ath").replace(
        "let receipt = apply(cluster, plan, k)",
        "let receipt = apply(cluster, Scale { deploy: \"api\", to: 1 + 1 }, k)",
    );
    let d = &compile(&src).err().unwrap()[0];
    assert_eq!((d.code.as_str(), d.stratum), ("E0450", Stratum::E5));
}

#[test]
fn event_arguments_are_checked_against_their_refinement() {
    let src =
        read("remediate.ath").replace("on alert(deploy: Text ^ Untrusted)", "on alert(deploy: Text[..8] ^ Untrusted)");
    let c = compile(&src).unwrap();
    let mut arx = Arx::new(&c.module, Box::new(world()), masked(fixture_models()));
    let err = arx.run("Remediator", "alert", vec![Value::Text("nome-de-deployment-longo".into())]).unwrap_err();
    assert_eq!(err.code, "R0003");
}

/// Servidor HTTP mínimo que responde uma vez no formato `chat/completions` e devolve a requisição recebida.
fn mock_openai(content: &str) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    let content = content.to_string();
    let handle = std::thread::spawn(move || {
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
            // transfer-encoding: chunked
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
        let reply = json!({
            "choices": [{ "message": { "role": "assistant", "content": content } }],
            "usage": { "prompt_tokens": 121, "completion_tokens": 15 },
        })
        .to_string();
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        );
        sock.write_all(resp.as_bytes()).unwrap();
        format!("{head}{body}")
    });
    (url, handle)
}

#[test]
fn openai_compatible_backend_sends_the_type_as_a_strict_schema() {
    let c = compile(&read("remediate.ath")).unwrap();
    let (url, server) = mock_openai(r#"{"value":{"kind":"Restart","deploy":"api"}}"#);
    let provider =
        OpenAiCompat::new(&url, "qwen2.5-7b-instruct", Some("chave-de-teste".into()), Constraint::JsonSchema);
    let mut arx = Arx::new(&c.module, Box::new(world()), Box::new(provider));
    let out = arx.run("Remediator", "alert", vec![Value::Text("api".into())]).unwrap();
    assert_eq!(out, Value::Text("resolvido".into()));

    let raw = server.join().unwrap();
    assert!(raw.starts_with("POST /v1/chat/completions"), "{raw}");
    assert!(raw.to_ascii_lowercase().contains("authorization: bearer chave-de-teste"));
    let body: serde_json::Value = serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(body["model"], "qwen2.5-7b-instruct");
    assert_eq!(body["max_tokens"], 174, "teto de saída derivado do tipo");
    let schema = &body["response_format"]["json_schema"];
    assert_eq!(schema["strict"], true);
    let scale = &schema["schema"]["properties"]["value"]["anyOf"][1];
    assert_eq!(scale["properties"]["to"]["minimum"], 2);
    assert_eq!(scale["properties"]["kind"]["enum"][0], "Scale");

    // tokens informados pelo servidor entram na liquidação do orçamento
    let inf = infer_entries(&arx.journal)[0];
    assert_eq!((inf["tok_in"].as_u64(), inf["tok_out"].as_u64()), (Some(121), Some(15)));
    assert!(inf["provider"].as_str().unwrap().starts_with("openai-compat"));
}

#[test]
fn gbnf_constraint_ships_the_grammar() {
    let c = compile(&read("remediate.ath")).unwrap();
    let te = ath_syntax::parse_module("module p\ntool p(x: Action) -> Unit ! {}").unwrap();
    let ath_syntax::ast::Item::Tool(t) = &te.items[0] else { unreachable!() };
    let shape = ath_grammar::shape_of(&c.module, &t.params[0].ty).unwrap();
    let p = OpenAiCompat::new("http://127.0.0.1:1/v1", "local", None, Constraint::Gbnf);
    let body = p.body(&Request { model: "planner", prompt: "…", shape: &shape, max_tokens: 174 });
    assert!(body["grammar"].as_str().unwrap().contains("int-2-64 ::="));
    assert!(body.get("response_format").is_none());
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
