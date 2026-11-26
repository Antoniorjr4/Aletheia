//! **Ciclo 1, pós-M11 — dois diagnósticos que não diziam o que fazer (26/11/2026).**
//!
//! * **E0422.** A recusa estava certa (a política comparava texto com literal), mas o diagnóstico
//!   apontava a cláusula `keeping`, citava `s.managed()` mesmo quando o prompt não o recebia e
//!   sugeria `s.is_managed(x)`, uma função da suíte bancária. Agora ele aponta a comparação
//!   responsável e orienta pelas listas que este programa entrega ao modelo. A regra não mudou.
//! * **Palavra reservada como nome.** `let authority = …` dava "esperado identificador, encontrado
//!   `authority`". Agora a mensagem diz que é palavra reservada e onde está.

use athc::compile;
use std::path::PathBuf;

fn text_at(src: &str, lo: u32, hi: u32) -> &str {
    &src[lo as usize..hi as usize]
}

fn m11_like(cmp: &str) -> String {
    format!(
        r#"module ex
type Op = | Move {{ opp: Text[..20], stage: Text[..20] }}
type Plan = | Plan {{ ops: List[Op, 0..4], answer: Text[..200] }}
model m {{ price_in = 0.15  price_out = 0.60  ctx = 131072  max_out = 2000  reasoning = 0  ttft = 1.0  per_tok = 0.05 }}
policy Ok(o: Op) -> Bool = match o {{ Move {{ opp, stage }} => s.binds("next_stages", opp, stage) && {cmp} }}
policy P(p: Plan) -> Bool = true
agent A() budget [tok <= 50000, usd <= 0.10, wall <= 100.0] {{
  on task(request: Text[..200] ^ Trusted) -> Text ! {{ Infer }} {{
    let d = infer m -> Plan {{ task = "x", request = request, stages = s.inventory("next_stages") }}
    let plan = refine d by P keeping ops by Ok as left retry 1 else {{ Plan {{ ops: [], answer: "no" }} }}
    match plan {{ Plan {{ answer }} => answer }}
  }}
}}
"#
    )
}

#[test]
fn e0422_aponta_a_comparacao_e_orienta_pelas_listas_do_programa() {
    for cmp in [r#"stage != "closed_won""#, r#""closed_won" == stage"#, r#"contains("won lost", stage) == false"#] {
        let src = m11_like(cmp);
        let d = compile(&src).err().unwrap_or_default();
        let e = d.iter().find(|x| x.code == "E0422").unwrap_or_else(|| panic!("{cmp}: {d:?}"));
        // a regra de segurança continua: é recusa, e só esta
        assert_eq!(d.iter().map(|x| x.code.as_str()).collect::<Vec<_>>(), ["E0422"], "{cmp}");
        // o span principal é a expressão responsável
        let at = text_at(&src, e.span.lo, e.span.hi);
        assert!(cmp.starts_with(at) || at == cmp || cmp.contains(at), "{cmp}: aponta `{at}`");
        assert!(at.contains("closed_won") || at.contains("won lost"), "{cmp}: aponta `{at}`");
        // a cláusula que usa a política aparece como contexto
        assert!(e.secondary.iter().any(|(s, _)| text_at(&src, s.lo, s.hi) == "Ok"), "{:?}", e.secondary);
        // a orientação cita a lista que o prompt recebe, e nada da suíte bancária
        let all = format!("{} {} {} {}", e.msg, e.label, e.notes.join(" "), e.help.as_deref().unwrap_or(""));
        assert!(all.contains(r#"s.member("next_stages""#), "{all}");
        assert!(!all.contains("managed"), "{all}");
    }
}

#[test]
fn e0422_no_programa_que_usa_managed_orienta_por_managed() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/remediate.ath");
    let src = std::fs::read_to_string(p).unwrap().replace(
        "Restart { deploy } => s.is_managed(deploy),",
        r#"Restart { deploy } => deploy == "api" || deploy == "worker","#,
    );
    let d = compile(&src).err().unwrap();
    assert_eq!(d.iter().map(|x| x.code.as_str()).collect::<Vec<_>>(), ["E0422"]);
    let e = &d[0];
    assert!(text_at(&src, e.span.lo, e.span.hi).contains("\"api\""), "{}", text_at(&src, e.span.lo, e.span.hi));
    assert!(e.help.as_deref().unwrap_or("").contains("s.is_managed"), "{:?}", e.help);
}

#[test]
fn controle_sem_literal_continua_aceito() {
    let src = m11_like(r#"s.member("next_stages", opp) == false || true"#);
    let d = compile(&src).err().unwrap_or_default();
    assert!(d.iter().all(|x| x.code != "E0422"), "{d:?}");
}

#[test]
fn palavra_reservada_como_nome_diz_a_causa_e_o_lugar() {
    let casos = [("let authority = 1", "authority"), ("let policy = 1", "policy"), ("let model = 1", "model")];
    for (stmt, w) in casos {
        let src = format!("module t\ntype R = | R {{ a: Int[0..9] }}\npolicy P(r: R) -> Bool = {{ {stmt}\n  true }}\n");
        let d = compile(&src).err().unwrap();
        assert_eq!(d[0].code, "P0001");
        assert!(d[0].msg.contains(&format!("`{w}` é palavra reservada")), "{}", d[0].msg);
        assert!(d[0].msg.contains("esperado um nome"), "{}", d[0].msg);
        assert_eq!(text_at(&src, d[0].span.lo, d[0].span.hi), w);
    }
    // campo de `infer` com nome reservado: o caso do M11 (`policy = quote(…)`)
    let src = m11_like("true").replace("task = \"x\",", "policy = \"x\",");
    let d = compile(&src).err().unwrap();
    assert!(d[0].msg.contains("`policy` é palavra reservada"), "{}", d[0].msg);
    // nome que faltou antes de uma palavra reservada: a mensagem não afirma que ela era o nome
    let src = m11_like("true").replace("refine d by P keeping", "refine d by else keeping");
    let d = compile(&src).err().unwrap();
    assert!(d[0].msg.starts_with("esperado um nome, encontrado `else`"), "{}", d[0].msg);
    assert!(d[0].msg.contains("se era o nome"), "{}", d[0].msg);
    // um nome comum que só começa com a palavra continua sendo nome
    let ok = "module t\ntype R = | R { a: Int[0..9] }\npolicy P(r: R) -> Bool = { let authority_cents = 1\n  authority_cents == 1 }\n";
    assert!(compile(ok).is_ok(), "{:?}", compile(ok).err().map(|d| d[0].msg.clone()));
}
