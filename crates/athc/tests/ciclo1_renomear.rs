//! **Ciclo 1, pós-M11 — renomeação de campo em padrão (26/11/2026).**
//!
//! No M11, 15 das 16 autorias Aletheia escreveram `Use { item: k }` para comparar um campo com o
//! mesmo campo dos elementos já aceitos, e o compilador recusava com "esperado `}`, encontrado `:`".
//! Agora `Campo { campo: nome }` liga o valor do campo `campo` ao nome local `nome`; a forma
//! abreviada `Campo { campo }` continua igual. O nome local recebe tudo o que o campo tem: tipo,
//! limites, integridade, confidencialidade e origem. Estes testes fixam:
//!
//! * equivalência com o contorno por `let`, nos dois runtimes, inclusive nos `match` aninhados;
//! * campo inexistente, campo repetido, nome ligado duas vezes;
//! * renomear não apaga `Untrusted` nem `Secret`, nem libera efeito antes recusado;
//! * limites e obrigações levantadas (`requires`) seguem o campo real.

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

/// Unicidade por campo, na ordem proposta: o padrão que motivou a mudança.
fn prog(q: &str) -> String {
    format!(
        r#"module renomear
type Op = | Use {{ item: Text[..20], units: Int[1..50] }} | Note {{ body: Text[..20] }}
type Plan = | Plan {{ ops: List[Op, 0..4] }}
model planner {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 4000  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
{q}
policy P(p: Plan) -> Bool = true
agent A() budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {{
  on go(req: Text[..200] ^ Trusted) -> Text ! {{ Infer }} {{
    let d = infer planner -> Plan {{ request = req, items = s.inventory("items") }}
    let p = refine d by P keeping ops by Q as fora else {{ return "sem plano" }}
    match p {{ Plan {{ ops: kept }} =>
      text(len(kept)) ++ " mantidas, " ++ text(sum o in kept => match o {{ Use {{ units: u }} => u, _ => 0 }})
        ++ " unidades, " ++ text(len(fora)) ++ " fora"
    }}
  }}
}}
"#
    )
}

const RENOMEADO: &str = r#"policy Q(o: Op, aceitos: List[Op, 0..4]) -> Bool = match o {
  Use { item: k, units: n } => s.member("items", k) && n <= 40
    && (sum a in aceitos => match a { Use { item } => if item == k { 1 } else { 0 }, _ => 0 }) == 0,
  Note { .. } => true,
}"#;
const COM_LET: &str = r#"policy Q(o: Op, aceitos: List[Op, 0..4]) -> Bool = match o {
  Use { item, units } => { let k = item
    let n = units
    s.member("items", k) && n <= 40
      && (sum a in aceitos => match a { Use { item } => if item == k { 1 } else { 0 }, _ => 0 }) == 0 },
  Note { .. } => true,
}"#;
// renomeação dos dois lados do `match` aninhado: nenhum nome se repete
const DOS_DOIS_LADOS: &str = r#"policy Q(o: Op, aceitos: List[Op, 0..4]) -> Bool = match o {
  Use { item: novo, units: n } => s.member("items", novo) && n <= 40
    && (sum a in aceitos => match a { Use { item: velho } => if velho == novo { 1 } else { 0 }, _ => 0 }) == 0,
  Note { .. } => true,
}"#;

fn run_both(src: &str, ops: J, tag: &str) -> (J, J) {
    let c = compile(src).unwrap_or_else(|d| panic!("{:?}", d.iter().map(|x| (&x.code, &x.msg)).collect::<Vec<_>>()));
    let models = json!({ "planner": [{ "kind": "Plan", "ops": ops }] });
    let world = json!({ "inventories": { "items": ["bolt", "nut"] } });
    let mut arx = Arx::new(
        &c.module,
        Box::new(SimWorld::from_json(&world).unwrap()),
        Box::new(Scripted::from_json(&models).unwrap()),
    );
    arx.contracts = athc::contract_hashes(&c);
    let a = arx.run("A", "go", vec![Value::Text("x".into())]).unwrap().to_json();
    let dir = std::env::temp_dir().join(format!("ciclo1-renomear-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let py = ath_emit::python(&c.module, &c.check, &c.codex, "r.ath").expect("emissão");
    let (prog, w, m, j, r) =
        (dir.join("r.py"), dir.join("w.json"), dir.join("m.json"), dir.join("j.jsonl"), dir.join("r.json"));
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
    (a, report["result"].clone())
}

fn use_(item: &str, units: i64) -> J {
    json!({ "kind": "Use", "item": item, "units": units })
}

#[test]
fn renomeacao_equivale_ao_contorno_por_let_nos_dois_runtimes() {
    let note = json!({ "kind": "Note", "body": "n" });
    // esperados derivados à mão: item do inventário, no máximo 40 unidades, um por item, na ordem
    let casos = [
        ("vazio", json!([]), "0 mantidas, 0 unidades, 0 fora"),
        ("dois itens", json!([use_("bolt", 3), use_("nut", 4)]), "2 mantidas, 7 unidades, 0 fora"),
        ("repetido", json!([use_("bolt", 3), use_("bolt", 4)]), "1 mantidas, 3 unidades, 1 fora"),
        (
            "repetido depois de outro",
            json!([use_("bolt", 3), use_("nut", 1), use_("bolt", 9)]),
            "2 mantidas, 4 unidades, 1 fora",
        ),
        // o recusado não entra na lista dos aceitos: o seguinte com o mesmo item ainda pode entrar
        ("recusado não conta", json!([use_("bolt", 45), use_("bolt", 5)]), "1 mantidas, 5 unidades, 1 fora"),
        ("fora do inventário", json!([use_("gear", 1), note.clone()]), "1 mantidas, 0 unidades, 1 fora"),
    ];
    for (tag, q) in [("ren", RENOMEADO), ("let", COM_LET), ("dois", DOS_DOIS_LADOS)] {
        for (i, (nome, ops, want)) in casos.iter().enumerate() {
            let (a, p) = run_both(&prog(q), ops.clone(), &format!("{tag}{i}"));
            assert_eq!(a, json!(want), "ARX, {tag}: {nome}");
            assert_eq!(p, json!(want), "Python, {tag}: {nome}");
        }
    }
}

#[test]
fn campo_inexistente_repetido_e_nome_ligado_duas_vezes() {
    let q = |arm: &str| {
        prog(&format!(
            "policy Q(o: Op, aceitos: List[Op, 0..4]) -> Bool = match o {{\n  {arm},\n  Note {{ .. }} => true,\n}}"
        ))
    };
    // campo inexistente: o erro aponta o campo, não o nome local
    assert_eq!(codes(&q("Use { iten: k } => s.member(\"items\", k)")), vec!["E0010"]);
    // o mesmo campo duas vezes, renomeado ou não
    assert_eq!(codes(&q("Use { item: a, item: b } => s.member(\"items\", a)")), vec!["E0014"]);
    assert_eq!(codes(&q("Use { item, item: b } => s.member(\"items\", b)")), vec!["E0014"]);
    // dois campos no mesmo nome local, e um nome local igual a outro campo abreviado
    assert_eq!(codes(&q("Use { item: x, units: x } => x == x")), vec!["E0014"]);
    assert_eq!(codes(&q("Use { units: item, item } => s.member(\"items\", item)")), vec!["E0014"]);
    // o nome local só existe no braço: fora dele, `k` não existe
    let fora = prog(
        "policy Q(o: Op, aceitos: List[Op, 0..4]) -> Bool = { let ok = match o { Use { item: k } => s.member(\"items\", k), Note { .. } => true }\n  ok && k == k }",
    );
    assert!(!codes(&fora).is_empty());
    // o campo real continua invisível como nome quando foi renomeado
    assert!(!codes(&q("Use { item: k } => s.member(\"items\", item)")).is_empty());
    // a forma `campo = nome` não é aceita, e a mensagem diz a forma certa
    let e = compile(&q("Use { item = k } => s.member(\"items\", k)")).err().unwrap();
    assert_eq!(e[0].code, "P0001");
    assert!(e[0].msg.contains("item: nome"), "{}", e[0].msg);
    // controles: a renomeação de um campo não muda o nome que o braço vizinho usa
    assert!(codes(&q("Use { item: k, units } => s.member(\"items\", k) && units <= 40")).is_empty());
    assert!(codes(&q("Use { item: _ } => true")).is_empty());
    // `campo: _` também confere o campo (revisão independente, achado A2)
    assert_eq!(codes(&q("Use { iten: _ } => true")), vec!["E0010"]);
    assert_eq!(codes(&q("Use { item: _, item: _ } => true")), vec!["E0014"]);
    assert_eq!(codes(&q("Use { item: k, item: _ } => s.member(\"items\", k)")), vec!["E0014"]);
}

#[test]
fn o_nome_local_nao_entra_no_hash() {
    // revisão independente, achado A1: como em `let` e `for`, o nome local é posição no Codex
    let root = |arm: &str| {
        let c = compile(&prog(&format!(
            "policy Q(o: Op, aceitos: List[Op, 0..4]) -> Bool = match o {{\n  {arm},\n  Note {{ .. }} => true,\n}}"
        )))
        .unwrap_or_else(|d| panic!("{:?}", d.iter().map(|x| &x.msg).collect::<Vec<_>>()));
        c.codex.root.clone()
    };
    let a = root("Use { item: k } => s.member(\"items\", k)");
    assert_eq!(a, root("Use { item: j } => s.member(\"items\", j)"));
    assert_eq!(a, root("Use { item } => s.member(\"items\", item)"));
    assert_eq!(a, root("Use { item: item } => s.member(\"items\", item)"));
    // campo diferente é programa diferente
    assert_ne!(a, root("Use { units: k } => k <= 3"));
}

#[test]
fn limites_do_campo_seguem_para_o_nome_local() {
    // `units: Int[1..50]`: `n * 2` é limitado (E0451 não dispara), igual à forma abreviada
    let q = |arm: &str| {
        prog(&format!(
            "policy Q(o: Op, aceitos: List[Op, 0..4]) -> Bool = match o {{\n  {arm},\n  Note {{ .. }} => true,\n}}"
        ))
    };
    assert!(codes(&q("Use { units: n } => n * 2 <= 60")).is_empty());
    assert!(codes(&q("Use { units } => units * 2 <= 60")).is_empty());
    // e o tipo também: comparar o número com texto é erro nas duas formas
    assert_eq!(codes(&q("Use { units: n } => n == \"x\"")), codes(&q("Use { units } => units == \"x\"")));
    assert!(!codes(&q("Use { units: n } => n == \"x\"")).is_empty());
}

fn agent(body: &str) -> String {
    format!(
        r#"module t
type Rec = | Rec {{ body: Text[..40], n: Int[0..9] }}
tool read_web(c: Cap[Repo.Read]) -> Rec ^ Untrusted ! {{ Repo.Read }} reversible latency 1.0
tool read_private(c: Cap[Repo.Read]) -> Rec ^ Secret ! {{ Repo.Read }} reversible latency 1.0
tool read_own(c: Cap[Repo.Read]) -> Rec ! {{ Repo.Read }} reversible latency 1.0
tool wipe(c: Cap[Repo.Write]) -> Unit ! {{ Repo.Write }} irrevocable latency 1.0
tool post(c: Cap[Repo.Write], body: Text ^ Endorsed) -> Unit ! {{ Repo.Write }} irrevocable latency 1.0
tool post_public(c: Cap[Repo.Write], body: Text ^ Public) -> Unit ! {{ Repo.Write }} irrevocable latency 1.0
tool set(c: Cap[Repo.Write], n: Int[0..9]) -> Unit ! {{ Repo.Write }} reversible latency 1.0
agent A(r: Cap[Repo.Read], w: Cap[Repo.Write]) budget [tok <= 1000, usd <= 1.0, wall <= 100.0] {{
  on go(q: Text[..8] ^ Trusted) -> Unit ! {{ Repo.Read, Repo.Write }} {{
{body}
  }}
}}
"#
    )
}

#[test]
fn renomear_nao_apaga_untrusted_nem_secret() {
    // cada par: forma abreviada e renomeada; as duas têm de ser recusadas pelo mesmo motivo
    let pares = [
        // dado não confiável num efeito que exige endosso
        (
            "E0412",
            "    match read_web(r) { Rec { body } => commit post(w, body) }",
            "    match read_web(r) { Rec { body: b } => commit post(w, b) }",
        ),
        // dado não confiável decidindo um efeito irrevogável (D15)
        (
            "E0412",
            "    match read_web(r) { Rec { body } => if body == \"go\" { commit wipe(w) } else { set(w, 1) } }",
            "    match read_web(r) { Rec { body: b } => if b == \"go\" { commit wipe(w) } else { set(w, 1) } }",
        ),
        // segredo numa saída pública
        (
            "E0440",
            "    match read_private(r) { Rec { body } => commit post_public(w, body) }",
            "    match read_private(r) { Rec { body: segredo } => commit post_public(w, segredo) }",
        ),
        // o segredo passado adiante por `let` depois de renomeado
        (
            "E0440",
            "    match read_private(r) { Rec { body } => { let t = body\n commit post_public(w, t) } }",
            "    match read_private(r) { Rec { body: x } => { let t = x\n commit post_public(w, t) } }",
        ),
    ];
    for (code, curto, renomeado) in pares {
        let (a, b) = (codes(&agent(curto)), codes(&agent(renomeado)));
        assert!(a.contains(&code.to_string()), "{curto}\n{a:?}");
        assert!(b.contains(&code.to_string()), "{renomeado}\n{b:?}");
        assert_eq!(a, b, "a renomeação mudou os diagnósticos:\n{renomeado}");
    }
    // a origem continua no diagnóstico: o nome local não esconde de onde veio o dado
    let d = compile(&agent("    match read_web(r) { Rec { body: b } => commit post(w, b) }")).err().unwrap();
    let e = d.iter().find(|x| x.code == "E0412").unwrap();
    let texto = format!("{} {} {:?} {:?}", e.msg, e.label, e.notes, e.secondary);
    assert!(texto.contains("read_web"), "{texto}");
    // controles: com dado confiável, as mesmas formas passam (e o limite do campo prova o parâmetro)
    for b in [
        "    match read_own(r) { Rec { body: b } => commit post_public(w, b) }",
        "    match read_own(r) { Rec { n: k } => set(w, k) }",
    ] {
        assert!(codes(&agent(b)).is_empty(), "{b}\n{:?}", codes(&agent(b)));
    }
}

#[test]
fn obrigacoes_levantadas_seguem_o_campo_real() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let orig = std::fs::read_to_string(root.join("examples/rc4/treasury.ath")).unwrap();
    let ren = orig
        .replace("Plan { pays, finals, answer } => {", "Plan { pays: ps, finals: fs, answer: resposta } => {")
        .replace("for p in pays {", "for p in ps {")
        .replace("for f in finals {", "for f in fs {")
        .replace(
            "commit receipt(bank, answer, keys.mint())\n        answer",
            "commit receipt(bank, resposta, keys.mint())\n        resposta",
        )
        .replace(
            "Pay { to, amount, memo } => pay(bank, to, amount, memo, keys.mint())",
            "Pay { to: d, amount: v, memo: m } => pay(bank, d, v, m, keys.mint())",
        )
        .replace(
            "Wire { to, amount }           => commit wire(bank, to, amount, keys.mint())",
            "Wire { to: x, amount: y } => commit wire(bank, x, y, keys.mint())",
        );
    assert_ne!(orig, ren, "as substituições não pegaram");
    assert!(ren.contains("Wire { to: x, amount: y }") && ren.contains("Pay { to: d, amount: v, memo: m }"));
    let lifted = |src: &str| {
        let c =
            compile(src).unwrap_or_else(|d| panic!("{:?}", d.iter().map(|x| (&x.code, &x.msg)).collect::<Vec<_>>()));
        let py = ath_emit::python(&c.module, &c.check, &c.codex, "t.ath").unwrap();
        let i = py.find("lifted=[").expect("obrigações levantadas");
        py[i..i + py[i..].find("])").unwrap()].to_string()
    };
    // mesmos contratos, mesmos caminhos, mesmos parâmetros ← campos
    assert_eq!(lifted(&orig), lifted(&ren));
}
