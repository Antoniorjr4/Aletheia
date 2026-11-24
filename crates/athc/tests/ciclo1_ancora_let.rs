//! **Ciclo 1 — âncora do inventário através de `let` (24/11/2026).**
//!
//! E0421 exige que o prompt do `infer` receba o inventário em que a política decide. Até aqui só a
//! forma direta `campo = s.inventory("k")` contava; `let inv = s.inventory("k")` seguido de
//! `campo = inv` era recusado, embora entregue ao modelo exatamente o mesmo texto: a fotografia é
//! capturada uma vez por execução e é imutável nos dois runtimes, e `let` não se reatribui.
//! A forma com `let` passa a contar, resolvida pelo escopo do verificador: só a ligação visível
//! conta, qualquer outra ligação com o mesmo nome (outro `let`, variável de `for`, campo de padrão)
//! esconde a âncora, e nada derivado da fotografia (`fit`, concatenação, `if`) é âncora.

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

/// `$PRE` são instruções antes do `infer`; `$FIELD` é o valor do campo `people` do prompt.
fn prog(pre: &str, field: &str) -> String {
    format!(
        r#"module ancora
type Dm = | Dm {{ to: Text[..20], body: Text[..40] }}
model m {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
policy Safe(d: Dm) -> Bool = match d {{ Dm {{ to, body }} => s.member("people", to) }}
agent A() budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {{
  on go(req: Text[..200] ^ Trusted) -> Text ! {{ Infer }} {{
    {pre}
    let d = infer m -> Dm {{ request = req, people = {field} }}
    let p = refine d by Safe else {{ return "recusado" }}
    match p {{ Dm {{ to, body }} => body }}
  }}
}}
"#
    )
}

#[test]
fn controle_forma_direta() {
    assert!(codes(&prog("", r#"s.inventory("people")"#)).is_empty());
    assert!(codes(&prog("", "req")).contains(&"E0421".to_string()));
}

#[test]
fn let_da_fotografia_ancora_direto_e_em_cadeia() {
    assert!(codes(&prog(r#"let inv = s.inventory("people")"#, "inv")).is_empty());
    assert!(codes(&prog("let inv = s.inventory(\"people\")\n    let outra = inv", "outra")).is_empty());
}

#[test]
fn qualquer_ligacao_mais_recente_esconde_a_ancora() {
    // outro `let` com o mesmo nome
    let pre = "let inv = s.inventory(\"people\")\n    let inv = req";
    assert!(codes(&prog(pre, "inv")).contains(&"E0421".to_string()));
    // `let` interno a um bloco que já terminou não vale fora dele; a ligação visível é a externa
    let pre = "let inv = req\n    let x = if true { let inv = s.inventory(\"people\")\n inv } else { req }";
    assert!(codes(&prog(pre, "inv")).contains(&"E0421".to_string()));
    // variável de `for` com o mesmo nome, dentro do laço
    let src = prog(r#"let inv = s.inventory("people")"#, "inv").replace(
        "    let d = infer m -> Dm { request = req, people = inv }\n    let p = refine d by Safe else { return \"recusado\" }\n    match p { Dm { to, body } => body }",
        "    for inv in [req] {\n      let d = infer m -> Dm { request = req, people = inv }\n      let p = refine d by Safe else { return \"recusado\" }\n      \"\"\n    }\n    \"ok\"",
    );
    assert!(src.contains("for inv in"), "substituição do teste falhou");
    assert!(codes(&src).contains(&"E0421".to_string()), "{:?}", codes(&src));
}

#[test]
fn derivado_da_fotografia_nao_e_ancora() {
    for pre in [
        r#"let inv = fit(s.inventory("people"), 10)"#,
        r#"let inv = if true { s.inventory("people") } else { req }"#,
        r#"let inv = s.inventory("outros")"#,
    ] {
        let c = codes(&prog(pre, "inv"));
        assert!(c.contains(&"E0421".to_string()), "{pre}: {c:?}");
    }
}

#[test]
fn ancora_de_um_infer_nao_vaza_para_outro_handler() {
    let src = prog(r#"let inv = s.inventory("people")"#, "inv").replace(
        "\n}\n",
        "\n  on other(req: Text[..200] ^ Trusted) -> Text ! { Infer } {\n    let inv = req\n    let d = infer m -> Dm { request = req, people = inv }\n    let p = refine d by Safe else { return \"recusado\" }\n    \"\"\n  }\n}\n",
    );
    assert!(src.contains("on other"), "substituição do teste falhou");
    let c = codes(&src);
    assert_eq!(c.iter().filter(|x| *x == "E0421").count(), 1, "{c:?}");
}

/// prompt_hash de cada `infer` no journal de cada runtime, e o resultado.
fn run_both(src: &str, tag: &str) -> (Vec<String>, Vec<String>, serde_json::Value, serde_json::Value) {
    let c = compile(src).unwrap_or_else(|d| panic!("{:?}", d.iter().map(|x| &x.code).collect::<Vec<_>>()));
    let models = json!({ "m": [{ "kind": "Dm", "to": "Alice", "body": "oi" }] });
    let world = json!({ "inventories": { "people": ["Alice", "Bob"] } });
    let mut arx = Arx::new(
        &c.module,
        Box::new(SimWorld::from_json(&world).unwrap()),
        Box::new(Scripted::from_json(&models).unwrap()),
    );
    arx.contracts = athc::contract_hashes(&c);
    let out = arx.run("A", "go", vec![Value::Text("x".into())]).unwrap().to_json();
    let dir = std::env::temp_dir().join(format!("ciclo1-ancora-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let py = ath_emit::python(&c.module, &c.check, &c.codex, "a.ath").expect("emissão");
    let (prog, w, m, j, r) =
        (dir.join("a.py"), dir.join("w.json"), dir.join("m.json"), dir.join("j.jsonl"), dir.join("r.json"));
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
    let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&r).unwrap()).unwrap();
    let hashes = |jl: &str| -> Vec<String> {
        jl.lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|v| {
                let d = if v.get("detail").is_some() { v["detail"].clone() } else { v.clone() };
                d.get("prompt_hash").and_then(|h| h.as_str()).map(str::to_string)
            })
            .collect()
    };
    (hashes(&arx.journal.to_jsonl()), hashes(&std::fs::read_to_string(&j).unwrap()), out, report["result"].clone())
}

#[test]
fn forma_com_let_entrega_ao_modelo_o_mesmo_prompt_nos_dois_runtimes() {
    let (ad, pd, ra, rp) = run_both(&prog("", r#"s.inventory("people")"#), "direta");
    let (al, pl, la, lp) = run_both(&prog(r#"let inv = s.inventory("people")"#, "inv"), "let");
    assert!(!ad.is_empty() && !pd.is_empty(), "journal sem prompt_hash: {ad:?} {pd:?}");
    assert_eq!(ad, al, "ARX: prompt diferente entre forma direta e forma com let");
    assert_eq!(pd, pl, "Python: prompt diferente entre forma direta e forma com let");
    assert_eq!((ra, rp), (la, lp));
}
