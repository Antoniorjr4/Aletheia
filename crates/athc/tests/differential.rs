//! Testes diferenciais: o mesmo programa `.ath`, no mesmo mundo e com o mesmo roteiro de
//! modelo, executado pelo ARX (Rust) e pelo código emitido por `athc emit --target py` sobre o
//! `aletheia_runtime`. Os dois backends precisam produzir journals **idênticos byte a byte**
//! (logo, a mesma cabeça de hash), o mesmo resultado ou erro, o mesmo estado final do mundo e o
//! mesmo orçamento gasto. Isso cobre o grafo de eventos (ordem e conteúdo de cada entrada), as
//! decisões de política (`endorse`/`policy_reject`) e os estados de recuperação (sagas,
//! compensações, `TimeoutUnknown`, reconciliação, transferência).

use arx::host::SimWorld;
use arx::model::{Masked, ModelProvider, Scripted};
use arx::{Arx, Value};
use athc::{compile, Compiled};
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn fixture(name: &str) -> Json {
    serde_json::from_str(&read(&format!("examples/fixtures/{name}.json"))).unwrap()
}

fn scratch() -> PathBuf {
    let d = std::env::temp_dir().join(format!("aletheia-diff-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Case<'a> {
    name: String,
    agent: &'a str,
    event: &'a str,
    world: Json,
    models: Json,
    masked: bool,
    args: Vec<Value>,
}

/// O que se compara: journal (bytes) e o relatório canônico (resultado, erro, mundo, orçamento).
#[derive(Debug)]
struct Outcome {
    journal: String,
    report: String,
}

fn report(result: Json, error: Json, world: Json, spent: Json, head: &str) -> String {
    json!({ "result": result, "error": error, "world": world, "spent": spent, "journal_head": head }).to_string()
}

fn run_arx(c: &Compiled, case: &Case) -> Outcome {
    let scripted = Scripted::from_json(&case.models).unwrap();
    let provider: Box<dyn ModelProvider> =
        if case.masked { Box::new(Masked { inner: Box::new(scripted) }) } else { Box::new(scripted) };
    let mut arx = Arx::new(&c.module, Box::new(SimWorld::from_json(&case.world).unwrap()), provider);
    arx.contracts = athc::contract_hashes(c);
    let out = arx.run(case.agent, case.event, case.args.clone());
    let (result, error) = match &out {
        Ok(v) => (v.to_json(), Json::Null),
        Err(e) => (Json::Null, json!({ "code": e.code, "msg": e.msg })),
    };
    let spent = json!({ "tok": arx.meter.tok, "usd": arx.meter.usd, "wall": arx.meter.wall });
    Outcome {
        journal: arx.journal.to_jsonl(),
        report: report(result, error, arx.host.state(), spent, &arx.journal.head),
    }
}

fn emit(program: &str) -> (Compiled, PathBuf) {
    let c = compile(&read(program))
        .unwrap_or_else(|d| panic!("{program}: {:?}", d.iter().map(|x| &x.code).collect::<Vec<_>>()));
    let py = ath_emit::python(&c.module, &c.check, &c.codex, program).expect("emissão");
    let path = scratch().join(format!("{}.py", program.replace(['/', '.'], "_")));
    std::fs::write(&path, py).unwrap();
    (c, path)
}

fn run_py(emitted: &Path, case: &Case) -> Outcome {
    let dir = scratch();
    let tag = case.name.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    let (w, m, j, r) = (
        dir.join(format!("{tag}.world.json")),
        dir.join(format!("{tag}.models.json")),
        dir.join(format!("{tag}.journal.jsonl")),
        dir.join(format!("{tag}.report.json")),
    );
    std::fs::write(&w, case.world.to_string()).unwrap();
    std::fs::write(&m, case.models.to_string()).unwrap();
    let mut cmd = Command::new("python3");
    cmd.env("PYTHONPATH", root().join("runtimes/python"))
        .args(["-m", "aletheia_runtime", "run"])
        .arg(emitted)
        .arg("--world")
        .arg(&w)
        .arg("--models")
        .arg(&m)
        .args(["--provider", if case.masked { "masked" } else { "scripted" }])
        .args(["--agent", case.agent, "--on", case.event])
        .arg("--journal")
        .arg(&j)
        .arg("--report")
        .arg(&r);
    for a in &case.args {
        cmd.arg("--arg-json").arg(a.to_json().to_string());
    }
    let out = cmd.output().expect("python3 é necessário para os testes diferenciais");
    assert!(
        out.status.code().is_some_and(|c| c <= 1) && r.exists(),
        "{}: o runtime Python falhou\n{}",
        case.name,
        String::from_utf8_lossy(&out.stderr)
    );
    let rep: Json = serde_json::from_str(&std::fs::read_to_string(&r).unwrap()).unwrap();
    Outcome {
        journal: std::fs::read_to_string(&j).unwrap_or_default(),
        report: report(
            rep["result"].clone(),
            rep["error"].clone(),
            rep["world"].clone(),
            rep["spent"].clone(),
            rep["journal_head"].as_str().unwrap(),
        ),
    }
}

/// Compara os dois backends; devolve a cabeça do journal (para relatórios).
fn same(c: &Compiled, emitted: &Path, case: Case) -> String {
    let a = run_arx(c, &case);
    let p = run_py(emitted, &case);
    if a.journal != p.journal {
        let (la, lp): (Vec<&str>, Vec<&str>) = (a.journal.lines().collect(), p.journal.lines().collect());
        let i = la.iter().zip(&lp).position(|(x, y)| x != y).unwrap_or(la.len().min(lp.len()));
        panic!(
            "{}: journals diferem na entrada {i} ({} × {} entradas)\nARX:    {}\nPython: {}",
            case.name,
            la.len(),
            lp.len(),
            la.get(i).unwrap_or(&"<fim>"),
            lp.get(i).unwrap_or(&"<fim>")
        );
    }
    assert_eq!(a.report, p.report, "{}: resultado, mundo ou orçamento diferem", case.name);
    let head: Json = serde_json::from_str(&a.report).unwrap();
    head["journal_head"].as_str().unwrap().to_string()
}

fn text(s: &str) -> Vec<Value> {
    vec![Value::Text(s.into())]
}

// ------------------------------------------------------------------ remediate.ath

#[test]
fn remediate_is_isomorphic_across_every_fixture_world() {
    let (c, py) = emit("examples/remediate.ath");
    let models = fixture("models");
    let mut heads = Vec::new();
    for world in
        ["world", "world-saga-compensa", "world-saga-falha", "world-timeout-reconcilia", "world-timeout-transfere"]
    {
        for masked in [true, false] {
            heads.push(same(
                &c,
                &py,
                Case {
                    name: format!("remediate/{world}/{}", if masked { "masked" } else { "scripted" }),
                    agent: "Remediator",
                    event: "alert",
                    world: fixture(world),
                    models: models.clone(),
                    masked,
                    args: text("api"),
                },
            ));
        }
    }
    heads.push(same(
        &c,
        &py,
        Case {
            name: "remediate/restart".into(),
            agent: "Remediator",
            event: "alert",
            world: fixture("world"),
            models: fixture("models-restart"),
            masked: true,
            args: text("api"),
        },
    ));
    heads.sort();
    heads.dedup();
    assert!(heads.len() >= 6, "os mundos exercitam caminhos distintos: {heads:?}");
}

#[test]
fn remediate_error_paths_are_isomorphic() {
    let (c, py) = emit("examples/remediate.ath");
    // ferramenta mais lenta que o contrato Δ: ContractBreach R0701 e `abort` no journal
    let mut slow = fixture("world");
    slow["latency"]["logs"] = json!(5.0);
    same(
        &c,
        &py,
        Case {
            name: "remediate/R0701".into(),
            agent: "Remediator",
            event: "alert",
            world: slow,
            models: fixture("models"),
            masked: true,
            args: text("api"),
        },
    );
    // argumento fora do refinamento do evento: R0003 antes de qualquer entrada
    same(
        &c,
        &py,
        Case {
            name: "remediate/R0003".into(),
            agent: "Remediator",
            event: "alert",
            world: fixture("world"),
            models: fixture("models"),
            masked: true,
            args: text(&"x".repeat(40)),
        },
    );
    // roteiro com texto bruto fora do tipo, inclusive não-JSON
    same(
        &c,
        &py,
        Case {
            name: "remediate/garbage".into(),
            agent: "Remediator",
            event: "alert",
            world: fixture("world"),
            models: json!({ "planner": ["isto não é JSON", "{\"value\": {\"kind\": \"Delete\"}}", ""] }),
            masked: false,
            args: text("api"),
        },
    );
}

// ------------------------------------------------------------------ rollout.ath (listas e laços)

#[test]
fn rollout_lists_and_nested_loops_are_isomorphic() {
    let (c, py) = emit("examples/rollout.ath");
    let plans = fixture("models-rollout");
    let four = Json::Array(vec![json!({ "kind": "Restart", "deploy": "api" }); 4]);
    let cases: Vec<(&str, &str, Json, bool)> = vec![
        ("plano", "world", plans.clone(), false),
        ("plano/masked", "world", plans.clone(), true),
        ("fora dos limites", "world", json!({ "planner": [[], four] }), false),
        ("máscara corta o plano", "world", json!({ "planner": [four] }), true),
        (
            "saga falha no laço",
            "world-saga-falha",
            json!({ "planner": [[{ "kind": "Scale", "deploy": "api", "to": 6 }, { "kind": "Restart", "deploy": "worker" }]] }),
            false,
        ),
        (
            "compensa no laço",
            "world-saga-compensa",
            json!({ "planner": [[{ "kind": "Scale", "deploy": "api", "to": 6 }]] }),
            false,
        ),
        (
            "reconcilia no laço",
            "world-timeout-reconcilia",
            json!({ "planner": [[{ "kind": "Scale", "deploy": "api", "to": 6 }, { "kind": "Scale", "deploy": "worker", "to": 3 }]] }),
            false,
        ),
        (
            "transfere no laço",
            "world-timeout-transfere",
            json!({ "planner": [[{ "kind": "Scale", "deploy": "api", "to": 6 }]] }),
            false,
        ),
    ];
    for (name, world, models, masked) in cases {
        same(
            &c,
            &py,
            Case {
                name: format!("rollout/{name}"),
                agent: "Rollout",
                event: "alert",
                world: fixture(world),
                models,
                masked,
                args: text("api"),
            },
        );
    }
}

// ------------------------------------------------------------------ M1 (cenários no estilo AgentDojo)

#[test]
fn m1_scenarios_are_isomorphic() {
    let (c, py) = emit("bench/m1/aletheia/agent.ath");
    // os mundos vêm do próprio harness da M1 (world_for), para os dois backends
    let out = Command::new("python3")
        .current_dir(root().join("bench/m1"))
        .args([
            "-c",
            "import json, pathlib, run\n\
             print(json.dumps([{'id': s['id'], 'alert': s['alert'], 'model': s['model'], 'world': run.world_for(s)}\n\
               for s in (json.loads(p.read_text()) for p in sorted(pathlib.Path('scenarios').glob('*.json')))]))",
        ])
        .output()
        .expect("python3");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let scenarios: Vec<Json> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(scenarios.len(), 7);
    for sc in scenarios {
        same(
            &c,
            &py,
            Case {
                name: format!("m1/{}", sc["id"].as_str().unwrap()),
                agent: "Remediator",
                event: "alert",
                world: sc["world"].clone(),
                models: json!({ "planner": sc["model"] }),
                masked: true,
                args: text(sc["alert"].as_str().unwrap()),
            },
        );
    }
}
