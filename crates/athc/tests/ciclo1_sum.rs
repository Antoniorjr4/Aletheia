//! **Ciclo 1, P3 — D8: `sum x in xs => n` (24/11/2026).**
//!
//! Sem soma sobre lista, um limite sobre o total de um plano exigia o idioma de slots fixos
//! (`a: Slot, b: Slot, c: Slot` com uma variante `Empty`), que o M10 mostrou ser fonte de linhas a
//! mais, de variantes duplicadas e de políticas fracas. `sum` é a terceira dobra delimitada, ao lado
//! de `all` e `any`: itera uma `List[T, a..b]`, o corpo é `Int`, a aritmética é a de `+` (64 bits com
//! volta, igual nos dois runtimes) e o intervalo do resultado é provado pelo tipo.
//!
//! Na mesma passada: três vazamentos de confidencialidade pré-existentes, encontrados ao tipar a
//! soma, ficam fechados (elemento de lista secreta tratado como público; contagem de `for` e
//! resultado de `all`/`any` sobre lista secreta sem elevar a confidencialidade).

use arx::host::SimWorld;
use arx::model::Scripted;
use arx::{Arx, Value};
use athc::compile;
use serde_json::json;

fn codes(src: &str) -> Vec<String> {
    match compile(src) {
        Ok(_) => Vec::new(),
        Err(d) => d.into_iter().map(|x| x.code).collect(),
    }
}

/// Plano com até 3 operações; `$POLICY` decide sobre `ops`.
fn prog(policy: &str, elem: &str) -> String {
    format!(
        r#"module soma
type Op = | Pay {{ to: Text[..20], cents: {elem} }} | Nop
type Plan = | Plan {{ ops: List[Op, 0..3] }}
model m {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
policy Safe(p: Plan) -> Bool = match p {{ Plan {{ ops }} => {policy} }}
agent A() budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {{
  on go(req: Text[..200] ^ Trusted) -> Text ! {{ Infer }} {{
    let d = infer m -> Plan {{ request = req, people = s.inventory("people") }}
    let p = refine d by Safe else {{ return "recusado" }}
    "aceito"
  }}
}}
"#
    )
}

const LIMITE: &str = r#"(all o in ops => match o { Pay { to } => s.member("people", to), Nop => true }) && (sum o in ops => match o { Pay { cents } => cents, Nop => 0 }) <= 10"#;

#[test]
fn soma_aceita_em_politica_com_corpo_inteiro() {
    assert!(codes(&prog(LIMITE, "Int[-100..100]")).is_empty(), "{:?}", codes(&prog(LIMITE, "Int[-100..100]")));
    // corpo com `match` direto, sem política auxiliar
    let p = r#"s.member("people", "x") && (sum o in ops => match o { Pay { cents } => cents, Nop => 0 }) <= 10"#;
    assert!(codes(&prog(p, "Int[0..100]")).is_empty());
}

#[test]
fn soma_recusa_corpo_que_nao_e_inteiro_e_iteravel_que_nao_e_lista() {
    let p = r#"s.member("people", "x") && (sum o in ops => true) <= 10"#;
    assert!(!codes(&prog(p, "Int[0..100]")).is_empty());
    let p = r#"s.member("people", "x") && (sum o in 7 => 1) <= 10"#;
    assert!(codes(&prog(p, "Int[0..100]")).contains(&"E0008".to_string()));
    // `sum` continua sendo nome comum fora da forma `sum x in`
    let p = r#"s.member("people", "x") && (match ops { _ => true })"#;
    let src = prog(p, "Int[0..100]").replace("policy Safe(", "policy Sum(sum: Op) -> Bool = true\npolicy Safe(");
    assert!(compile(&src).is_err() || compile(&src).is_ok()); // só precisa analisar sem pânico
}

/// Tipo do parâmetro de uma ferramenta que recebe a soma: o intervalo provado tem de caber nele.
fn bound_prog(body: &str, list: &str, param: &str) -> String {
    format!(
        r#"module limite
type Op = | Pay {{ cents: Int[0..10] }} | Nop
tool total(c: Cap[X.W], n: {param} ^ Endorsed) -> Unit ! {{ X.W }} irrevocable latency 1.0
agent A(c: Cap[X.W]) budget [tok <= 100, usd <= 1.0, wall <= 100.0] {{
  on go(xs: {list} ^ Trusted) -> Unit ! {{ X.W }} {{
    commit total(c, sum x in xs => {body})
  }}
}}
"#
    )
}

#[test]
fn intervalo_da_soma_e_provado_pelo_tipo() {
    // 0..3 termos em [0, 10]: [0, 30]
    assert!(codes(&bound_prog("x", "List[Int[0..10], 0..3]", "Int[0..30]")).is_empty());
    assert!(codes(&bound_prog("x", "List[Int[0..10], 0..3]", "Int[0..29]")).contains(&"E0450".to_string()));
    // termos negativos: 1..3 termos em [-5, 10] → [-15, 30]
    assert!(codes(&bound_prog("x", "List[Int[-5..10], 1..3]", "Int[-15..30]")).is_empty());
    assert!(codes(&bound_prog("x", "List[Int[-5..10], 1..3]", "Int[-14..30]")).contains(&"E0450".to_string()));
    // termos sem limite: sem intervalo provado
    assert!(codes(&bound_prog("x", "List[Int, 0..3]", "Int[0..30]")).contains(&"E0450".to_string()));
    // Corpo `match`: desde a revisão de 26/11 o intervalo de `if`/`match` é o menor que contém os
    // ramos (antes, `Int` sem intervalo). A expectativa mudou porque a capacidade mudou; a de baixo
    // (teto 29) mostra que o intervalo não é frouxo.
    let body = "match x { Pay { cents } => cents, Nop => 0 }";
    assert!(codes(&bound_prog(body, "List[Op, 0..3]", "Int[0..30]")).is_empty());
    assert!(codes(&bound_prog(body, "List[Op, 0..3]", "Int[0..29]")).contains(&"E0450".to_string()));
    // Teto que transborda 64 bits: sem intervalo provado, nunca um intervalo errado.
    let c = codes(&bound_prog("x", "List[Int[0..9223372036854775807], 0..3]", "Int[0..9223372036854775807]"));
    assert!(c.contains(&"E0450".to_string()), "{c:?}");
}

// ------------------------------------------------------------ confidencialidade

fn leak_prog(body: &str) -> String {
    format!(
        r#"module t
tool read_private(c: Cap[Repo.Read], issue: Text[..8]) -> List[Text[..8], 0..3] ^ Secret
  ! {{ Repo.Read }} reversible latency 1.0
tool read_public(c: Cap[Repo.Read], issue: Text[..8]) -> List[Text[..8], 0..3]
  ! {{ Repo.Read }} reversible latency 1.0
tool post_public(c: Cap[Repo.Write], body: Text[..600] ^ Public) -> Unit
  ! {{ Repo.Write }} irrevocable latency 1.0
agent A(r: Cap[Repo.Read], w: Cap[Repo.Write])
  budget [tok <= 100, usd <= 1.0, wall <= 100.0]
{{
  on go(n: Text[..8] ^ Trusted) -> Unit ! {{ Repo.Read, Repo.Write }} {{
{body}
  }}
}}
"#
    )
}

#[test]
fn iterar_um_segredo_nao_o_torna_publico() {
    let cases = [
        // elemento publicado direto
        "    let s = read_private(r, n)\n    for x in s {\n      commit post_public(w, x)\n    }",
        // contagem de iterações observável
        "    let s = read_private(r, n)\n    for x in s {\n      commit post_public(w, \"ping\")\n    }",
        // resultado de quantificador decide um ramo
        "    let s = read_private(r, n)\n    if any x in s => x == \"segredo\" {\n      commit post_public(w, \"a\")\n    } else {\n      commit post_public(w, \"b\")\n    }",
        // resultado da soma publicado
        "    let s = read_private(r, n)\n    commit post_public(w, text(sum x in s => len(x)))",
    ];
    for b in cases {
        let c = codes(&leak_prog(b));
        assert!(c.contains(&"E0440".to_string()), "{b}\n{c:?}");
    }
    // controle: as mesmas formas sobre a lista pública são aceitas
    for b in cases {
        let b = b.replace("read_private", "read_public");
        assert!(codes(&leak_prog(&b)).is_empty(), "{b}\n{:?}", codes(&leak_prog(&b)));
    }
}

// ------------------------------------------------------------ os dois runtimes

fn run_both(ops: serde_json::Value, tag: &str) -> (serde_json::Value, serde_json::Value) {
    let c = compile(&prog(LIMITE, "Int[-100..100]")).unwrap();
    let models = json!({ "m": [{ "kind": "Plan", "ops": ops }] });
    let world = json!({ "inventories": { "people": ["Alice"] } });
    let mut arx = Arx::new(
        &c.module,
        Box::new(SimWorld::from_json(&world).unwrap()),
        Box::new(Scripted::from_json(&models).unwrap()),
    );
    arx.contracts = athc::contract_hashes(&c);
    let out = arx.run("A", "go", vec![Value::Text("x".into())]).unwrap().to_json();
    let dir = std::env::temp_dir().join(format!("ciclo1-sum-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let py = ath_emit::python(&c.module, &c.check, &c.codex, "s.ath").expect("emissão");
    let (prog_p, w, m, j, r) =
        (dir.join("s.py"), dir.join("w.json"), dir.join("m.json"), dir.join("j.jsonl"), dir.join("r.json"));
    std::fs::write(&prog_p, py).unwrap();
    std::fs::write(&w, world.to_string()).unwrap();
    std::fs::write(&m, models.to_string()).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let st = std::process::Command::new("python3")
        .env("PYTHONPATH", root.join("runtimes/python"))
        .args(["-m", "aletheia_runtime", "run"])
        .arg(&prog_p)
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
    let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&r).unwrap()).unwrap();
    (out, report["result"].clone())
}

#[test]
fn soma_decide_igual_nos_dois_runtimes() {
    let pay = |c: i64| json!({ "kind": "Pay", "to": "Alice", "cents": c });
    let nop = json!({ "kind": "Nop" });
    let cases = [
        (json!([]), true),
        (json!([pay(10)]), true),
        (json!([pay(11)]), false),
        (json!([pay(4), pay(3), pay(3)]), true),
        (json!([pay(4), pay(4), pay(3)]), false),
        (json!([pay(100), pay(-95), nop.clone()]), true),
        (json!([pay(-100), pay(-100), pay(100)]), true),
        (json!([nop.clone(), nop.clone(), pay(11)]), false),
        // pessoa fora do inventário: recusado pelo `all`, não pela soma
        (json!([{ "kind": "Pay", "to": "Mallory", "cents": 1 }]), false),
    ];
    for (i, (ops, ok)) in cases.iter().enumerate() {
        let (a, p) = run_both(ops.clone(), &i.to_string());
        let want = if *ok { json!("aceito") } else { json!("recusado") };
        assert_eq!(a, want, "ARX: {ops}");
        assert_eq!(p, want, "Python: {ops}");
    }
}

#[test]
fn variante_duplicada_diz_o_tipo_e_sugere_um_nome() {
    let src = "module v\ntype A = | Empty | X\ntype B = | Empty | Y\n";
    let d = compile(src).err().expect("aceito com variante duplicada");
    let e = d.iter().find(|x| x.code == "E0002").expect("sem E0002");
    assert!(e.notes.iter().any(|n| n.contains("`Empty` já é variante de `A`")), "{:?}", e.notes);
    assert!(e.help.as_deref().is_some_and(|h| h.contains("`EmptyB`")), "{:?}", e.help);
    // duplicada no mesmo tipo: o erro de sempre, sem sugestão de outro tipo
    let d = compile("module v\ntype A = | X | X\n").err().expect("aceito");
    let e = d.iter().find(|x| x.code == "E0002").expect("sem E0002");
    assert!(e.notes.is_empty() && e.help.is_none());
}
