//! **Ciclo 1, pós-P3 — `keeping` com avaliação ordenada e autorização integral (26/11/2026).**
//!
//! Duas lacunas medidas no P3, as duas resolvidas no mesmo ponto da linguagem (o `refine … keeping`,
//! onde a proposta do modelo vira plano endossado):
//!
//! * **C3, estado ao longo da sequência.** `policy Q(x: T, aceitos: List[T, 0..n]) -> Bool`: com o
//!   segundo parâmetro, `Q` julga cada elemento na ordem proposta vendo a lista dos que ela mesma já
//!   aceitou neste plano. Aceitar acrescenta o elemento à lista; recusar não muda nada e não
//!   interrompe: o elemento seguinte é julgado. O estado é limitado pelo tipo, não tem efeito
//!   externo e não sobrevive ao `refine` (é um limite por plano, não uma cota entre execuções).
//! * **C5, autorização integral.** `keeping all campo by Q as fora`: se `Q` recusa qualquer
//!   elemento, nenhum fica e `fora` é a proposta inteira. É decisão anterior a qualquer efeito;
//!   não é transação: não desfaz chamadas externas nem trata falha de ferramenta.
//!
//! Os valores esperados foram derivados à mão da regra "teto de 500 no total dos aceitos, na ordem".

use arx::host::SimWorld;
use arx::model::Scripted;
use arx::{Arx, Value};
use athc::compile;
use serde_json::{json, Value as J};

fn codes(src: &str) -> Vec<String> {
    match compile(src) {
        Ok(_) => Vec::new(),
        Err(d) => d.into_iter().map(|x| x.code).collect(),
    }
}

/// `$Q` é a declaração da política de elemento `Q`; `$MODE` é "" ou "all ".
fn prog(q: &str, mode: &str) -> String {
    format!(
        r#"module ordem
type Op = | Pay {{ to: Text[..20], cents: Int[1..100000] }} | Note {{ body: Text[..20] }}
type Plan = | Plan {{ ops: List[Op, 0..4] }}
model planner {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 4000  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
{q}
policy P(p: Plan) -> Bool = match p {{ Plan {{ ops }} => (sum o in ops => match o {{ Pay {{ cents }} => cents, _ => 0 }}) <= 500 }}
agent A() budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {{
  on go(req: Text[..200] ^ Trusted) -> Text ! {{ Infer }} {{
    let d = infer planner -> Plan {{ request = req, people = s.inventory("people") }}
    let p = refine d by P keeping {mode}ops by Q as fora else {{ return "sem plano" }}
    match p {{ Plan {{ ops }} =>
      text(len(ops)) ++ " mantidas, soma " ++ text(sum o in ops => match o {{ Pay {{ cents }} => cents, _ => 0 }})
        ++ ", " ++ text(len(fora)) ++ " fora"
    }}
  }}
}}
"#
    )
}

const CAP: &str = r#"policy Q(o: Op, aceitos: List[Op, 0..4]) -> Bool = match o {
  Pay { to, cents } => s.member("people", to)
    && (sum a in aceitos => match a { Pay { cents } => cents, _ => 0 }) + cents <= 500,
  Note { .. } => true,
}"#;
const PLAIN: &str =
    r#"policy Q(o: Op) -> Bool = match o { Pay { to } => s.member("people", to), Note { .. } => true }"#;

fn run_both(src: &str, ops: J, tag: &str) -> (J, J) {
    let c = compile(src).unwrap_or_else(|d| panic!("{:?}", d.iter().map(|x| (&x.code, &x.msg)).collect::<Vec<_>>()));
    let models = json!({ "planner": [{ "kind": "Plan", "ops": ops }] });
    let world = json!({ "inventories": { "people": ["ana", "bia"] } });
    let mut arx = Arx::new(
        &c.module,
        Box::new(SimWorld::from_json(&world).unwrap()),
        Box::new(Scripted::from_json(&models).unwrap()),
    );
    arx.contracts = athc::contract_hashes(&c);
    let a = arx.run("A", "go", vec![Value::Text("x".into())]).unwrap().to_json();
    let dir = std::env::temp_dir().join(format!("ciclo1-keeping-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let py = ath_emit::python(&c.module, &c.check, &c.codex, "k.ath").expect("emissão");
    let (prog, w, m, j, r) =
        (dir.join("k.py"), dir.join("w.json"), dir.join("m.json"), dir.join("j.jsonl"), dir.join("r.json"));
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
    // os dois runtimes registram a mesma poda
    let prunes = |jl: &str| jl.lines().filter(|l| l.contains("\"prune\"")).count();
    assert_eq!(prunes(&arx.journal.to_jsonl()), prunes(&std::fs::read_to_string(&j).unwrap()), "{tag}: journal");
    (a, report["result"].clone())
}

fn pay(to: &str, cents: i64) -> J {
    json!({ "kind": "Pay", "to": to, "cents": cents })
}

fn check(src: &str, tag: &str, cases: &[(&str, J, &str)]) {
    for (i, (name, ops, want)) in cases.iter().enumerate() {
        let (a, p) = run_both(src, ops.clone(), &format!("{tag}{i}"));
        assert_eq!(a, json!(want), "ARX: {name}");
        assert_eq!(p, json!(want), "Python: {name}");
    }
}

#[test]
fn teto_corrido_na_ordem_proposta() {
    let note = json!({ "kind": "Note", "body": "n" });
    check(
        &prog(CAP, ""),
        "cap",
        &[
            ("plano vazio", json!([]), "0 mantidas, soma 0, 0 fora"),
            ("exatamente no teto", json!([pay("ana", 500)]), "1 mantidas, soma 500, 0 fora"),
            ("um centavo acima", json!([pay("ana", 501)]), "0 mantidas, soma 0, 1 fora"),
            ("soma no teto em duas", json!([pay("ana", 200), pay("bia", 300)]), "2 mantidas, soma 500, 0 fora"),
            // a ordem decide: a recusada não entra no total e a seguinte ainda é julgada
            (
                "300, 301, 200",
                json!([pay("ana", 300), pay("ana", 301), pay("ana", 200)]),
                "2 mantidas, soma 500, 1 fora",
            ),
            (
                "301, 300, 200",
                json!([pay("ana", 301), pay("ana", 300), pay("ana", 200)]),
                "1 mantidas, soma 301, 2 fora",
            ),
            (
                "200, 301, 300",
                json!([pay("ana", 200), pay("ana", 301), pay("ana", 300)]),
                "2 mantidas, soma 500, 1 fora",
            ),
            ("grande primeiro", json!([pay("ana", 600), pay("ana", 100)]), "1 mantidas, soma 100, 1 fora"),
            // recusada por outra regra (pessoa fora do inventário): também não conta no total
            (
                "estranho no meio",
                json!([pay("ana", 300), pay("eva", 100), pay("bia", 200)]),
                "2 mantidas, soma 500, 1 fora",
            ),
            ("todas recusadas", json!([pay("eva", 1), pay("ana", 501), pay("eva", 2)]), "0 mantidas, soma 0, 3 fora"),
            (
                "o que não é pagamento não soma",
                json!([note.clone(), pay("ana", 500), note.clone(), pay("ana", 1)]),
                "3 mantidas, soma 500, 1 fora",
            ),
        ],
    );
}

#[test]
fn autorizacao_integral() {
    check(
        &prog(PLAIN, "all "),
        "all",
        &[
            ("plano vazio", json!([]), "0 mantidas, soma 0, 0 fora"),
            ("nenhuma recusa", json!([pay("ana", 100), pay("bia", 200)]), "2 mantidas, soma 300, 0 fora"),
            ("recusa no fim", json!([pay("ana", 100), pay("bia", 200), pay("eva", 1)]), "0 mantidas, soma 0, 3 fora"),
            ("recusa no começo", json!([pay("eva", 1), pay("ana", 100)]), "0 mantidas, soma 0, 2 fora"),
            ("recusa no meio", json!([pay("ana", 100), pay("eva", 1), pay("bia", 200)]), "0 mantidas, soma 0, 3 fora"),
            ("todas recusadas", json!([pay("eva", 1), pay("eva", 2)]), "0 mantidas, soma 0, 2 fora"),
        ],
    );
    // controle: sem `all`, a mesma proposta executa o que passa
    check(
        &prog(PLAIN, ""),
        "parcial",
        &[(
            "recusa no meio, execução parcial",
            json!([pay("ana", 100), pay("eva", 1), pay("bia", 200)]),
            "2 mantidas, soma 300, 1 fora",
        )],
    );
    // os dois juntos: o teto corrido recusa a terceira, e então nada fica
    check(
        &prog(CAP, "all "),
        "capall",
        &[
            (
                "teto estourado na terceira",
                json!([pay("ana", 300), pay("ana", 200), pay("ana", 1)]),
                "0 mantidas, soma 0, 3 fora",
            ),
            ("dentro do teto", json!([pay("ana", 300), pay("ana", 200)]), "2 mantidas, soma 500, 0 fora"),
        ],
    );
}

#[test]
fn forma_da_politica_com_estado() {
    assert!(codes(&prog(CAP, "")).is_empty(), "{:?}", codes(&prog(CAP, "")));
    // segundo parâmetro que não é a lista do elemento
    let bad = CAP
        .replace("aceitos: List[Op, 0..4]", "aceitos: Int[0..500]")
        .replace("(sum a in aceitos => match a { Pay { cents } => cents, _ => 0 })", "aceitos");
    assert!(codes(&prog(&bad, "")).contains(&"E0437".to_string()));
    // lista menor que o campo: o estado não caberia
    let small = CAP.replace("List[Op, 0..4]", "List[Op, 0..3]");
    assert!(codes(&prog(&small, "")).contains(&"E0437".to_string()));
    // lista que não começa vazia
    let nonempty = CAP.replace("List[Op, 0..4]", "List[Op, 1..4]");
    assert!(codes(&prog(&nonempty, "")).contains(&"E0437".to_string()));
    // três parâmetros
    let three = CAP.replace("aceitos: List[Op, 0..4])", "aceitos: List[Op, 0..4], z: Op)");
    assert!(codes(&prog(&three, "")).contains(&"E0437".to_string()));
    // `keeping all by Q`: `all` continua podendo ser o nome do campo
    let field_all = prog(PLAIN, "")
        .replace("ops: List[Op, 0..4]", "all: List[Op, 0..4]")
        .replace("keeping ops by", "keeping all by")
        .replace("Plan { ops }", "Plan { all }")
        .replace("in ops", "in all")
        .replace("len(ops)", "len(all)");
    assert!(codes(&field_all).is_empty(), "{:?}", codes(&field_all));
}

/// O que ficou de fora continua sendo dado não confiável: não decide efeito.
#[test]
fn a_contagem_de_recusas_nao_autoriza_efeito() {
    let src = prog(PLAIN, "all ")
        .replace(
            "model planner",
            "tool pay(c: Cap[Bank.W], to: Text[..20] ^ Endorsed) -> Text ! { Bank.W } irrevocable latency 1.0\nmodel planner",
        )
        .replace("agent A()", "agent A(bank: Cap[Bank.W])")
        .replace("! {{ Infer }}", "! { Infer, Bank.W }")
        .replace("! { Infer }", "! { Infer, Bank.W }")
        .replace("    match p {", "    if len(fora) == 0 { commit pay(bank, \"ana\") } else { \"\" }\n    match p {");
    assert!(src.contains("commit pay"), "substituição do teste falhou");
    let c = codes(&src);
    assert!(c.contains(&"E0412".to_string()), "{c:?}");
}

/// Revisão independente, achado 1: uma política com estado chamada por outra recebia só o primeiro
/// argumento (ARX: variável não vinculada; Python: `TypeError` sem relatório).
#[test]
fn politica_com_estado_pode_ser_chamada_por_outra() {
    let q = CAP.replace("policy Q(", "policy Inner(")
        + "\npolicy Q(o: Op, aceitos: List[Op, 0..4]) -> Bool = Inner(o, aceitos)";
    check(
        &prog(&q, ""),
        "deleg",
        &[
            (
                "300, 301, 200",
                json!([pay("ana", 300), pay("ana", 301), pay("ana", 200)]),
                "2 mantidas, soma 500, 1 fora",
            ),
            ("estranho", json!([pay("eva", 1)]), "0 mantidas, soma 0, 1 fora"),
        ],
    );
}
