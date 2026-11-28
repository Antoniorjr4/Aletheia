//! **Ciclo 1, pós-P3 — propriedades de texto na política (26/11/2026).**
//!
//! No P3, os pedidos C2 (senha com letra e dígito) e C6 (assunto sem `http://`, `https://`, `www.`,
//! sem distinguir caixa, em qualquer posição) não cabiam na política: a regra foi para o host.
//! Quatro funções puras e totais fecham a lacuna:
//!
//! | função | resultado | definição |
//! | --- | --- | --- |
//! | `contains(t, p)` | Bool | `p` ocorre em `t`, comparando pontos de código, exato; `""` ocorre sempre |
//! | `lower(t)` | Text | só `A`–`Z` viram `a`–`z`; qualquer outro caractere fica como está |
//! | `ascii_letters(t)` | Int | quantos caracteres em `A`–`Z` ou `a`–`z` |
//! | `ascii_digits(t)` | Int | quantos caracteres em `0`–`9` |
//!
//! "Sem distinguir caixa" é, portanto, ASCII: não há dobra Unicode, normalização nem equivalência
//! de compatibilidade (`ＷＷＷ.` de largura total não é `www.`). É uma escolha declarada, igual nos
//! dois runtimes; não é uma avaliação de segurança de senha nem um reconhecedor de URL.
//! Os valores esperados abaixo foram escritos à mão a partir dessa definição.

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

const PROBE: &str = r#"module texto
agent A() budget [tok <= 100, usd <= 1.0, wall <= 100.0] {
  on go(x: Text[..40] ^ Trusted, p: Text[..10] ^ Trusted) -> Text ! { } {
    text(ascii_letters(x)) ++ "|" ++ text(ascii_digits(x)) ++ "|" ++ lower(x) ++ "|"
      ++ (if contains(x, p) { "sim" } else { "nao" }) ++ "|" ++ (if contains(lower(x), p) { "sim" } else { "nao" })
  }
}
"#;

fn run_both(src: &str, args: &[&str], tag: &str) -> (String, String) {
    let c = compile(src).unwrap_or_else(|d| panic!("{:?}", d.iter().map(|x| (&x.code, &x.msg)).collect::<Vec<_>>()));
    let world = json!({});
    let models = json!({});
    let mut arx = Arx::new(
        &c.module,
        Box::new(SimWorld::from_json(&world).unwrap()),
        Box::new(Scripted::from_json(&models).unwrap()),
    );
    arx.contracts = athc::contract_hashes(&c);
    let a = arx.run("A", "go", args.iter().map(|x| Value::Text((*x).into())).collect()).unwrap().to_json();
    let dir = std::env::temp_dir().join(format!("ciclo1-texto-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let py = ath_emit::python(&c.module, &c.check, &c.codex, "t.ath").expect("emissão");
    let (prog, w, m, j, r) =
        (dir.join("t.py"), dir.join("w.json"), dir.join("m.json"), dir.join("j.jsonl"), dir.join("r.json"));
    std::fs::write(&prog, py).unwrap();
    std::fs::write(&w, world.to_string()).unwrap();
    std::fs::write(&m, models.to_string()).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut cmd = std::process::Command::new("python3");
    cmd.env("PYTHONPATH", root.join("runtimes/python"))
        .args(["-m", "aletheia_runtime", "run"])
        .arg(&prog)
        .arg("--world")
        .arg(&w)
        .arg("--models")
        .arg(&m)
        .args(["--provider", "scripted", "--agent", "A", "--on", "go"]);
    for x in args {
        cmd.arg("--arg-json").arg(json!(x).to_string());
    }
    let st = cmd.arg("--journal").arg(&j).arg("--report").arg(&r).output().expect("python3");
    assert!(r.exists(), "{}", String::from_utf8_lossy(&st.stderr));
    let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&r).unwrap()).unwrap();
    (a.as_str().unwrap().to_string(), report["result"].as_str().unwrap_or("<não texto>").to_string())
}

#[test]
fn definicao_das_quatro_funcoes_nos_dois_runtimes() {
    // (texto, trecho, esperado: letras|dígitos|lower|contains exato|contains depois de lower)
    let cases: &[(&str, &str, &str)] = &[
        ("", "", "0|0||sim|sim"),
        ("", "a", "0|0||nao|nao"),
        ("abc", "", "3|0|abc|sim|sim"),
        ("See WWW.Evil.com", "www.", "13|0|see www.evil.com|nao|sim"),
        ("seewww.x", "www.", "7|0|seewww.x|sim|sim"),
        ("www", "www.", "3|0|www|nao|nao"),
        ("HtTpS://x", "https://", "6|0|https://x|nao|sim"),
        // largura total: não é ASCII, não muda de caixa, não conta como letra
        ("ＷＷＷ.x", "www.", "1|0|ＷＷＷ.x|nao|nao"),
        // sinal Kelvin (U+212A) e I com ponto (U+0130): `str.lower` do Python os mudaria
        ("\u{212A}\u{0130}Z9", "z", "1|1|\u{212A}\u{0130}z9|nao|sim"),
        // dígitos não ASCII (arábico-índico, sobrescrito) não contam
        ("a\u{0663}\u{00B2}7", "7", "1|1|a\u{0663}\u{00B2}7|sim|sim"),
        ("É1é", "é", "0|1|É1é|sim|sim"),
        // emoji: um caractere, não letra; `contains` compara pontos de código
        ("p\u{1F600}9", "\u{1F600}", "1|1|p\u{1F600}9|sim|sim"),
        ("Aa0 Zz9", "a0", "4|2|aa0 zz9|sim|sim"),
    ];
    for (i, (x, p, want)) in cases.iter().enumerate() {
        let (a, py) = run_both(PROBE, &[x, p], &i.to_string());
        assert_eq!(&a, want, "ARX: {x:?} {p:?}");
        assert_eq!(&py, want, "Python: {x:?} {p:?}");
    }
}

#[test]
fn tipos_e_aridade() {
    let p = |body: &str| format!("module t\npolicy P(t: Text[..9], n: Int[0..9]) -> Bool = {body}\n");
    assert!(codes(&p(r#"contains(lower(t), "x") && ascii_letters(t) + ascii_digits(t) <= 9"#)).is_empty());
    assert!(!codes(&p("contains(t)")).is_empty());
    assert!(!codes(&p(r#"contains(n, "x")"#)).is_empty());
    assert!(!codes(&p(r#"contains(t, n)"#)).is_empty());
    assert!(!codes(&p("ascii_digits(n) == 0")).is_empty());
    assert!(!codes(&p("lower(t)")).is_empty()); // política devolve Bool, não Text
}

/// Inspecionar um texto não o endossa nem o publica: o rótulo do resultado é o do argumento.
#[test]
fn o_resultado_herda_integridade_e_confidencialidade() {
    let src = |body: &str| {
        format!(
            r#"module t
tool read_private(c: Cap[Repo.Read], k: Text[..8]) -> Text[..40] ^ Secret ! {{ Repo.Read }} reversible latency 1.0
tool read_web(c: Cap[Repo.Read], k: Text[..8]) -> Text[..40] ^ Untrusted ! {{ Repo.Read }} reversible latency 1.0
tool post_public(c: Cap[Repo.Write], body: Text[..600] ^ Public) -> Unit ! {{ Repo.Write }} irrevocable latency 1.0
tool post_endorsed(c: Cap[Repo.Write], body: Text[..600] ^ Endorsed) -> Unit ! {{ Repo.Write }} irrevocable latency 1.0
agent A(r: Cap[Repo.Read], w: Cap[Repo.Write]) budget [tok <= 100, usd <= 1.0, wall <= 100.0] {{
  on go(n: Text[..8] ^ Trusted) -> Unit ! {{ Repo.Read, Repo.Write }} {{
{body}
  }}
}}
"#
        )
    };
    // segredo: nem o texto minúsculo, nem as contagens, nem o resultado de `contains` saem
    for b in [
        "    commit post_public(w, lower(read_private(r, n)))",
        "    commit post_public(w, text(ascii_digits(read_private(r, n))))",
        "    commit post_public(w, text(ascii_letters(read_private(r, n))))",
        "    if contains(read_private(r, n), \"x\") {\n      commit post_public(w, \"a\")\n    } else {\n      commit post_public(w, \"b\")\n    }",
        "    if contains(\"abc\", read_private(r, n)) {\n      commit post_public(w, \"a\")\n    } else {\n      commit post_public(w, \"b\")\n    }",
    ] {
        let c = codes(&src(b));
        assert!(c.contains(&"E0440".to_string()), "{b}\n{c:?}");
    }
    // não confiável: `lower` não endossa o texto, e `contains` sobre ele não pode decidir um efeito
    for b in [
        "    commit post_endorsed(w, lower(read_web(r, n)))",
        "    if contains(read_web(r, n), \"x\") {\n      commit post_endorsed(w, \"a\")\n    } else {\n      commit post_endorsed(w, \"b\")\n    }",
        // a mesma regra de sempre, com `==` no lugar de `contains` (controle da regra, não da função)
        "    if read_web(r, n) == \"x\" {\n      commit post_endorsed(w, \"a\")\n    } else {\n      commit post_endorsed(w, \"b\")\n    }",
    ] {
        assert!(!codes(&src(b)).is_empty(), "{b}");
    }
    // controle: sobre o pedido confiável, as mesmas formas passam
    for b in [
        "    commit post_public(w, lower(n))",
        "    commit post_endorsed(w, lower(n))",
        "    if contains(n, \"x\") {\n      commit post_public(w, \"a\")\n    } else {\n      commit post_public(w, \"b\")\n    }",
    ] {
        assert!(codes(&src(b)).is_empty(), "{b}\n{:?}", codes(&src(b)));
    }
}
