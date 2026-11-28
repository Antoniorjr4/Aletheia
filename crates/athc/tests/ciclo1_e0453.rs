//! **E0453: mistura de operações aprovadas (28/11/2026).**
//!
//! A E0453 recusa, na compilação, a chamada de ferramenta cujos argumentos vindos de valores
//! endossados dependem de duas ou mais operações aprovadas distintas: dois elementos de uma lista
//! endossada, um elemento e um campo do plano, um elemento e a lista inteira, ou valores de dois
//! `refine`. Ela não prova que os argumentos sejam os aprovados (os campos de uma só operação ainda
//! podem ser transformados) e não cobre a agregação sobre uma lista inteira sozinha.
//!
//! As sondas ficam em `bench/agentdojo/x2/e0453/sondas` (`gen.py`): `m*` misturam e são recusadas;
//! `u*` usam uma operação só e são aceitas; `m10` só agrega e é aceita, fora da garantia. Os
//! controles aceitos rodam nos dois runtimes, com as mesmas chamadas.

use arx::host::SimWorld;
use arx::model::Scripted;
use arx::{Arx, Value};
use athc::compile;
use serde_json::{json, Value as J};
use std::path::PathBuf;

fn sondas() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/agentdojo/x2/e0453/sondas")
}

fn src(nome: &str) -> String {
    std::fs::read_to_string(sondas().join(format!("{nome}.ath"))).unwrap()
}

fn codigos(nome: &str) -> Vec<String> {
    compile(&src(nome)).err().unwrap_or_default().into_iter().map(|d| d.code).collect()
}

const MISTURAS: [&str; 26] = [
    "m1_dois_lacos",
    "m2_variaveis",
    "m3_reconstrucao",
    "m4_transformacao",
    "m5_juncao",
    "m6_elemento_e_lista",
    "m7_elemento_e_raiz",
    "m8_duas_listas",
    "m9_quantificador",
    "m11_lista_literal",
    "m12_campo_por_ponto",
    "m13_dois_refines_escalar",
    "m14_if_campo_de_dois",
    "m15_else_elemento_e_outro",
    "m16_else_aninhado_elemento_e_lista",
    // fugas da 1ª versão achadas pela revisão independente de 28/11: aceitas em 4185d7f
    "m17_if_ramo_ja_misturado",
    "m18_if_ramo_escalar_misturado",
    "m19_if_sem_outro_ramo_igual",
    "m20_if_em_if",
    "m21_for_lista_literal_reconstruida",
    "m22_for_lista_literal_escalar",
    "m23_refine_else_misturado",
    "m24_atomic",
    "m25_atomic_on_failure",
    "m26_ponto_duas_listas_agregadas",
    "m27_reconstrucao_com_lista",
];

/// Fora da garantia, declarados: dependência de controle. Aceitos.
const LIMITES: [&str; 2] = ["l1_valor_por_controle", "l2_condicao_da_chamada"];

/// Recusas conservadoras de programas legítimos, declaradas: duas junções com a mesma condição
/// tratadas como independentes; um elemento com a contagem da lista; `for x in [a]`.
const CONSERVADORAS: [&str; 3] = ["c1_dois_ifs_mesma_condicao", "c2_elemento_e_contagem", "c3_lista_literal_de_um"];

const UMA: [&str; 13] = [
    "u1_mesmo_elemento",
    "u2_literal_e_confiavel",
    "u3_transformada",
    "u4_alias_e_match_duplo",
    "u5_if_mesmo",
    "u6_lista_literal_um",
    "u7_ponto_mesmo",
    "u8_else_aninhado_agregado",
    "u9_else_elemento",
    "u10_if_transformado_mesmo",
    "u11_if_dois_campos_mesmo",
    "u12_atomic_mesmo",
    "u13_reconstrucao_com_lista",
];

#[test]
fn as_sondas_geradas_estao_versionadas() {
    // o gerador e os arquivos não divergem
    let dir = std::env::temp_dir().join(format!("e0453-gen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let st = std::process::Command::new("python3").arg(sondas().join("gen.py")).current_dir(&dir).status().unwrap();
    assert!(st.success());
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        let nome = p.file_name().unwrap().to_str().unwrap().to_string();
        let versionado = std::fs::read_to_string(sondas().join(&nome)).unwrap_or_default();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), versionado, "{nome}");
    }
}

#[test]
fn cada_mistura_e_recusada_com_a_e0453_e_so_com_ela() {
    for m in MISTURAS.iter().chain(&CONSERVADORAS) {
        assert_eq!(codigos(m), vec!["E0453"], "{m}");
    }
}

#[test]
fn o_diagnostico_aponta_as_origens_e_o_caminho_autorizado() {
    let d = compile(&src("m1_dois_lacos")).err().unwrap();
    let d = &d[0];
    assert!(d.msg.contains("combina dados de 2 origens aprovadas distintas"), "{}", d.msg);
    // as duas origens: os dois laços que escolheram os elementos
    let origens: Vec<_> = d.secondary.iter().filter(|(_, l)| l.contains("origem")).collect();
    assert_eq!(origens.len(), 2, "{:?}", d.secondary);
    let texto = d.notes.join("\n") + "\n" + d.help.as_deref().unwrap_or("");
    assert!(texto.contains("não prova que os argumentos sejam os aprovados"), "{texto}");
    // o caminho autorizado existe na linguagem: uma variante do plano decidida no `refine`
    assert!(texto.contains("variante do plano") && texto.contains("`refine`"), "{texto}");
    assert!(!texto.contains("authorize"), "{texto}");
}

#[test]
fn uma_operacao_literais_e_dados_confiaveis_e_a_agregacao_pura_sao_aceitos() {
    for u in UMA.iter().chain(&LIMITES).chain(&["m10_so_agregados"]) {
        assert!(codigos(u).is_empty(), "{u}: {:?}", codigos(u));
    }
}

/// As chamadas de `pay` (sem a capacidade) no ARX e no Python, e o fim de cada um.
fn executa(nome: &str) -> [(Vec<J>, Option<String>); 2] {
    let c = compile(&src(nome)).unwrap();
    let plano = json!({ "kind": "Plan", "note": "nota",
        "ops": [{ "kind": "Pay", "to": "A", "memo": "a", "n": 1 }, { "kind": "Pay", "to": "B", "memo": "b", "n": 5 }],
        "extra": [{ "kind": "Ex", "memo": "x" }] });
    let world = json!({ "sinks": ["pay"] });
    // `u9` pede uma operação avulsa por iteração depois do plano
    let op = |to: &str, memo: &str| json!({ "kind": "Pay", "to": to, "memo": memo, "n": 0 });
    let models = if nome.starts_with("u9") {
        json!({ "planner": [plano, op("X", "x"), op("Y", "y")] })
    } else {
        json!({ "planner": [plano.clone(), plano.clone(), plano] })
    };
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
    let _ = arx.run("A", "go", vec![Value::Text("pague".into()), Value::Bool(true)]);
    let a = calls(&arx.journal.to_jsonl());

    let dir = std::env::temp_dir().join(format!("ciclo1-e0453-{}-{nome}", std::process::id()));
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
        .args(["--arg-json", "\"pague\"", "--arg-json", "true"])
        .arg("--journal")
        .arg(&j)
        .arg("--report")
        .arg(&r)
        .output()
        .expect("python3");
    assert!(j.exists(), "{}", String::from_utf8_lossy(&st.stderr));
    [a, calls(&std::fs::read_to_string(&j).unwrap())]
}

#[test]
fn os_controles_aceitos_executam_igual_nos_dois_runtimes() {
    let esperado: [(&str, J); 16] = [
        ("u1_mesmo_elemento", json!([["A", "a"], ["B", "b"]])),
        ("u2_literal_e_confiavel", json!([["A", "pedido pague"], ["B", "pedido pague"]])),
        // transformada: os campos são de uma só operação, mas não são os aprovados
        ("u3_transformada", json!([["A", "a!"], ["B", "b!"]])),
        ("u4_alias_e_match_duplo", json!([["A", "a"], ["B", "b"]])),
        ("u5_if_mesmo", json!([["A", "a"], ["B", "b"]])),
        // cada item da lista literal `[a, b]` é um elemento inteiro: (a,b) ∈ {AA, AB, BA, BB}, 8 chamadas
        (
            "u6_lista_literal_um",
            json!([["A", "a"], ["A", "a"], ["A", "a"], ["B", "b"], ["B", "b"], ["A", "a"], ["B", "b"], ["B", "b"]]),
        ),
        ("u7_ponto_mesmo", json!([["A", "a"], ["B", "b"]])),
        ("u8_else_aninhado_agregado", json!([["r", "a,b2"]])),
        ("u9_else_elemento", json!([["X", "x"], ["Y", "y"]])),
        ("u10_if_transformado_mesmo", json!([["A", "a"], ["B", "b"]])),
        ("u11_if_dois_campos_mesmo", json!([["A", "a"], ["B", "b"]])),
        ("u12_atomic_mesmo", json!([["A", "a"], ["B", "b"]])),
        ("u13_reconstrucao_com_lista", json!([["A", "a"], ["B", "b"]])),
        // fora da garantia: o `if` sobre `b` decide o memo e a ocorrência da chamada
        ("l1_valor_por_controle", json!([["A", "a"], ["A", "a"], ["B", "b"], ["B", "b"]])),
        ("l2_condicao_da_chamada", json!([["A", "a"], ["B", "b"]])),
        // agregação pura: aceita, e a chamada depende das duas operações (fora da garantia)
        ("m10_so_agregados", json!([["AB", "ab"]])),
    ];
    for (nome, calls) in esperado {
        let [a, p] = executa(nome);
        assert_eq!(a, p, "{nome}: ARX × Python");
        assert_eq!(a.1, None, "{nome}: {a:?}");
        assert_eq!(J::Array(a.0.clone()), calls, "{nome}");
    }
}

/// As sondas das duas revisões independentes de 28/11 (`revisao1`, `revisao2`), com o veredito
/// de cada uma depois das correções: as fugas achadas são E0453; os limites declarados
/// (dependência de controle, fronteira de ferramenta), a agregação pura e os controles legítimos
/// são aceitos; as recusas conservadoras continuam E0453.
#[test]
fn as_sondas_das_revisoes_tem_o_veredito_registrado() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/agentdojo/x2/e0453");
    let esperado: std::collections::BTreeMap<String, Vec<String>> =
        serde_json::from_str(&std::fs::read_to_string(base.join("revisoes_esperado.json")).unwrap()).unwrap();
    assert!(esperado.len() > 80);
    for (f, codes) in esperado {
        let got: Vec<String> = compile(&std::fs::read_to_string(base.join(&f)).unwrap())
            .err()
            .unwrap_or_default()
            .into_iter()
            .map(|d| d.code)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(got, codes, "{f}");
    }
}
