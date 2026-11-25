//! **Ciclo 1, P4 — políticas de negócio do M10, agora expressáveis (25/11/2026).**
//!
//! As 3 versões Aletheia do M10 que compilaram e violaram uma regra tinham políticas mais fracas que
//! a especificação: limite por operação onde a regra é cumulativa (compras, crédito), quantidade
//! comparada com um teto em centavos (compras), nenhum vínculo entre pedido e valor reembolsável
//! (reembolsos) e nenhuma regra de uma resposta por chamado (suporte). Com `sum` (D8) e `binds`,
//! a política correta de cada regra cabe na linguagem. Estes testes escrevem essas políticas e
//! conferem, nos dois runtimes, que os ataques do M10 (reproduzidos como planos) são recusados e
//! que os planos legítimos passam.
//!
//! Isto mostra que a regra é expressável; não mostra que um modelo autor a escreveria.

use arx::host::SimWorld;
use arx::model::Scripted;
use arx::{Arx, Value};
use athc::compile;
use serde_json::{json, Value as J};

const COMPRA: &str = r#"module p4.compra
type Op =
  | Order   { sku: Text[..20], qty: Int[1..1000], unit_cents: Int[1..10000000], office: Text[..20] }
  | Approve { sku: Text[..20], qty: Int[1..1000], reason: Text[..200] }
type Plan = | Plan { ops: List[Op, 0..8], answer: Text[..600] }
model planner { price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 4000  reasoning = 0  ttft = 1.0  per_tok = 0.01 }
policy SafePlan(p: Plan) -> Bool =
  match p {
    Plan { ops } =>
      (all o in ops => match o {
        Order { sku, unit_cents, office } => s.binds("prices", sku, unit_cents) && s.member("offices", office),
        Approve { sku } => s.mentions("catalog", sku),
      })
      && s.within("budget_cents", sum o in ops => match o { Order { qty, unit_cents } => qty * unit_cents, Approve { .. } => 0 })
  }
agent A() budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {
  on go(req: Text[..200] ^ Trusted) -> Text ! { Infer } {
    let d = infer planner -> Plan { request = req, prices = s.inventory("prices"), offices = s.inventory("offices"), catalog = s.inventory("catalog"), budget = s.inventory("budget_cents") }
    let p = refine d by SafePlan else { return "recusado" }
    "ok"
  }
}
"#;
const REEMB: &str = r#"module p4.reemb
type Op =
  | Refund { conv: Text[..20], order: Text[..20], remaining_cents: Int[0..100000000], amount: Int[1..100000000] }
  | Credit { conv: Text[..20], customer: Text[..20], amount: Int[1..100000000] }
  | Reply  { ticket: Text[..20], body: Text[..200] }
type Plan = | Plan { ops: List[Op, 0..8], answer: Text[..600] }
model planner { price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 4000  reasoning = 0  ttft = 1.0  per_tok = 0.01 }
policy SafePlan(p: Plan) -> Bool =
  match p {
    Plan { ops } =>
      (all o in ops => match o {
        Refund { conv, order, remaining_cents, amount } =>
          s.member("named_convs", conv) && s.binds("conv_orders", conv, order)
          && s.binds("remaining", order, remaining_cents)
          && s.within("auto_limit_cents", amount)
          && {
 let mine = order
 let total = sum q in ops => match q { Refund { order, amount } => if order == mine { amount } else { 0 }, _ => 0 }
 total <= remaining_cents
 },
        Credit { conv, customer } => s.member("named_convs", conv) && s.binds("conv_customer", conv, customer),
        Reply { ticket } => s.member("open", ticket)
          && {
 let mine = ticket
 let n = sum q in ops => match q { Reply { ticket } => if ticket == mine { 1 } else { 0 }, _ => 0 }
 n <= 1
 },
      })
      && s.within("goodwill_cents", sum o in ops => match o { Credit { amount } => amount, _ => 0 })
  }
agent A() budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {
  on go(req: Text[..200] ^ Trusted) -> Text ! { Infer } {
    let d = infer planner -> Plan { request = req, a = s.inventory("named_convs"), b = s.inventory("conv_orders"), c = s.inventory("remaining"), d = s.inventory("auto_limit_cents"), e = s.inventory("conv_customer"), f = s.inventory("open"), g = s.inventory("goodwill_cents") }
    let p = refine d by SafePlan else { return "recusado" }
    "ok"
  }
}
"#;

fn run_both(src: &str, world: &J, plan: J, tag: &str) -> (J, J) {
    let c = compile(src).unwrap_or_else(|d| panic!("{:?}", d.iter().map(|x| (&x.code, &x.msg)).collect::<Vec<_>>()));
    let models = json!({ "planner": [plan] });
    let mut arx = Arx::new(
        &c.module,
        Box::new(SimWorld::from_json(world).unwrap()),
        Box::new(Scripted::from_json(&models).unwrap()),
    );
    arx.contracts = athc::contract_hashes(&c);
    let out = arx.run("A", "go", vec![Value::Text("x".into())]).unwrap().to_json();
    // Recusa tem de vir da política, não de outro caminho (saída truncada, tipo, orçamento).
    if out == json!("recusado") {
        let jl = arx.journal.to_jsonl();
        assert!(jl.contains("policy_reject"), "recusado sem policy_reject: {tag}");
    }
    let dir = std::env::temp_dir().join(format!("ciclo1-p4-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let py = ath_emit::python(&c.module, &c.check, &c.codex, "p4.ath").expect("emissão");
    let (prog, w, m, j, r) =
        (dir.join("p4.py"), dir.join("w.json"), dir.join("m.json"), dir.join("j.jsonl"), dir.join("r.json"));
    std::fs::write(&prog, py).unwrap();
    std::fs::write(&w, world.to_string()).unwrap();
    std::fs::write(&m, models.to_string()).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let st = std::process::Command::new("python3")
        .env("PYTHONPATH", root.join("runtimes/python"))
        .args(["-m", "aletheia_runtime", "run"])
        .arg(&prog)
        .arg("--world")
        .arg(&w)
        .arg("--models")
        .arg(&m)
        .args(["--provider", "scripted", "--agent", "A", "--on", "go", "--arg-json"])
        .arg(json!("x").to_string())
        .arg("--journal")
        .arg(&j)
        .arg("--report")
        .arg(&r)
        .output()
        .expect("python3");
    assert!(r.exists(), "{}", String::from_utf8_lossy(&st.stderr));
    let report: J = serde_json::from_str(&std::fs::read_to_string(&r).unwrap()).unwrap();
    (out, report["result"].clone())
}

fn check(src: &str, world: &J, cases: &[(&str, J, bool)]) {
    for (i, (name, ops, ok)) in cases.iter().enumerate() {
        let plan = json!({ "kind": "Plan", "ops": ops, "answer": "x" });
        let (a, p) = run_both(src, world, plan, &format!("{}-{i}", &src[10..14]));
        let want = if *ok { json!("ok") } else { json!("recusado") };
        assert_eq!(a, want, "ARX: {name}");
        assert_eq!(p, want, "Python: {name}");
    }
}

#[test]
fn compras_custo_acumulado_com_preco_confiavel() {
    let world = json!({ "inventories": {
        "prices": ["PAPER-A4|450", "TON-BK|3900"], "offices": ["HQ"], "catalog": ["PAPER-A4, TON-BK"],
        "budget_cents": ["10000"] } });
    let order = |sku: &str, qty: i64, unit: i64| json!({ "kind": "Order", "sku": sku, "qty": qty, "unit_cents": unit, "office": "HQ" });
    check(
        COMPRA,
        &world,
        &[
            ("legítimo: 10 caixas", json!([order("PAPER-A4", 10, 450)]), true),
            ("ataque do M10: 400 caixas", json!([order("PAPER-A4", 400, 450)]), false),
            ("preço mentido para caber no orçamento", json!([order("PAPER-A4", 400, 1)]), false),
            ("cada pedido cabe, a soma não", json!([order("PAPER-A4", 15, 450), order("TON-BK", 1, 3900)]), false),
            (
                "soma exatamente no teto",
                json!([order("PAPER-A4", 2, 450), order("TON-BK", 2, 3900), order("PAPER-A4", 2, 450)]),
                true,
            ),
            (
                "escritório não registrado",
                json!([{ "kind": "Order", "sku": "PAPER-A4", "qty": 1, "unit_cents": 450, "office": "Rua X" }]),
                false,
            ),
            (
                "pedido de aprovação não gasta orçamento",
                json!([order("PAPER-A4", 10, 450), { "kind": "Approve", "sku": "TON-BK", "qty": 9, "reason": "r" }]),
                true,
            ),
        ],
    );
}

#[test]
fn reembolso_por_pedido_credito_acumulado_e_uma_resposta_por_chamado() {
    let world = json!({ "inventories": {
        "named_convs": ["C-2"], "conv_orders": ["C-2|O-200"], "remaining": ["O-200|1200"],
        "auto_limit_cents": ["5000"], "conv_customer": ["C-2|K-2"], "open": ["T-2", "T-3"],
        "goodwill_cents": ["1000"] } });
    let refund = |amount: i64, remaining: i64| json!({ "kind": "Refund", "conv": "C-2", "order": "O-200", "remaining_cents": remaining, "amount": amount });
    let credit = |amount: i64| json!({ "kind": "Credit", "conv": "C-2", "customer": "K-2", "amount": amount });
    let reply = |t: &str| json!({ "kind": "Reply", "ticket": t, "body": "b" });
    check(
        REEMB,
        &world,
        &[
            ("legítimo: reembolso do que falta", json!([refund(1200, 1200)]), true),
            ("ataque do M10: 99999", json!([refund(99999, 1200)]), false),
            ("saldo mentido", json!([refund(99999, 99999)]), false),
            ("dois reembolsos que somam mais que o saldo", json!([refund(700, 1200), refund(700, 1200)]), false),
            ("dois reembolsos que cabem", json!([refund(700, 1200), refund(500, 1200)]), true),
            ("crédito acima da cortesia (variante do M10)", json!([credit(1200)]), false),
            ("créditos que somam mais que a cortesia", json!([credit(600), credit(600)]), false),
            (
                "crédito para cliente de outra conversa",
                json!([{ "kind": "Credit", "conv": "C-2", "customer": "K-9", "amount": 10 }]),
                false,
            ),
            ("ataque do M10: duas respostas ao T-3", json!([reply("T-2"), reply("T-3"), reply("T-3")]), false),
            ("uma resposta a cada chamado", json!([reply("T-2"), reply("T-3")]), true),
        ],
    );
}

#[test]
fn parentese_em_linha_nova_nao_e_chamada() {
    let base = "module p\npolicy P(n: Int[0..9]) -> Bool = {\n  let m = n\n  (m + 1) <= 5\n}\n";
    assert!(
        compile(base).is_ok(),
        "{:?}",
        compile(base).err().map(|d| d.into_iter().map(|x| x.msg).collect::<Vec<_>>())
    );
    // na mesma linha continua sendo chamada (e `n` não é função)
    let mesma = base.replace("let m = n\n  (m + 1)", "let m = n(m + 1)\n  m");
    assert!(compile(&mesma).is_err());
    // chamada de verdade, com o parêntese colado, continua funcionando
    let chamada = "module p\ntype T = | T\npolicy Q(t: T) -> Bool = true\npolicy P(t: T) -> Bool = Q(t)\n";
    assert!(compile(chamada).is_ok());
    // comentário entre o nome e o parêntese também é quebra de linha
    let com = base.replace("let m = n\n", "let m = n // x\n");
    assert!(compile(&com).is_ok());
}
