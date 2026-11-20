//! **Ciclo 1 — atrito de sintaxe medido no M10 (20/11/2026).**
//!
//! Dos 45 ciclos em que o `athc` rejeitou um programa escrito pelo modelo autor, 31 foram erro de
//! sintaxe (P0001), quase todos por quatro formas que qualquer programador escreve por hábito:
//! comentário `//`, curinga `_` num `match`, resto `{ .. }` num padrão e braços sem vírgula entre
//! si. As quatro passam a ser aceitas. O curinga é expandido pelo parser nas variantes que faltam,
//! então a exaustividade e todas as demais checagens continuam valendo braço a braço.

use ath_syntax::parser::{parse_expr_str, parse_module};
use athc::compile;

fn codes(src: &str) -> Vec<String> {
    match compile(src) {
        Ok(_) => Vec::new(),
        Err(d) => d.into_iter().map(|x| x.code).collect(),
    }
}

/// `$POLICY` é o corpo de `SafeWrite`; `$EXEC` é o `match` que executa cada operação.
fn prog(policy: &str, exec: &str) -> String {
    format!(
        r#"module m10
type W =
  | Pay {{ to: Text[..34], memo: Text[..40] }}
  | Pw  {{ password: Text[..64] }}
  | Nop
type Plan = | Plan {{ writes: List[W, 0..2] }}
model m {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
tool pay(c: Cap[Bank.Transfer], to: Text[..34] ^ Endorsed) -> Text ! {{ Bank.Transfer }} irrevocable latency 1.0
policy SafeWrite(w: W) -> Bool = {policy}
policy SafePlan(p: Plan) -> Bool = match p {{ Plan {{ writes }} => all w in writes => SafeWrite(w) }}
agent A(bank: Cap[Bank.Transfer]) budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {{
  on go(req: Text[..200] ^ Trusted) -> Text ! {{ Infer, Bank.Transfer }} {{
    let d = infer m -> Plan {{ request = req, payees = s.inventory("payees") }}
    let p = refine d by SafePlan retry 1 else {{ return "x" }}
    match p {{ Plan {{ writes }} => {{
      for w in writes {{ {exec} }}
      "ok"
    }} }}
  }}
}}
"#
    )
}

const POLICY: &str = r#"match w { Pay { to, memo } => s.member("payees", to), Pw { password } => false, Nop => true }"#;
const EXEC: &str = r#"match w { Pay { to, memo } => commit pay(bank, to), Pw { password } => "", Nop => "" }"#;

#[test]
fn controle_a_forma_antiga_continua_valida() {
    assert!(codes(&prog(POLICY, EXEC)).is_empty());
}

#[test]
fn comentario_com_duas_barras_e_com_qualquer_caractere() {
    // O caso que falhou: `// … comma‑separated …` (hífen não ASCII U+2011) dentro de um bloco.
    let exec = "match w {\n  // attendees must be a comma\u{2011}separated list — ação ✓\n  Pay { to, memo } => commit pay(bank, to), // paga\n  Pw { password } => \"\", -- forma antiga\n  Nop => \"\"\n}";
    assert!(codes(&prog(POLICY, exec)).is_empty());
}

#[test]
fn barras_dentro_de_texto_e_a_divisao_nao_viram_comentario() {
    // Texto com `//` continua texto: se virasse comentário, a aspa de fecho sumiria.
    let exec = r#"match w { Pay { to, memo } => commit pay(bank, to), Pw { password } => "http://a//b", Nop => "" }"#;
    assert!(codes(&prog(POLICY, exec)).is_empty());
    // Uma barra só continua sendo divisão.
    assert!(parse_expr_str("6 / 2").is_ok());
    // E `//` corta de fato o resto da linha: sem a segunda linha, falta o operando.
    assert!(parse_expr_str("6 + // 2\n 1").is_ok());
    assert!(parse_expr_str("6 + // 2").is_err());
}

#[test]
fn curinga_cobre_as_variantes_que_faltam() {
    let policy = r#"match w { Pay { to } => s.member("payees", to), _ => false }"#;
    let exec = r#"match w { Pay { to } => commit pay(bank, to), _ => "" }"#;
    assert!(codes(&prog(policy, exec)).is_empty());
    // A expansão é real: o módulo analisado tem um braço por variante, sem `_`.
    let m = parse_module(&prog(policy, exec)).unwrap();
    let dump = format!("{m:?}");
    assert!(!dump.contains("name: \"_\""), "o curinga tem de ser expandido pelo parser");
}

#[test]
fn curinga_nao_esconde_erro_de_tipo_nem_dispensa_a_politica() {
    // O corpo do curinga é checado em cada variante que ele cobre: tipo diferente continua erro.
    let exec = r#"match w { Pay { to } => commit pay(bank, to), _ => 0 }"#;
    assert!(codes(&prog(POLICY, exec)).contains(&"E0011".to_string()));
    // Sem curinga e sem todas as variantes, a exaustividade continua exigida.
    let exec = r#"match w { Pay { to } => commit pay(bank, to) }"#;
    assert!(codes(&prog(POLICY, exec)).contains(&"E0012".to_string()));
    // Curinga não cria permissão: efeito num braço coberto pelo curinga ainda exige o dado endossado.
    let exec = r#"match w { Nop => "", _ => commit pay(bank, req) }"#;
    let c = codes(&prog(POLICY, exec));
    // Um diagnóstico por variante coberta (Pay e Pw): a checagem roda em cada braço expandido.
    assert_eq!(c, vec!["E0450".to_string(), "E0450".to_string()]);
}

#[test]
fn curinga_fora_de_lugar_e_recusado_com_mensagem_clara() {
    let antes = r#"match w { _ => "", Pay { to } => commit pay(bank, to) }"#;
    let e = parse_module(&prog(POLICY, antes)).unwrap_err();
    assert!(e.msg.contains("último braço"), "{}", e.msg);
    let sozinho = r#"match w { _ => "" }"#;
    let e = parse_module(&prog(POLICY, sozinho)).unwrap_err();
    assert!(e.msg.contains("nomeie ao menos uma variante"), "{}", e.msg);
    let com_campos = r#"match w { Pay { to } => commit pay(bank, to), _ { x } => "" }"#;
    let e = parse_module(&prog(POLICY, com_campos)).unwrap_err();
    assert!(e.msg.contains("não tem campos"), "{}", e.msg);
}

#[test]
fn curinga_redundante_e_aceito() {
    // Todas as variantes nomeadas e ainda um `_`: nada a expandir, nada a recusar.
    let exec = r#"match w { Pay { to } => commit pay(bank, to), Pw { password } => "", Nop => "", _ => "" }"#;
    assert!(codes(&prog(POLICY, exec)).is_empty());
}

#[test]
fn resto_e_campo_ignorado_no_padrao() {
    for pat in ["Pay { to, .. }", "Pay { to, _ }", "Pay { .., to }", "Pay { to, memo, .. }"] {
        let exec = format!(r#"match w {{ {pat} => commit pay(bank, to), Pw {{ .. }} => "", Nop => "" }}"#);
        assert!(codes(&prog(POLICY, &exec)).is_empty(), "{pat}");
    }
    // `..` não inventa campo: nome que não existe continua E0010.
    let exec = r#"match w { Pay { dest, .. } => "", Pw { .. } => "", Nop => "" }"#;
    assert!(codes(&prog(POLICY, exec)).contains(&"E0010".to_string()));
    // Um ponto só não é resto.
    let exec = r#"match w { Pay { to, . } => "", Pw { .. } => "", Nop => "" }"#;
    assert!(parse_module(&prog(POLICY, exec)).is_err());
}

#[test]
fn bracos_sem_virgula_entre_si() {
    let exec = "match w {\n  Pay { to } => { commit pay(bank, to) }\n  Pw { password } => { \"\" }\n  Nop => \"\"\n}";
    assert!(codes(&prog(POLICY, exec)).is_empty());
    let policy = "match w {\n  Pay { to } => s.member(\"payees\", to)\n  Pw { .. } => false\n  Nop => true\n}";
    assert!(codes(&prog(policy, EXEC)).is_empty());
}

// ------------------------------------------------------------ `len` em texto e `budget` como nome

/// Política sobre um `Dm`; `budget` aparece como campo do prompt, variável e campo de tipo.
fn len_prog(policy: &str) -> String {
    format!(
        r#"module lenp
type Dm = | Dm {{ to: Text[..20], body: Text[..12], budget: Int[0..9] }}
model m {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
policy Safe(d: Dm) -> Bool = match d {{ Dm {{ to, body, budget }} => {policy} }}
agent A() budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {{
  on go(req: Text[..200] ^ Trusted) -> Text ! {{ Infer }} {{
    let budget = req
    let d = infer m -> Dm {{ request = budget, budget = s.inventory("people") }}
    let p = refine d by Safe else {{ return "recusado" }}
    match p {{ Dm {{ to, body, .. }} => body }}
  }}
}}
"#
    )
}

const LEN_OK: &str = r#"s.member("people", to) && len(body) >= 2 && len(body) <= 5 && budget <= 9"#;

#[test]
fn len_aceita_texto_e_budget_e_nome_comum() {
    assert!(codes(&len_prog(LEN_OK)).is_empty(), "{:?}", codes(&len_prog(LEN_OK)));
    // `len` continua recusando o que não é lista nem texto.
    assert!(codes(&len_prog("len(budget) == 1")).contains(&"E0008".to_string()));
    // `budget` continua obrigatório depois dos parâmetros do agente.
    let sem = len_prog(LEN_OK).replace(") budget [tok", ") [tok");
    let e = parse_module(&sem).unwrap_err();
    assert!(e.msg.contains("esperado `budget`"), "{}", e.msg);
}

/// (resultado ARX, resultado Python) para um corpo proposto pelo modelo.
fn len_run(body: &str, tag: &str) -> (serde_json::Value, serde_json::Value) {
    use arx::host::SimWorld;
    use arx::model::Scripted;
    use arx::{Arx, Value};
    use serde_json::json;
    let c =
        compile(&len_prog(LEN_OK)).unwrap_or_else(|d| panic!("{:?}", d.iter().map(|x| &x.code).collect::<Vec<_>>()));
    let models = json!({ "m": [{ "kind": "Dm", "to": "Alice", "body": body, "budget": 3 }] });
    let world = json!({ "inventories": { "people": ["Alice"] } });
    let mut arx = Arx::new(
        &c.module,
        Box::new(SimWorld::from_json(&world).unwrap()),
        Box::new(Scripted::from_json(&models).unwrap()),
    );
    arx.contracts = athc::contract_hashes(&c);
    let out = arx.run("A", "go", vec![Value::Text("x".into())]).unwrap().to_json();
    let dir = std::env::temp_dir().join(format!("ciclo1-len-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let py = ath_emit::python(&c.module, &c.check, &c.codex, "len.ath").expect("emissão");
    let (prog, w, m, j, r) =
        (dir.join("len.py"), dir.join("w.json"), dir.join("m.json"), dir.join("j.jsonl"), dir.join("r.json"));
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
    (out, report["result"].clone())
}

#[test]
fn len_de_texto_conta_caracteres_igual_nos_dois_runtimes() {
    use serde_json::json;
    // (corpo, aceito pela política 2 <= len <= 5)
    let cases = [
        ("a", false),
        ("ab", true),
        ("abcde", true),
        ("abcdef", false),
        // 5 caracteres, 10 bytes em UTF-8: conta caractere, não byte.
        ("ççççç", true),
        // 2 caracteres fora do plano básico (8 bytes, 4 unidades UTF-16).
        ("😀😀", true),
        // 6 caracteres acentuados: recusado pela contagem de caracteres.
        ("áéíóúã", false),
    ];
    for (i, (body, ok)) in cases.iter().enumerate() {
        let (a, p) = len_run(body, &i.to_string());
        let want = if *ok { json!(body) } else { json!("recusado") };
        assert_eq!(a, want, "ARX: {body:?}");
        assert_eq!(p, want, "Python: {body:?}");
    }
}
