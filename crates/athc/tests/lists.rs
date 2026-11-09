//! Listas delimitadas, laços com custo multiplicativo (E6), obrigações de tamanho (E5),
//! linearidade em laços (E1), fluxo implícito por iteração (E3) e o contrato Δ estendido.

use arx::host::SimWorld;
use arx::model::Scripted;
use arx::{Arx, Value};
use ath_check::Stratum;
use athc::compile;
use serde_json::json;
use std::path::PathBuf;

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(examples().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn rollout() -> String {
    read("rollout.ath")
}

/// Compila e devolve (estrato, códigos) dos diagnósticos; pânico se o programa for aceito.
fn rejected(src: &str) -> (Stratum, Vec<String>) {
    match compile(src) {
        Ok(_) => panic!("programa aceito, mas deveria ser rejeitado:\n{src}"),
        Err(d) => (d[0].stratum, d.iter().map(|x| x.code.clone()).collect()),
    }
}

fn accepted(src: &str) -> athc::Compiled {
    match compile(src) {
        Ok(c) => c,
        Err(d) => {
            panic!("rejeitado: {:?}", d.iter().map(|x| format!("{} {} {}", x.code, x.msg, x.label)).collect::<Vec<_>>())
        }
    }
}

fn swap(src: &str, from: &str, to: &str) -> String {
    assert!(src.contains(from), "trecho ausente: {from}");
    src.replacen(from, to, 1)
}

// ------------------------------------------------------------------ E6: β(laço) ⪯ teto · β(corpo)

#[test]
fn e6_multiplies_loop_ceilings_including_nested_loops() {
    let c = accepted(&rollout());
    let r = &c.check.reports[0];
    let ic = c.check.facts.infer_costs.values().next().unwrap();
    assert_eq!((ic.out_ty.as_str(), ic.tok_out, ic.overhead), ("List[Action, 1..3]", 506 + 512, 768));
    // por amostra: 2,0 + 1018 × 0,01 = 12,18 s; o corpo do laço externo, no pior ramo (falha da
    // saga), é scale 1 + healthy 1 + await 10 + unscale↩ 1 + restored 1 + 2 × page 1 = 16 s
    let infer = 2.0 + 1018.0 * 0.01;
    let expected = 2.0 + 2.0 * infer + 3.0 * (1.0 + 1.0 + 10.0 + 1.0 + 1.0 + 2.0 * 1.0);
    assert!((r.worst.wall - expected).abs() < 1e-9, "wall = {}, esperado {expected}", r.worst.wall);
    assert_eq!(r.worst.tok, 2 * (ic.tok_in + ic.tok_out));
    let path = r.worst.path();
    assert!(path.contains("for a ×3 ⟨scale 1,0 → healthy 1,0 → await 10,0 → unscale↩ 1,0"), "{path}");
    assert!(path.contains("for ch ×2 ⟨page 1,0⟩ 2,0⟩ 48,0"), "{path}");

    // aumentar o teto do laço interno multiplica pelo externo: +1 canal = +3 × 1 s
    let three = accepted(&swap(&rollout(), r#"["sre", "dev"]"#, r#"["sre", "dev", "ops"]"#));
    assert!((three.check.reports[0].worst.wall - (expected + 3.0)).abs() < 1e-9);
    // teto 1 na lista do modelo: o laço externo roda uma vez e o teto de saída do tipo cai
    // para 10 + 2 + 164 = 176 bytes (a mesma lista limita o custo do laço e o da amostra)
    let one = accepted(&rollout().replace("List[Action, 1..3]", "List[Action, 1..1]"));
    let w1 = one.check.reports[0].worst.wall;
    let infer1 = 2.0 + (176.0 + 512.0) * 0.01;
    assert!((w1 - (2.0 + 2.0 * infer1 + 16.0)).abs() < 1e-9, "{w1}");
}

#[test]
fn e5_the_policy_domain_must_contain_the_draft() {
    // a política só decide sobre planos de 1 etapa, mas o modelo pode propor até 3
    let src = rollout().replacen("policy SafePlan(p: List[Action, 1..3])", "policy SafePlan(p: List[Action, 1..1])", 1);
    let err = compile(&src).err().unwrap();
    assert_eq!((err[0].stratum, err[0].code.as_str()), (Stratum::E5, "E0450"));
    assert!(err[0].label.contains("`List[Action, 1..1]`, o rascunho produz `List[Action, 1..3]`"), "{}", err[0].label);
}

#[test]
fn e6_rejects_a_budget_the_loops_overrun() {
    let (st, codes) = rejected(&swap(&rollout(), "wall <= 120.0", "wall <= 60.0"));
    assert_eq!(st, Stratum::E6);
    assert_eq!(codes, ["B0207"]);
    let err = compile(&swap(&rollout(), "wall <= 120.0", "wall <= 60.0")).err().unwrap();
    assert!(err[0].notes.iter().any(|n| n.contains("for a ×3")), "{:?}", err[0].notes);
}

#[test]
fn empty_and_singleton_loops_cost_what_their_bounds_say() {
    // laço sobre lista vazia: custo zero; sobre lista de um elemento: o corpo uma vez
    let base = swap(&rollout(), r#"for ch in ["sre", "dev"]"#, "for ch in []");
    let empty = accepted(&base);
    let full = accepted(&rollout());
    let d = full.check.reports[0].worst.wall - empty.check.reports[0].worst.wall;
    assert!((d - 3.0 * 2.0).abs() < 1e-9, "a lista vazia remove 3 × 2 pages: {d}");
}

// ------------------------------------------------------------------ E5: tamanhos e elementos

const SIZES: &str = r#"
module t
policy P(xs: List[Int[2..64], 1..3]) -> Bool = all x in xs => x >= 2
policy P0(xs: List[Int[2..64], 0..3]) -> Bool = all x in xs => x >= 2
policy One(n: Int[1..3]) -> Bool = n >= 1
agent G(keys: Cap[IdemKey.Mint]) budget [tok <= 10] {
  on go(xs: List[Int[2..64], 0..5], ys: List[Int[2..64], 1..3]) -> Bool ! {} {
    BODY
  }
}
"#;

fn sizes(body: &str) -> String {
    SIZES.replace("BODY", body)
}

#[test]
fn e5_proves_list_lengths_at_the_boundaries() {
    accepted(&sizes("P([2, 3, 64])")); // teto exato
    accepted(&sizes("P([64])")); // piso exato
    accepted(&sizes("P0([])")); // lista vazia onde 0 é admitido
    accepted(&sizes("P(ys)")); // limites declarados contidos
    accepted(&sizes("One(len(ys))")); // len herda os limites: Int[1..3]
    assert_eq!(rejected(&sizes("P([2, 3, 4, 5])")), (Stratum::E5, vec!["E0452".to_string()]));
    assert_eq!(rejected(&sizes("P([])")), (Stratum::E5, vec!["E0452".to_string()]));
    assert_eq!(rejected(&sizes("P([1, 3])")), (Stratum::E5, vec!["E0451".to_string()]));
    assert_eq!(rejected(&sizes("P(xs)")), (Stratum::E5, vec!["E0450".to_string()]));
    assert_eq!(rejected(&sizes("One(len(xs))")), (Stratum::E5, vec!["E0450".to_string()]));
    let err = compile(&sizes("P([2, 3, 4, 5])")).err().unwrap();
    assert_eq!(err[0].label, "4 elemento(s); `List[Int[2..64], 1..3]` admite de 1 a 3");
}

#[test]
fn e0_rejects_iterating_what_is_not_a_list() {
    assert_eq!(rejected(&sizes("all x in 3 => true")), (Stratum::E0, vec!["E0008".to_string()]));
    assert_eq!(rejected(&sizes("all x in [1, true] => true")).1, ["E0011"]);
}

// ------------------------------------------------------------------ E1 e E3 em laços

#[test]
fn e1_counts_loop_iterations_for_linear_keys() {
    let twice = swap(
        &rollout(),
        r#"      for ch in ["sre", "dev"] {
        commit page(oncall, ch ++ " ← " ++ r, keys.mint())
      }"#,
        r#"      let k = keys.mint()
      for ch in ["sre", "dev"] {
        commit page(oncall, ch ++ " ← " ++ r, k)
      }"#,
    );
    assert_eq!(rejected(&twice), (Stratum::E1, vec!["E1104".to_string()]));
    // com teto exato 1 o corpo roda uma vez: a chave é consumida exatamente uma vez
    accepted(&twice.replace(r#"for ch in ["sre", "dev"]"#, r#"for ch in ["sre"]"#));
    // lista vazia: a chave pode não ser consumida
    assert_eq!(rejected(&twice.replace(r#"for ch in ["sre", "dev"]"#, "for ch in []")).1, ["E1105"]);
}

#[test]
fn e3_iterating_untrusted_data_lowers_the_pc() {
    let src = swap(
        &rollout(),
        r#"for ch in ["sre", "dev"] {"#,
        r#"for ch in [service] {
        commit restart(cluster, "api", keys.mint())
      }
      for ch in ["sre", "dev"] {"#,
    );
    let err = compile(&src).err().unwrap();
    assert_eq!((err[0].stratum, err[0].code.as_str()), (Stratum::E3, "E0412"));
    assert!(err[0].notes.iter().any(|n| n.contains("ramo decidido por dado Untrusted")), "{:?}", err[0].notes);
}

// ------------------------------------------------------------------ contrato Δ estendido

#[test]
fn contract_overhead_must_cover_the_schema_and_fits_the_window() {
    let (st, codes) = rejected(&swap(&rollout(), "overhead  = 768", "overhead  = 64"));
    assert_eq!((st, codes), (Stratum::E6, vec!["B0302".to_string()]));
    let small_ctx = swap(&rollout(), "ctx       = 131072", "ctx       = 3000");
    let err = compile(&small_ctx).err().unwrap();
    assert_eq!(err[0].code, "B0301");
    assert!(err[0].label.contains("768 tok de overhead"), "{}", err[0].label);
    assert_eq!(rejected(&swap(&rollout(), r#"  id        = "openai/gpt-oss-20b""#, "")).1, ["E0030"]);
    assert_eq!(rejected(&swap(&rollout(), r#"version   = "2026-11-09""#, r#"versao = "x""#)).1, ["E0030"]);
}

#[test]
fn contract_identity_is_content_addressed() {
    let a = accepted(&rollout());
    let b = accepted(&swap(&rollout(), r#"version   = "2026-11-09""#, r#"version   = "2026-12-01""#));
    let c = accepted(&swap(&rollout(), "overhead  = 768", "overhead  = 800"));
    let h = |x: &athc::Compiled| x.codex.hash_of("planner").unwrap().to_string();
    assert_ne!(h(&a), h(&b), "mudar a versão é mudar de contrato");
    assert_ne!(h(&a), h(&c), "mudar o overhead é mudar de contrato");
    assert_ne!(a.codex.root, b.codex.root);
}

// ------------------------------------------------------------------ ARX

fn run(c: &athc::Compiled, plans: serde_json::Value) -> (Result<Value, arx::RtError>, Arx<'_>) {
    let world: serde_json::Value = serde_json::from_str(&read("fixtures/world.json")).unwrap();
    let mut arx = Arx::new(
        &c.module,
        Box::new(SimWorld::from_json(&world).unwrap()),
        Box::new(Scripted::from_json(&json!({ "planner": plans })).unwrap()),
    );
    arx.contracts = athc::contract_hashes(c);
    let out = arx.run("Rollout", "alert", vec![Value::Text("api".into())]);
    (out, arx)
}

fn tools(arx: &Arx) -> Vec<String> {
    arx.journal
        .entries
        .iter()
        .filter(|e| e["kind"] == "tool")
        .map(|e| e["detail"]["tool"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn arx_runs_the_plan_with_nested_loops_and_journals_the_contract() {
    let c = accepted(&rollout());
    let plans: serde_json::Value = serde_json::from_str(&read("fixtures/models-rollout.json")).unwrap();
    let (out, arx) = run(&c, plans["planner"].clone());
    assert_eq!(out.unwrap(), Value::Text("concluído".into()));
    assert_eq!(tools(&arx), ["logs", "restart", "page", "page", "scale", "healthy", "page", "page"]);
    let inf: Vec<_> = arx.journal.entries.iter().filter(|e| e["kind"] == "infer").map(|e| &e["detail"]).collect();
    assert_eq!(inf.len(), 2);
    assert_eq!(inf[0]["invalid"], "`value[0].to` = 0 fora do refinamento [2..64]");
    let contract = &inf[1]["contract"];
    assert_eq!((contract["id"].as_str(), contract["provider"].as_str()), (Some("openai/gpt-oss-20b"), Some("groq")));
    assert_eq!(contract["hash"].as_str(), c.codex.hash_of("planner"));
    // sem `usage` do servidor, a entrada é a estimativa do prompt + o overhead declarado
    assert_eq!(inf[1]["tok_in"], inf[1]["tok_in_contract"]);
    assert!(inf[1]["tok_in"].as_u64().unwrap() > 768);
    assert!(arx::journal::Journal::verify(&arx.journal.entries).is_ok());
}

#[test]
fn arx_rejects_plans_outside_the_list_bounds() {
    let c = accepted(&rollout());
    let four = serde_json::Value::Array(vec![json!({ "kind": "Restart", "deploy": "api" }); 4]);
    let (out, arx) = run(&c, json!([[], four]));
    assert_eq!(out.unwrap(), Value::Text("escalado".into()));
    let inv: Vec<_> = arx
        .journal
        .entries
        .iter()
        .filter(|e| e["kind"] == "infer")
        .map(|e| e["detail"]["invalid"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        inv,
        ["`value` tem 0 elemento(s); o tipo admite de 1 a 3", "`value` tem 4 elemento(s); o tipo admite de 1 a 3"]
    );
    assert_eq!(tools(&arx), ["logs", "page"], "nada além do aviso de escalada");
}

#[test]
fn arx_policy_quantifies_over_the_whole_plan() {
    let c = accepted(&rollout());
    // uma etapa insegura derruba o plano inteiro, mesmo com a primeira etapa segura
    let mixed = json!([{ "kind": "Restart", "deploy": "api" }, { "kind": "Restart", "deploy": "payments" }]);
    let (out, arx) = run(&c, json!([mixed.clone(), mixed]));
    assert_eq!(out.unwrap(), Value::Text("escalado".into()));
    assert_eq!(arx.journal.entries.iter().filter(|e| e["kind"] == "policy_reject").count(), 2);
    assert!(!tools(&arx).contains(&"restart".to_string()));
}
