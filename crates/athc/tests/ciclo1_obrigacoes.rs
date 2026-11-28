//! **Ciclo 1 — B1 e B2 na análise de obrigações (27/11/2026).**
//!
//! Achados da revisão independente do lote pós-M11, anteriores a ele (o compilador congelado
//! `223fb53` também os tinha). Em ambos o compilador afirmava "obrigação descarregada" sem que ela
//! estivesse demonstrada, e o monitor do efeito abortava com R0631 ("defeito da análise"):
//!
//! * **B1:** dois `for` sobre a mesma lista endossada produziam o mesmo caminho `[ops, *]`. A análise
//!   juntava num só "elemento" argumentos de elementos diferentes. Agora o elemento é identificado
//!   pelo laço que o escolheu.
//! * **B2:** `let q = refine … keeping … as fora` não ligava `fora` na análise, e uma ligação externa
//!   com o mesmo nome emprestava sua origem endossada. Agora o nome é ligado ao que a política
//!   recusou, com origem desconhecida.
//!
//! As reproduções e os controles ficam em `bench/obrigacoes/casos`, versionados antes do reparo
//! (`bench/obrigacoes/antes.json`). Aqui eles viram regressões: o que o compilador afirma, quais
//! chamadas chegam à ferramenta e com qual código cada runtime para.

use arx::host::SimWorld;
use arx::model::Scripted;
use arx::{Arx, Value};
use ath_syntax::obligations::{analyze, Status};
use athc::compile;
use serde_json::{json, Value as J};
use std::path::PathBuf;

fn casos() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/obrigacoes/casos")
}

fn src(nome: &str) -> String {
    std::fs::read_to_string(casos().join(format!("{nome}.ath"))).unwrap()
}

/// O destino de cada sítio com `requires`: `Ok(n refines)` se descarregada, `Err(motivo)` se dinâmica.
fn sitios(nome: &str) -> Vec<Result<usize, String>> {
    // sobre a AST: desde 28/11 a E0453 recusa na compilação as reproduções que misturam
    // elementos (B1, F2); a análise de obrigações continua conferida sobre elas
    let m = ath_syntax::parser::parse_module(&src(nome)).unwrap_or_else(|e| panic!("{nome}: {e:?}"));
    analyze(&m)
        .sites
        .iter()
        .map(|s| match &s.status {
            Status::Discharged { refines } => Ok(refines.len()),
            Status::Dynamic { reason } => Err(reason.clone()),
        })
        .collect()
}

/// As chamadas que chegaram à ferramenta (sem a capacidade) e o código de aborto, nos dois runtimes.
fn executa(nome: &str, mundo: &str, roteiro: &str) -> [(Vec<J>, Option<String>); 2] {
    executa_com(nome, mundo, roteiro, &[json!("x")])
}

/// Como `executa`, com os argumentos do evento (texto ou booleano).
fn executa_com(nome: &str, mundo: &str, roteiro: &str, args: &[J]) -> [(Vec<J>, Option<String>); 2] {
    let c = compile(&src(nome)).unwrap();
    let world: J =
        serde_json::from_str(&std::fs::read_to_string(casos().join(format!("{mundo}.json"))).unwrap()).unwrap();
    let models: J =
        serde_json::from_str(&std::fs::read_to_string(casos().join(format!("{roteiro}.json"))).unwrap()).unwrap();
    let calls = |jsonl: &str| -> (Vec<J>, Option<String>) {
        let es: Vec<J> =
            jsonl.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect();
        let tools = es
            .iter()
            .filter(|e| e["kind"] == "tool")
            .map(|e| {
                J::Array(e["detail"]["args"].as_array().unwrap().iter().filter(|a| !a.is_object()).cloned().collect())
            })
            .collect();
        let abort = es.iter().find(|e| e["kind"] == "abort").map(|e| e["detail"]["code"].as_str().unwrap().to_string());
        (tools, abort)
    };
    let mut arx = Arx::new(
        &c.module,
        Box::new(SimWorld::from_json(&world).unwrap()),
        Box::new(Scripted::from_json(&models).unwrap()),
    );
    arx.contracts = athc::contract_hashes(&c);
    let vals = args
        .iter()
        .map(|a| match a {
            J::Bool(b) => Value::Bool(*b),
            J::String(t) => Value::Text(t.clone()),
            other => panic!("argumento {other}"),
        })
        .collect();
    let _ = arx.run("A", "go", vals);
    let a = calls(&arx.journal.to_jsonl());

    let dir = std::env::temp_dir().join(format!("ciclo1-obrig-{}-{nome}-{roteiro}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let py = ath_emit::python(&c.module, &c.check, &c.codex, "o.ath").unwrap();
    let (prog, w, m, j, r) =
        (dir.join("o.py"), dir.join("w.json"), dir.join("m.json"), dir.join("j.jsonl"), dir.join("r.json"));
    std::fs::write(&prog, py).unwrap();
    std::fs::write(&w, world.to_string()).unwrap();
    std::fs::write(&m, models.to_string()).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let st = std::process::Command::new("python3")
        .env("PYTHONPATH", root.join("runtimes/python"))
        .args(["-m", "aletheia_runtime", "run"])
        .arg(&prog)
        .arg("--world")
        .arg(&w)
        .arg("--models")
        .arg(&m)
        .args(["--provider", "scripted", "--agent", "A", "--on", "go"])
        .args(args.iter().flat_map(|a| ["--arg-json".to_string(), a.to_string()]))
        .arg("--journal")
        .arg(&j)
        .arg("--report")
        .arg(&r)
        .output()
        .expect("python3");
    assert!(j.exists(), "{}", String::from_utf8_lossy(&st.stderr));
    let p = calls(&std::fs::read_to_string(&j).unwrap());
    [a, p]
}

fn codigos(nome: &str) -> Vec<String> {
    compile(&src(nome)).err().unwrap_or_default().into_iter().map(|d| d.code).collect()
}

#[test]
fn b1_argumentos_de_elementos_diferentes_nao_descarregam() {
    // antes do reparo: Ok(1) — "levantada para o refine", falso
    let s = sitios("b1_cruzado");
    assert!(matches!(&s[..], [Err(r)] if r.contains("outro elemento")), "{s:?}");
    // desde 28/11 o programa não compila: a chamada mistura dois elementos aprovados (E0453).
    // O comportamento em execução de antes (a 1ª chamada executa; a 2ª, R0630) está registrado
    // em bench/obrigacoes/depois.json.
    assert_eq!(codigos("b1_cruzado"), vec!["E0453"]);
}

#[test]
fn b1_controles_com_origem_comum_continuam_descarregados() {
    for nome in ["c1_mesmo_elemento", "c1_alias", "c1_laco_interno_sem_uso"] {
        assert_eq!(sitios(nome), vec![Ok(1)], "{nome}");
    }
    // a obrigação levantada recusa no `refine` o plano com um elemento que viola: nada chega à ferramenta
    for (calls, abort) in executa("c1_mesmo_elemento", "mundo_pairs", "roteiro_um_invalido") {
        assert!(calls.is_empty());
        assert_eq!(abort, None);
    }
    for (calls, abort) in executa("c1_alias", "mundo_pairs", "roteiro_dois_validos") {
        assert_eq!(calls, vec![json!(["alice", "100"]), json!(["bob", "5"])]);
        assert_eq!(abort, None);
    }
}

#[test]
fn b2_o_nome_de_keeping_as_nao_empresta_a_origem_de_fora() {
    // antes: Ok(1) — levantada para o refine externo (L12), cuja lista nem é a usada
    for nome in ["b2_as_sombreia", "b2_as_recusado_valido"] {
        let s = sitios(nome);
        assert!(matches!(&s[..], [Err(r)] if r.contains("`fora` é o que a política recusou")), "{nome}: {s:?}");
    }
    // mallory foi recusado pela política; o monitor do efeito o recusa antes da ferramenta
    for (calls, abort) in executa("b2_as_sombreia", "mundo_payees", "roteiro_sombra") {
        assert!(calls.is_empty());
        assert_eq!(abort.as_deref(), Some("R0630"));
    }
    // o recusado que satisfaz o contrato chega à ferramenta: o programa escolheu usá-lo, e o
    // `requires` (a única afirmação sobre essa chamada) vale — agora conferido em execução
    for (calls, abort) in executa("b2_as_recusado_valido", "mundo_payees", "roteiro_sombra_recusado_valido") {
        assert_eq!(calls, vec![json!(["alice", "outro"])]);
        assert_eq!(abort, None);
    }
}

#[test]
fn b2_controle_sem_sombra_continua_descarregado() {
    assert_eq!(sitios("c2_sem_sombra"), vec![Ok(1)]);
    for (calls, abort) in executa("c2_sem_sombra", "mundo_payees", "roteiro_sem_sombra") {
        assert_eq!(calls, vec![json!(["alice", "ok"])]);
        assert_eq!(abort, None);
    }
}

// ---- achados da revisão independente do reparo (27/11), corrigidos no mesmo lote

#[test]
fn f1_requires_com_if_ou_match_le_todos_os_parametros() {
    // antes: `descarregada`, sem refine nenhum, com argumentos literais
    let s = sitios("f1_requires_if");
    assert!(matches!(&s[..], [Err(_)]), "{s:?}");
    for (calls, abort) in executa("f1_requires_if", "mundo_payees", "roteiro_vazio") {
        assert!(calls.is_empty());
        assert_eq!(abort.as_deref(), Some("R0630"));
    }
    // parâmetro lido só dentro de `if`: agora entra na obrigação levantada, e os dois runtimes
    // concordam (antes, o ARX abortava com R0100 e o Python passava `None`)
    assert_eq!(sitios("f1_requires_parcial"), vec![Ok(1)]);
    for (calls, abort) in executa("f1_requires_parcial", "mundo_payees", "roteiro_parcial_ok") {
        assert_eq!(calls, vec![json!(["alice", "ok"])]);
        assert_eq!(abort, None);
    }
    for (calls, abort) in executa("f1_requires_parcial", "mundo_payees", "roteiro_parcial_evil") {
        assert!(calls.is_empty(), "recusado no refine, antes de qualquer efeito");
        assert_eq!(abort, None);
    }
}

#[test]
fn f2_um_campo_de_cada_proposta_nao_descarrega() {
    let s = sitios("f2_dois_refines");
    assert!(matches!(&s[..], [Err(_)]), "{s:?}");
    // desde 28/11: um campo de cada proposta é mistura de operações aprovadas (E0453); antes,
    // nenhuma chamada e R0630 nos dois runtimes (bench/obrigacoes/depois.json)
    assert_eq!(codigos("f2_dois_refines"), vec!["E0453"]);
}

#[test]
fn f3_escolha_entre_elementos_do_mesmo_laco_continua_descarregada() {
    // o valor é um elemento ou outro, e os dois campos vêm desse mesmo valor: origem comum comprovável
    for nome in ["c3_if_mesmo_laco", "c3_lista_dois"] {
        assert_eq!(sitios(nome), vec![Ok(1)], "{nome}");
        for (calls, abort) in
            executa_com(nome, "mundo_pairs_sim", "roteiro_rows_um_invalido", &[json!("x"), json!(true)])
        {
            assert!(calls.is_empty(), "{nome}: o elemento que viola faz o refine recusar o plano");
            assert_eq!(abort, None);
        }
    }
}

// ---- achados da segunda revisão (27/11), corrigidos no mesmo lote

#[test]
fn g1_um_parametro_ligado_a_dois_campos_nao_descarrega() {
    // `one(bank, if c { to } else { memo })` e o mesmo com campos de elementos diferentes: antes do
    // ajuste, a ligação guardava só o primeiro campo e a obrigação levantada conferia o campo errado
    for (nome, roteiro) in [("g1_campo_if", "roteiro_um_op"), ("g1_campo_dois_elem", "roteiro_ops_validos")] {
        let s = sitios(nome);
        assert!(matches!(&s[..], [Err(_)]), "{nome}: {s:?}");
        for (calls, abort) in executa_com(nome, "mundo_rev2", roteiro, &[json!("x"), json!(false)]) {
            assert!(calls.is_empty(), "{nome}");
            assert_eq!(abort.as_deref(), Some("R0630"), "{nome}: contrato recusado no efeito, não defeito da análise");
        }
    }
}

#[test]
fn g2_refine_com_else_de_outra_proposta_continua_descarregado() {
    // a 2ª proposta viola e é recusada no `refine`; o `else` devolve a 1ª, que já foi conferida
    assert_eq!(sitios("g2_else_fallback"), vec![Ok(2)]);
    for (calls, abort) in executa_com("g2_else_fallback", "mundo_rev2", "roteiro_fallback", &[json!("x"), json!(true)])
    {
        assert_eq!(calls, vec![json!(["alice", "100"])]);
        assert_eq!(abort, None);
    }
}

#[test]
fn g3_campos_lidos_por_match_de_um_braco_continuam_descarregados() {
    assert_eq!(sitios("g3_match_um_braco"), vec![Ok(1)]);
    for (calls, abort) in
        executa_com("g3_match_um_braco", "mundo_rev2", "roteiro_ops_um_invalido", &[json!("x"), json!(true)])
    {
        assert!(calls.is_empty(), "o elemento que viola faz o refine recusar o plano");
        assert_eq!(abort, None);
    }
}

// ---- achado da terceira revisão (27/11)

#[test]
fn h1_requires_que_nao_le_parametro_e_conferido_no_efeito() {
    // `requires s.member("flags", "payments_open")`: só a fotografia decide, nenhum `refine` confere.
    // Antes: "descarregada" sem refine, e R0631 quando a bandeira está fechada.
    let s = sitios("h1_requires_sem_parametro");
    assert!(matches!(&s[..], [Err(r)] if r.contains("não lê parâmetro")), "{s:?}");
    for (calls, abort) in executa("h1_requires_sem_parametro", "mundo_fechado", "roteiro_um_plano") {
        assert!(calls.is_empty());
        assert_eq!(abort.as_deref(), Some("R0630"));
    }
}
