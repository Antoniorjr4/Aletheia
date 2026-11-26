//! **Ciclo 1 — achados da revisão independente de 26/11/2026, cada um com reprodução.**
//!
//! Um revisor que não escreveu as mudanças atacou as três capacidades pós-P3. Estes testes fixam o
//! que ele reproduziu (e dois defeitos anteriores que apareceram no caminho):
//!
//! * **D15** — um efeito irrevogável sem parâmetro `^ Endorsed` podia ser decidido por dado não
//!   confiável (`if web == "x" { commit wipe(c) }` compilava).
//! * **D16** — o valor de um ramo `Secret` saía de `if`/`match` como público.
//! * achado 4 — o veredito de uma política sobre um segredo saía público.
//! * achado 2 — estouro de 64 bits numa conta de política virava aceitação: agora E0451 estático.
//! * achado 5 — `contains("ana bia", nome)` contornava E0422.
//! * achados 1, 6, 7 e 11 — formas que o verificador aceitava e os runtimes não sustentavam.

use athc::compile;

fn codes(src: &str) -> Vec<String> {
    match compile(src) {
        Ok(_) => Vec::new(),
        Err(d) => d.into_iter().map(|x| x.code).collect(),
    }
}

fn agent(body: &str) -> String {
    format!(
        r#"module t
type T = | A1 | A2
type Plan = | Plan {{ n: Int[0..9] }}
model m {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
tool read_web(c: Cap[Repo.Read], k: Text[..8]) -> Text[..40] ^ Untrusted ! {{ Repo.Read }} reversible latency 1.0
tool read_list(c: Cap[Repo.Read], k: Text[..8]) -> List[Text[..8], 0..3] ^ Untrusted ! {{ Repo.Read }} reversible latency 1.0
tool read_private(c: Cap[Repo.Read], k: Text[..8]) -> Text[..40] ^ Secret ! {{ Repo.Read }} reversible latency 1.0
tool wipe(c: Cap[Repo.Write]) -> Unit ! {{ Repo.Write }} irrevocable latency 1.0
tool post(c: Cap[Repo.Write], body: Text) -> Unit ! {{ Repo.Write }} irrevocable latency 1.0
tool post_public(c: Cap[Repo.Write], body: Text ^ Public) -> Unit ! {{ Repo.Write }} irrevocable latency 1.0
policy Any(p: Plan) -> Bool = match p {{ Plan {{ n }} => s.within("cap", n) }}
policy HasX(t: Text[..40]) -> Bool = contains(lower(t), "x")
agent A(r: Cap[Repo.Read], w: Cap[Repo.Write]) budget [tok <= 1000, usd <= 1.0, wall <= 100.0] {{
  on go(n: Text[..8] ^ Trusted) -> Unit ! {{ Infer, Repo.Read, Repo.Write }} {{
{body}
  }}
}}
"#
    )
}

#[test]
fn d15_efeito_irrevogavel_nao_e_decidido_por_dado_nao_confiavel() {
    for b in [
        "    if read_web(r, n) == \"delete\" {\n      commit wipe(w)\n    } else {\n      commit post(w, \"ok\")\n    }",
        "    for x in read_list(r, n) {\n      commit wipe(w)\n    }",
        "    if contains(read_web(r, n), \"x\") {\n      commit post(w, \"a\")\n    } else {\n      commit post(w, \"b\")\n    }",
        // aninhado no `else` de um `refine`: o ramo interno continua decidido por dado
        "    let d = infer m -> Plan { q = n, cap = s.inventory(\"cap\") }\n    let p = refine d by Any else {\n      if read_web(r, n) == \"x\" {\n        commit wipe(w)\n      } else {\n        commit post(w, \"b\")\n      }\n      return commit post(w, \"fim\")\n    }\n    commit post(w, \"ok\")",
    ] {
        let c = codes(&agent(b));
        assert!(c.contains(&"E0412".to_string()), "{b}\n{c:?}");
    }
    // controles: ramo decidido pelo pedido confiável, e o `else` de um `refine` (veredito de uma
    // política auditada: o padrão "nenhum plano seguro → avisar" continua válido)
    for b in [
        "    if n == \"delete\" {\n      commit wipe(w)\n    } else {\n      commit post(w, \"ok\")\n    }",
        "    let d = infer m -> Plan { q = n, cap = s.inventory(\"cap\") }\n    let p = refine d by Any else {\n      commit post(w, \"nenhum plano seguro\")\n      return commit post(w, \"fim\")\n    }\n    commit post(w, \"ok\")",
    ] {
        assert!(codes(&agent(b)).is_empty(), "{b}\n{:?}", codes(&agent(b)));
    }
}

#[test]
fn d16_e_achado_4_segredo_nao_sai_por_ramo_nem_por_veredito() {
    for b in [
        "    let x = if n == \"a\" { read_private(r, n) } else { \"x\" }\n    commit post_public(w, x)",
        "    let x = match A1 { A1 => read_private(r, n), A2 => \"x\" }\n    commit post_public(w, x)",
        "    if HasX(read_private(r, n)) {\n      commit post_public(w, \"a\")\n    } else {\n      commit post_public(w, \"b\")\n    }",
    ] {
        let c = codes(&agent(b));
        assert!(c.contains(&"E0440".to_string()), "{b}\n{c:?}");
    }
    // controle: sem segredo, as mesmas formas passam
    for b in [
        "    let x = if n == \"a\" { n } else { \"x\" }\n    commit post_public(w, x)",
        "    if HasX(n) {\n      commit post_public(w, \"a\")\n    } else {\n      commit post_public(w, \"b\")\n    }",
    ] {
        assert!(codes(&agent(b)).is_empty(), "{b}\n{:?}", codes(&agent(b)));
    }
}

fn pol(decl: &str) -> String {
    format!("module p\ntype Op = | Pay {{ cents: Int[1..100] }} | Nop\n{decl}\n")
}

#[test]
fn achado_2_conta_sem_limite_provado_e_recusada_na_politica() {
    // limitada pelos tipos: aceita
    for ok in [
        "policy P(n: Int[0..9]) -> Bool = n + 1 <= 5",
        "policy P(n: Int[0..9]) -> Bool = {\n  let m = n\n  (m * 3) - 1 <= 5\n}",
        "policy P(xs: List[Op, 0..4]) -> Bool = (sum x in xs => match x { Pay { cents } => cents, Nop => 0 }) <= 500",
        "policy P(n: Int[0..9], d: Int[1..3]) -> Bool = n / d <= 5",
        "policy P(n: Int[0..9]) -> Bool = (if n <= 3 { n } else { 0 - n }) <= 5",
    ] {
        assert!(codes(&pol(ok)).is_empty(), "{ok}\n{:?}", codes(&pol(ok)));
    }
    // sem limite, com limite que estoura 64 bits, ou divisor que pode ser zero: E0451
    for bad in [
        "policy P(n: Int) -> Bool = n + 1 <= 5",
        "policy P(xs: List[Int, 0..4]) -> Bool = (sum x in xs => x) <= 500",
        "policy P(n: Int[1..9223372036854775807]) -> Bool = n + 400 <= 500",
        "policy P(n: Int[0..4000000000]) -> Bool = n * n <= 5",
        "policy P(xs: List[Int[0..4611686018427387904], 0..4]) -> Bool = (sum x in xs => x) <= 500",
        "policy P(n: Int[0..9], d: Int[0..3]) -> Bool = n / d <= 5",
        "policy P(n: Int[0..9], d: Int[-1..3]) -> Bool = n / d <= 5",
        "policy P(n: Int) -> Bool = 0 - n <= 5",
    ] {
        let c = codes(&pol(bad));
        assert!(c.contains(&"E0451".to_string()), "{bad}\n{c:?}");
    }
    // fora de política a aritmética continua livre (o efeito ainda exige o refinamento provado)
    let handler = "module p\nagent A() budget [tok <= 10, usd <= 1.0, wall <= 1.0] {\n  on go(n: Int ^ Trusted) -> Int ! { } { n + 1 }\n}\n";
    assert!(codes(handler).is_empty(), "{:?}", codes(handler));
}

#[test]
fn achado_5_inventario_escrito_a_mao_com_contains() {
    let src = |rule: &str| {
        format!(
            r#"module t
type Plan = | Plan {{ to: Text[..20] }}
model m {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
policy P(p: Plan) -> Bool = match p {{ Plan {{ to }} => {rule} }}
agent A() budget [tok <= 1000, usd <= 1.0, wall <= 100.0] {{
  on go(n: Text[..8] ^ Trusted) -> Text ! {{ Infer }} {{
    let d = infer m -> Plan {{ q = n, people = s.inventory("people") }}
    let p = refine d by P else {{ return "x" }}
    "ok"
  }}
}}
"#
        )
    };
    assert!(codes(&src(r#"s.member("people", to) || contains("eva ana", to)"#)).contains(&"E0422".to_string()));
    assert!(codes(&src(r#"s.member("people", to) || to == "eva""#)).contains(&"E0422".to_string()));
    // padrão procurado no valor não é inventário: continua aceito
    assert!(codes(&src(r#"s.member("people", to) && contains(to, "@") == false"#)).is_empty());
}

#[test]
fn achados_1_6_7_e_11_formas_que_o_verificador_agora_recusa() {
    let keep = |types: &str, q: &str| {
        format!(
            r#"module t
{types}
model m {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
{q}
policy P(p: Plan) -> Bool = true
agent A() budget [tok <= 1000, usd <= 1.0, wall <= 100.0] {{
  on go(n: Text[..8] ^ Trusted) -> Text ! {{ Infer }} {{
    let d = infer m -> Plan {{ q = n }}
    let p = refine d by P keeping ops by Q as fora else {{ return "x" }}
    "ok"
  }}
}}
"#
        )
    };
    let one = "type Plan = | Plan { ops: List[Int[1..100], 0..4] }";
    assert!(codes(&keep(one, "policy Q(n: Int[1..100], acc: List[Int[1..100], 0..4]) -> Bool = true")).is_empty());
    // 6: o elemento da lista de aceitos tem de ter os limites do elemento julgado, sem rótulo próprio
    assert!(codes(&keep(one, "policy Q(n: Int[1..100], acc: List[Int[1..5], 0..4]) -> Bool = true"))
        .contains(&"E0437".to_string()));
    assert!(codes(&keep(one, "policy Q(n: Int[1..100], acc: List[Int[1..100], 0..4] ^ Secret) -> Bool = true"))
        .contains(&"E0437".to_string()));
    // 7: o mesmo campo com tipos de elemento diferentes em variantes diferentes
    let two = "type Plan = | A { ops: List[Int[0..9], 0..4] } | B { ops: List[Text[..5], 0..2] }";
    assert!(codes(&keep(two, "policy Q(t: Text[..5]) -> Bool = true")).contains(&"E0437".to_string()));
    let two = "type Plan = | B { ops: List[Text[..5], 0..2] } | A { ops: List[Int[0..9], 0..4] }";
    assert!(codes(&keep(two, "policy Q(t: Text[..5]) -> Bool = true")).contains(&"E0437".to_string()));
    // 11: parâmetro repetido
    assert!(codes("module p\npolicy Q(x: Int[1..9], x: Int[1..9]) -> Bool = true\n").contains(&"E0002".to_string()));
}

/// **Reproduções dirigidas de 26/11 (pedidas pelo dono do projeto, encontradas por leitura).**
/// As três compilavam no commit `cbc3e52`; agora são recusadas, e os controles seguros passam.
#[test]
fn reproducoes_dirigidas_return_condicional_e_parametro_untrusted() {
    let src = |body: &str| {
        format!(
            r#"module t
tool read_web(c: Cap[Repo.Read], k: Text[..8]) -> Text[..40] ^ Untrusted ! {{ Repo.Read }} reversible latency 1.0
tool read_private(c: Cap[Repo.Read], k: Text[..8]) -> Text[..40] ^ Secret ! {{ Repo.Read }} reversible latency 1.0
tool wipe(c: Cap[Repo.Write]) -> Text ! {{ Repo.Write }} irrevocable latency 1.0
tool wipe_e(c: Cap[Repo.Write], why: Text[..40] ^ Endorsed) -> Text ! {{ Repo.Write }} irrevocable latency 1.0
tool note(c: Cap[Repo.Write], body: Text[..40] ^ Untrusted) -> Text ! {{ Repo.Write }} irrevocable latency 1.0
tool post_public(c: Cap[Repo.Write], body: Text ^ Public) -> Text ! {{ Repo.Write }} irrevocable latency 1.0
agent A(r: Cap[Repo.Read], w: Cap[Repo.Write]) budget [tok <= 100, usd <= 1.0, wall <= 100.0] {{
  on go(n: Text[..8] ^ Trusted) -> Text ! {{ Repo.Read, Repo.Write }} {{
{body}
  }}
}}
"#
        )
    };
    let refused: &[(&str, &str)] = &[
        // 1. `return` num ramo decidido por dado Untrusted; o efeito depois do `if` depende da condição
        ("    if read_web(r, n) == \"stop\" { return \"parou\" } else { \"\" }\n    commit wipe(w)", "E0412"),
        ("    if read_web(r, n) == \"stop\" { return \"parou\" } else { \"\" }\n    commit wipe_e(w, \"x\")", "E0412"),
        // o mesmo, mais fundo: o `return` está num `match` dentro do `if`, e o efeito vem depois de outro `if`
        ("    if read_web(r, n) == \"a\" { return \"a\" } else { \"\" }\n    if n == \"b\" { \"b\" } else { \"c\" }\n    commit wipe(w)", "E0412"),
        // 2. o mesmo com dado Secret, seguido de publicação em canal Public
        ("    if read_private(r, n) == \"a\" { return \"parou\" } else { \"\" }\n    commit post_public(w, \"ping\")", "E0440"),
        // 3. parâmetro `^ Untrusted` libera o conteúdo, não a decisão de o efeito acontecer
        ("    if read_web(r, n) == \"x\" { commit note(w, \"y\") } else { \"\" }", "E0412"),
        ("    let t = read_web(r, n)\n    if t == \"x\" { commit note(w, t) } else { \"\" }", "E0412"),
    ];
    for (b, code) in refused {
        let c = codes(&src(b));
        assert!(c.contains(&code.to_string()), "{b}\n{c:?}");
    }
    // controles seguros: não pode haver propagação indiscriminada
    for b in [
        // `return` decidido pelo pedido confiável
        "    if n == \"stop\" { return \"parou\" } else { \"\" }\n    commit wipe(w)",
        // `if` sobre dado sem `return`: o efeito depois dele não depende da condição
        "    if read_web(r, n) == \"stop\" { \"a\" } else { \"b\" }\n    commit wipe(w)",
        // conteúdo não confiável num parâmetro `^ Untrusted`, efeito decidido pelo pedido
        "    let t = read_web(r, n)\n    commit note(w, t)",
        // segredo inspecionado, mas o que vem depois não depende dele
        "    let s = read_private(r, n)\n    commit post_public(w, \"ping\")",
        // efeito ANTES do `return` condicional não depende dele
        "    commit wipe(w)\n    if read_web(r, n) == \"stop\" { return \"parou\" } else { \"\" }\n    \"fim\"",
    ] {
        assert!(codes(&src(b)).is_empty(), "{b}\n{:?}", codes(&src(b)));
    }
}
