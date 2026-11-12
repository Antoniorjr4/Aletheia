//! Testes diferenciais: o mesmo programa `.ath`, no mesmo mundo e com o mesmo roteiro de
//! modelo, executado pelo ARX (Rust) e pelo código emitido por `athc emit --target py` sobre o
//! `aletheia_runtime`. Os dois backends precisam produzir journals **idênticos byte a byte**
//! (logo, a mesma cabeça de hash), o mesmo resultado ou erro, o mesmo estado final do mundo e o
//! mesmo orçamento gasto. Isso cobre o grafo de eventos (ordem e conteúdo de cada entrada), as
//! decisões de política (`endorse`/`policy_reject`) e os estados de recuperação (sagas,
//! compensações, `TimeoutUnknown`, reconciliação, transferência).
//!
//! No fim do arquivo está o **tripé**: o avaliador formal do Lean (`ath-lean-eval`, o mesmo
//! `eval` dos teoremas T1–T3 e T6) entra como terceiro backend. Para programas do núcleo λ_Ath,
//! com e sem sagas, as três realizações precisam produzir o mesmo journal do núcleo, byte a byte.

use arx::host::SimWorld;
use arx::model::{Masked, ModelProvider, Scripted};
use arx::{Arx, Value};
use athc::{compile, Compiled};
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};
use std::process::Command;

mod lambda_ath;

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

// ------------------------------------------------------------------ replay de tráfego HTTP real

use arx::cassette::{first_divergence, Cassette};
use arx::model::{Constraint, Exchange, OpenAiCompat, Recorder, Replay};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::rc::Rc;

/// Servidor HTTP mínimo: responde em ordem a cada conexão com (status, cabeçalhos, corpo).
fn mock(replies: Vec<(u16, &'static str, String)>) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    let h = std::thread::spawn(move || {
        for (status, headers, reply) in replies {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 8192];
            let head_end = loop {
                let n = sock.read(&mut tmp).unwrap();
                buf.extend_from_slice(&tmp[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let head = String::from_utf8_lossy(&buf[..head_end]).to_ascii_lowercase();
            if let Some(len) = head.lines().find_map(|l| l.strip_prefix("content-length:")) {
                let len: usize = len.trim().parse().unwrap();
                while buf.len() < head_end + len {
                    let n = sock.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                }
            } else {
                while !buf.ends_with(b"0\r\n\r\n") {
                    let n = sock.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                }
            }
            let resp = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            sock.write_all(resp.as_bytes()).unwrap();
        }
    });
    (url, h)
}

const PLAN_BAD: &str = r#"{"value":[{"kind":"Scale","deploy":"payments","to":0}]}"#;
const PLAN_OK: &str = r#"{"value":[{"kind":"Restart","deploy":"api"},{"kind":"Scale","deploy":"worker","to":4}]}"#;

/// Tráfego no formato da Groq: 429 com `retry-after`, 400 `json_validate_failed` com a geração
/// fora do esquema e 200 com `usage`, raciocínio e o modelo servido.
fn groq_like() -> Vec<(u16, &'static str, String)> {
    vec![
        (
            429,
            "retry-after: 0\r\n",
            json!({ "error": { "message": "Rate limit reached ... Please try again in 0.2s.", "code": "rate_limit_exceeded" } })
                .to_string(),
        ),
        (
            400,
            "x-request-id: req_mock_400\r\n",
            json!({ "error": { "message": "Generated JSON does not match the expected schema.", "code": "json_validate_failed", "failed_generation": PLAN_BAD } })
                .to_string(),
        ),
        (
            200,
            "",
            json!({
                "id": "chatcmpl-mock-1",
                "model": "openai/gpt-oss-20b",
                "choices": [{ "message": { "role": "assistant", "content": PLAN_OK, "reasoning": "reiniciar a api e escalar o worker" }, "finish_reason": "stop" }],
                "usage": { "prompt_tokens": 701, "completion_tokens": 73, "completion_tokens_details": { "reasoning_tokens": 41 } },
            })
            .to_string(),
        ),
    ]
}

/// Grava uma execução do ARX contra o servidor, como faz `athc run --record`.
fn record(program: &str, agent: &str, replies: Vec<(u16, &'static str, String)>) -> Cassette {
    let src = read(program);
    let c = compile(&src).unwrap();
    let (url, server) = mock(replies);
    let mut p = OpenAiCompat::new(&url, "openai/gpt-oss-20b", Some("chave-de-teste".into()), Constraint::JsonSchema);
    let tape = Rc::new(RefCell::new(Vec::new()));
    let inner = std::mem::replace(&mut p.transport, Box::new(Replay::new(Vec::new())));
    p.transport = Box::new(Recorder { inner, tape: tape.clone() });
    let (provider, config) = (p.name(), p.config_json());
    let world = fixture("world");
    let mut arx = Arx::new(&c.module, Box::new(SimWorld::from_json(&world).unwrap()), Box::new(p));
    arx.contracts = athc::contract_hashes(&c);
    let _ = arx.run(agent, "alert", text("api"));
    server.join().unwrap();
    let exchanges = tape.borrow().clone();
    Cassette {
        provider,
        config,
        manifest: json!({ "source_path": program, "program": src, "agent": agent, "event": "alert", "args": ["api"], "world": world }),
        exchanges,
        journal: arx.journal.to_jsonl(),
    }
}

/// `athc replay` em processo: o programa embutido contra as respostas gravadas.
fn replay_arx(cas: &Cassette) -> (String, Json) {
    let c = compile(cas.manifest["program"].as_str().unwrap()).unwrap();
    let p = OpenAiCompat::from_config(&cas.config, Box::new(Replay::new(cas.exchanges.clone()))).unwrap();
    let mut arx = Arx::new(&c.module, Box::new(SimWorld::from_json(&cas.manifest["world"]).unwrap()), Box::new(p));
    arx.contracts = athc::contract_hashes(&c);
    let agent = cas.manifest["agent"].as_str().unwrap();
    let event = cas.manifest["event"].as_str().unwrap();
    let (_, h) = arx.handler(agent, event).unwrap();
    let args: Vec<Value> = h
        .params
        .iter()
        .zip(cas.manifest["args"].as_array().unwrap())
        .map(|(p, j)| arx.value_from_json(j, &p.ty).unwrap())
        .collect();
    let out = arx.run(agent, event, args);
    let outcome = match out {
        Ok(v) => json!({ "result": v.to_json() }),
        Err(e) => json!({ "error": e.code }),
    };
    (arx.journal.to_jsonl(), outcome)
}

/// `python -m aletheia_runtime replay` sobre o código emitido do programa embutido.
fn replay_py(cas: &Cassette, tag: &str) -> String {
    let dir = scratch();
    let src = cas.manifest["program"].as_str().unwrap();
    let c = compile(src).unwrap();
    let py = ath_emit::python(&c.module, &c.check, &c.codex, cas.manifest["source_path"].as_str().unwrap()).unwrap();
    let (prog, cfile, jfile) = (
        dir.join(format!("{tag}.py")),
        dir.join(format!("{tag}.cassette.json")),
        dir.join(format!("{tag}.replay.jsonl")),
    );
    std::fs::write(&prog, py).unwrap();
    cas.save(&cfile).unwrap();
    let out = Command::new("python3")
        .env("PYTHONPATH", root().join("runtimes/python"))
        .args(["-m", "aletheia_runtime", "replay"])
        .arg(&cfile)
        .arg(&prog)
        .arg("--journal")
        .arg(&jfile)
        .output()
        .expect("python3");
    assert!(jfile.exists(), "{tag}: {}", String::from_utf8_lossy(&out.stderr));
    std::fs::read_to_string(&jfile).unwrap()
}

fn assert_same(tag: &str, a: &str, b: &str) {
    if let Some((i, x, y)) = first_divergence(a, b) {
        panic!("{tag}: journals diferem na entrada {i}\nARX:    {x}\nPython: {y}");
    }
}

#[test]
fn replay_of_recorded_http_traffic_is_isomorphic() {
    let cas = record("examples/rollout.ath", "Rollout", groq_like());
    assert_eq!(cas.exchanges.len(), 3, "429, 400 json_validate_failed e 200");
    assert!(
        !serde_json::to_string(&cas.to_json()).unwrap().contains("chave-de-teste"),
        "a chave nunca entra no cassete"
    );
    let (arx_journal, outcome) = replay_arx(&cas);
    assert_eq!(outcome, json!({ "result": "concluído" }));
    assert_same("gravação × replay ARX", &cas.journal, &arx_journal);
    assert_same("gravação × replay Python", &cas.journal, &replay_py(&cas, "replay-rollout"));
    // a espera do 429 e as novas tentativas entram no journal pelos dois lados
    assert!(cas.journal.contains(r#""queued_s":0.5"#) && cas.journal.contains(r#""retries":1"#), "{}", cas.journal);
}

#[test]
fn replay_variants_force_every_contract_and_http_path_identically() {
    let base = record("examples/rollout.ath", "Rollout", groq_like());
    let mutate = |f: &dyn Fn(&mut Vec<Exchange>)| {
        let mut c = base.clone();
        f(&mut c.exchanges);
        c
    };
    let body = |ex: &mut Exchange, edit: &dyn Fn(&mut Json)| {
        let mut j: Json = serde_json::from_str(&ex.reply.body).unwrap();
        edit(&mut j);
        ex.reply.body = j.to_string();
    };
    let cases: Vec<(&str, Cassette, Json)> = vec![
        ("R0703 latência", mutate(&|x| x[2].reply.latency = 9.9), json!({ "error": "R0703" })),
        (
            "R0704 modelo servido",
            mutate(&|x| body(&mut x[2], &|j| j["model"] = json!("openai/gpt-oss-120b"))),
            json!({ "error": "R0704" }),
        ),
        (
            "R0705 entrada",
            mutate(&|x| body(&mut x[2], &|j| j["usage"]["prompt_tokens"] = json!(99_999))),
            json!({ "error": "R0705" }),
        ),
        (
            "R0702 saída",
            mutate(&|x| body(&mut x[2], &|j| j["usage"]["completion_tokens"] = json!(5_000))),
            json!({ "error": "R0702" }),
        ),
        (
            "HTTP 401",
            mutate(&|x| {
                x[0].reply.status = 401;
                x[0].reply.body = r#"{"error":{"message":"Invalid API Key","code":"invalid_api_key"}}"#.into();
            }),
            json!({ "error": "R0621" }),
        ),
        (
            "espera em ms no corpo",
            mutate(&|x| {
                x[0].reply.retry_after = None;
                x[0].reply.body = r#"{"error":{"message":"Please try again in 250ms."}}"#.into();
            }),
            json!({ "result": "concluído" }),
        ),
    ];
    for (tag, cas, expected) in cases {
        let (arx_journal, outcome) = replay_arx(&cas);
        assert_eq!(outcome, expected, "{tag}");
        assert_same(tag, &arx_journal, &replay_py(&cas, &tag.replace(' ', "_")));
    }
}

/// Cassetes gravados contra a Groq pelo workflow de fogo real (`bench/replay/`): os dois
/// backends reproduzem exatamente o journal da execução real.
#[test]
fn replay_of_real_groq_traffic_reproduces_the_recorded_journal() {
    // Cassetes gravados na Groq pelo workflow `replay.yml` e versionados em bench/replay/.
    // Para cada um: journal BLAKE3 idêntico, byte a byte, entre a execução real, o replay no
    // ARX e o replay no Python emitido; e, extraídos de cada journal, a mesma árvore de
    // compensação e o mesmo balanço de orçamento. O conjunto precisa cobrir as duas
    // gravações (ARX e Python) e todos os caminhos de saga sob tráfego real.
    let dir = root().join("bench/replay");
    let mut cassettes: Vec<PathBuf> = Vec::new();
    for d in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        if d.path().is_dir() {
            for f in std::fs::read_dir(d.path()).unwrap().flatten() {
                if f.path().to_string_lossy().ends_with(".cassette.json") {
                    cassettes.push(f.path());
                }
            }
        }
    }
    cassettes.sort();
    let mut kinds = std::collections::BTreeSet::new();
    let (mut by_arx, mut by_py) = (0, 0);
    for path in &cassettes {
        let tag = path.file_name().unwrap().to_string_lossy().replace(".cassette.json", "");
        let cas = Cassette::load(path).unwrap();
        assert!(cas.provider.starts_with("openai-compat("), "{tag}: provedor {}", cas.provider);
        assert!(!cas.exchanges.is_empty(), "{tag}: nenhuma troca HTTP gravada");
        if tag.ends_with(".py") {
            by_py += 1
        } else {
            by_arx += 1
        }
        let (arx_journal, _) = replay_arx(&cas);
        let py_journal = replay_py(&cas, &format!("real-{tag}"));
        assert_same(&format!("{tag}: gravação × replay ARX"), &cas.journal, &arx_journal);
        assert_same(&format!("{tag}: gravação × replay Python"), &cas.journal, &py_journal);
        let recorded = saga_tree(&cas.journal);
        assert_eq!(recorded, saga_tree(&arx_journal), "{tag}: árvore de compensação (ARX)");
        assert_eq!(recorded, saga_tree(&py_journal), "{tag}: árvore de compensação (Python)");
        let spent = balance(&cas.journal);
        assert_eq!(spent, balance(&arx_journal), "{tag}: balanço de orçamento (ARX)");
        assert_eq!(spent, balance(&py_journal), "{tag}: balanço de orçamento (Python)");
        for line in cas.journal.lines() {
            let e: Json = serde_json::from_str(line).unwrap();
            kinds.insert(e["kind"].as_str().unwrap().to_string());
        }
    }
    assert!(by_arx > 0 && by_py > 0, "faltam gravações reais: {by_arx} do ARX, {by_py} do Python");
    // sagas sob carga real, mais a injeção recusada no decodificador e o esgotamento do refine
    for k in [
        "saga_commit",
        "saga_abort",
        "compensate",
        "saga_recovered",
        "compensation_failed",
        "reconcile",
        "decode_reject",
        "refine_exhausted",
    ] {
        assert!(kinds.contains(k), "nenhum cassete real cobre `{k}`: {kinds:?}");
    }
}

/// A árvore de compensação de um journal: cada evento de saga (abertura implícita no
/// primeiro passo, espera, aborto, compensação, recuperação, falha de compensação, commit),
/// com a ferramenta e o resultado, na ordem causal do journal.
fn saga_tree(journal: &str) -> Vec<Json> {
    const SAGA: [&str; 9] = [
        "await_ok",
        "await_timeout",
        "saga_commit",
        "saga_abort",
        "compensate",
        "saga_recovered",
        "compensation_failed",
        "timeout_unknown",
        "reconcile",
    ];
    journal
        .lines()
        .map(|l| serde_json::from_str::<Json>(l).unwrap())
        .filter(|e| SAGA.contains(&e["kind"].as_str().unwrap()))
        .map(|e| json!({ "seq": e["seq"], "kind": e["kind"], "detail": e["detail"] }))
        .collect()
}

/// O balanço de orçamento: o gasto final por dimensão, mais o gasto de cada inferência.
fn balance(journal: &str) -> Json {
    let entries: Vec<Json> = journal.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let infer: Vec<Json> = entries
        .iter()
        .filter(|e| e["kind"] == "infer")
        .map(|e| json!([e["detail"]["tok_in"], e["detail"]["tok_out"], e["detail"]["usd"]]))
        .collect();
    json!({ "end": entries.last().unwrap()["detail"]["spent"], "infer": infer })
}

/// Rev7: o inventário entra no payload do modelo como `Trusted`, idêntico nos dois backends,
/// e a fotografia usada fica no journal BLAKE3.
#[test]
fn trusted_snapshot_is_labelled_in_the_payload_and_journaled_identically() {
    // Uma escala justificada pela telemetria da fotografia (CPU 95%, 3 → 6 réplicas): a política
    // contextual decide pela mesma fotografia que o prompt recebeu.
    let scale = r#"{"value":{"kind":"Scale","deploy":"api","to":6}}"#;
    let replies = vec![(
        200,
        "",
        json!({
            "id": "chatcmpl-mock-rev7",
            "model": "openai/gpt-oss-20b",
            "choices": [{ "message": { "role": "assistant", "content": scale, "reasoning": "CPU 95% na api" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 690, "completion_tokens": 40, "completion_tokens_details": { "reasoning_tokens": 12 } },
        })
        .to_string(),
    )];
    let cas = record("examples/remediate.ath", "Remediator", replies);
    assert_eq!(cas.exchanges.len(), 1);
    let req: Json = serde_json::from_str(&cas.exchanges[0].request).unwrap();
    let user = req["messages"][1]["content"].as_str().unwrap();
    for field in ["estado", "metricas"] {
        let header = format!("### {field} ({})\n", arx::TRUSTED_HEADER);
        assert!(user.contains(&header), "campo `{field}` sem rótulo Trusted:\n{user}");
    }
    assert!(user.contains(&format!("({})\napi, worker\n", arx::TRUSTED_HEADER)), "{user}");
    let snapshot: Vec<Json> = cas
        .journal
        .lines()
        .map(|l| serde_json::from_str::<Json>(l).unwrap())
        .filter(|e| e["kind"] == "snapshot")
        .collect();
    assert_eq!(snapshot.len(), 1, "a fotografia entra no journal exatamente uma vez");
    assert_eq!(snapshot[0]["detail"]["managed"], json!(["api", "worker"]));
    let endorsed = cas.journal.lines().any(|l| l.contains(r#""kind":"endorse""#) && l.contains(r#""to":6"#));
    assert!(endorsed, "a escala justificada precisa ser endossada:\n{}", cas.journal);
    let (arx_journal, _) = replay_arx(&cas);
    let py_journal = replay_py(&cas, "trusted-snapshot");
    assert_same("Trusted: gravação × replay ARX", &cas.journal, &arx_journal);
    assert_same("Trusted: gravação × replay Python", &cas.journal, &py_journal);
}

// ------------------------------------------------------------------ o tripé: Lean × ARX × Python

/// O binário do avaliador formal: `ATH_LEAN_EVAL` ou `lean/.lake/build/bin/ath-lean-eval`
/// (construído por `lake build` em `lean/`). Sem ele, os testes do tripé conferem só ARX ×
/// Python e avisam; com `ATH_REQUIRE_LEAN=1` (o job `tripé` da CI), a ausência é falha.
fn lean_eval() -> Option<PathBuf> {
    let p = std::env::var_os("ATH_LEAN_EVAL")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("lean/.lake/build/bin/ath-lean-eval"));
    if p.is_file() {
        return Some(p);
    }
    assert!(
        std::env::var_os("ATH_REQUIRE_LEAN").is_none(),
        "ATH_REQUIRE_LEAN: o avaliador do Lean não está em {}",
        p.display()
    );
    eprintln!("aviso: {} ausente; o tripé roda só ARX × Python (construa com `lake build` em lean/)", p.display());
    None
}

fn first_diff(tag: &str, what: &str, a: &str, b: &str) {
    if a == b {
        return;
    }
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let i = la.iter().zip(&lb).position(|(x, y)| x != y).unwrap_or(la.len().min(lb.len()));
    panic!(
        "{tag}: {what} diferem na entrada {i} ({} × {} entradas)\n  {}\n  {}",
        la.len(),
        lb.len(),
        la.get(i).unwrap_or(&"<fim>"),
        lb.get(i).unwrap_or(&"<fim>")
    );
}

/// O BLAKE3 escrito em Lean (`lean/Aletheia/Blake3.lean`) contra o crate de referência, nas
/// fronteiras de bloco (64 bytes), de chunk (1024 bytes) e da árvore (2ⁿ chunks).
#[test]
fn lean_blake3_matches_the_reference_crate() {
    let Some(lean) = lean_eval() else { return };
    let dir = scratch();
    for n in [0usize, 1, 63, 64, 65, 1023, 1024, 1025, 2047, 2048, 2049, 3072, 4097, 8192, 16_385, 65_536, 100_003] {
        let data: Vec<u8> = (0..n).map(|i| (i * 31 % 251) as u8).collect();
        let f = dir.join(format!("b3-{n}.bin"));
        std::fs::write(&f, &data).unwrap();
        let out = Command::new(&lean).arg("hash").arg(&f).output().unwrap();
        assert!(out.status.success(), "ath-lean-eval hash: {}", String::from_utf8_lossy(&out.stderr));
        let got = String::from_utf8(out.stdout).unwrap();
        assert_eq!(got.trim(), blake3::hash(&data).to_hex().as_str(), "BLAKE3 de {n} bytes");
    }
}

struct Tripod {
    /// a vista do núcleo do journal de cada backend
    lean: Option<String>,
    /// o modelo do ARX em Lean (`Arx.run`, sobre o qual `Arx.sim` está provado), na mesma vista
    lean_arx: Option<String>,
    arx: String,
    py: String,
    /// o journal completo do ARX (para contar os desfechos das sagas)
    full: String,
    /// o erro com que o ARX e o Python terminaram: só a recusa de uma operação com chave
    /// (`R0600` na chamada, `R0611` na reconciliação) é um desfecho admitido
    arx_err: Option<String>,
    py_err: Option<String>,
}

/// Executa um programa do núcleo nos três backends. `src` é a tradução de superfície, `core`
/// o mesmo programa no formato do `ath-lean-eval`.
fn tripod(tag: &str, src: &str, core: &Json, world: &Json, models: &Json, args: Vec<Value>) -> Tripod {
    let c = compile(src).unwrap_or_else(|d| {
        panic!("{tag}: o athc recusou a tradução\n{src}\n{:?}", d.iter().map(|x| (&x.code, &x.msg)).collect::<Vec<_>>())
    });
    // ARX
    let provider = Box::new(Scripted::from_json(models).unwrap());
    let mut arx = Arx::new(&c.module, Box::new(SimWorld::from_json(world).unwrap()), provider);
    arx.contracts = athc::contract_hashes(&c);
    let arx_err = match arx.run("A", "go", args.clone()) {
        Ok(_) => None,
        Err(e) if e.code == "R0600" || e.code == "R0611" => Some(e.code.clone()),
        Err(e) => panic!("{tag}: o ARX abortou ({} {})\n{src}", e.code, e.msg),
    };
    let arx_core =
        arx::journal::Journal::core_view_with(&arx.journal.entries, &athc::compensable_tools(&c.module)).to_jsonl();
    let arx_full = arx.journal.to_jsonl();
    // Python, sobre o código emitido
    let dir = scratch();
    let file = tag.replace(|ch: char| !ch.is_ascii_alphanumeric(), "_");
    let py = ath_emit::python(&c.module, &c.check, &c.codex, &format!("{file}.ath")).expect("emissão");
    let (pp, w, m, j, cj, r) = (
        dir.join(format!("{file}.py")),
        dir.join(format!("{file}.world.json")),
        dir.join(format!("{file}.models.json")),
        dir.join(format!("{file}.journal.jsonl")),
        dir.join(format!("{file}.core.jsonl")),
        dir.join(format!("{file}.report.json")),
    );
    std::fs::write(&pp, py).unwrap();
    std::fs::write(dir.join(format!("{file}.ath")), src).unwrap();
    std::fs::write(&w, world.to_string()).unwrap();
    std::fs::write(&m, models.to_string()).unwrap();
    let mut cmd = Command::new("python3");
    cmd.env("PYTHONPATH", root().join("runtimes/python"))
        .args(["-m", "aletheia_runtime", "run"])
        .arg(&pp)
        .args(["--world"])
        .arg(&w)
        .args(["--models"])
        .arg(&m)
        .args(["--provider", "scripted", "--agent", "A", "--on", "go", "--journal"])
        .arg(&j)
        .arg("--core-journal")
        .arg(&cj)
        .arg("--report")
        .arg(&r);
    for a in &args {
        cmd.arg("--arg-json").arg(a.to_json().to_string());
    }
    let out = cmd.output().expect("python3 é necessário para o tripé");
    let report: Json =
        std::fs::read_to_string(&r).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Json::Null);
    let py_err = report["error"]["code"].as_str().map(String::from);
    assert!(
        out.status.success() || matches!(py_err.as_deref(), Some("R0600" | "R0611")),
        "{tag}: o runtime Python falhou\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let py_core = std::fs::read_to_string(&cj).unwrap();
    // Lean: o avaliador canônico
    let run_lean = |mode: &str| {
        lean_eval().map(|bin| {
            let pj = dir.join(format!("{file}.core.json"));
            std::fs::write(&pj, core.to_string()).unwrap();
            let out = Command::new(bin).arg(mode).arg(&pj).output().unwrap();
            assert!(
                out.status.success(),
                "{tag}: o avaliador do Lean ({mode}) falhou\n{}\n{src}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        })
    };
    let lean = run_lean("run");
    let lean_arx = run_lean("arx");
    Tripod { lean, lean_arx, arx: arx_core, py: py_core, full: arx_full, arx_err, py_err }
}

fn assert_tripod(tag: &str, t: &Tripod) {
    assert_eq!(t.arx_err, t.py_err, "{tag}: desfecho do ARX × Python");
    first_diff(tag, "ARX × Python (vista do núcleo)", &t.arx, &t.py);
    if let Some(lean) = &t.lean {
        first_diff(tag, "Lean × ARX (journal do núcleo)", lean, &t.arx);
    }
    if let Some(model) = &t.lean_arx {
        first_diff(tag, "modelo Lean do ARX × ARX (vista do núcleo)", model, &t.arx);
    }
    let core: Vec<Json> = t.arx.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(arx::journal::Journal::verify(&core), Ok(()), "{tag}: cadeia do núcleo");
}

/// A injeção da Groq no núcleo: `Examples.remediate` de `lean/Aletheia/Examples.lean`, com o
/// modelo respondendo 0 (seguindo a injeção: o endosso recusa e o agente escala para 3) e 5
/// (o endosso aceita). Os traços são os dos dois `example` do Lean, agora em journal.
#[test]
fn groq_injection_in_the_core_is_the_same_on_the_three_backends() {
    let src = "module core.remediate

model planner { price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 0.0  per_tok = 0.0 }

tool scale_to(cluster: Cap[Cluster.Write], n: Int ^ Endorsed) -> Unit ! { Cluster.Write } reversible latency 0.0

policy SafeScale(n: Int[0..100]) -> Bool = 2 <= n && n <= 64

agent A(cluster: Cap[Cluster.Write]) budget [tok <= 100000, usd <= 1.0, wall <= 100.0] {
  on go(alerta: Int) -> Unit ! { Infer, Cluster.Write } {
    let plan = infer planner -> Int[0..100] { alerta = alerta }
    let n = refine plan by SafeScale retry 1 else { 3 }
    scale_to(cluster, n)
  }
}
";
    let world = json!({ "sinks": ["scale_to"], "latency": { "scale_to": 0.0 } });
    let mut heads = Vec::new();
    for (answer, accepted, scaled) in [(0, false, 3), (5, true, 5)] {
        // ρ₀ = [alerta = 42, cluster = cap 7]; a capacidade 7 concede `Cluster.Write`
        let core = json!({
            "effects": ["Cluster.Write"],
            "tools": [{ "name": "scale_to", "effect": 0, "arg": "int" }],
            "policies": [{ "name": "SafeScale", "lo": 2, "hi": 64 }],
            "grants": [[], [], [], [], [], [], [], [0]],
            "env": [{ "int": 42 }, { "cap": 7 }],
            "oracle": [answer],
            "exp": { "let": [
                { "infer": [0, "int", { "var": 0 }] },
                { "let": [
                    { "endorse": [0, { "var": 0 }, { "val": { "int": 3 } }] },
                    { "act": [0, 3, { "var": 0 }] }
                ] }
            ] }
        });
        let tag = format!("remediate/{answer}");
        let t = tripod(&tag, src, &core, &world, &json!({ "planner": [answer] }), vec![Value::Int(42)]);
        assert_tripod(&tag, &t);
        let lines: Vec<Json> = t.arx.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 3, "{tag}: endorse, act, end");
        assert_eq!(lines[0]["detail"]["accepted"], json!(accepted));
        assert_eq!(lines[1]["detail"]["args"], json!([scaled]));
        assert_eq!(lines[1]["detail"]["cap"], json!(["Cluster.Write"]));
        heads.push(lines[2]["hash"].clone());
    }
    assert_ne!(heads[0], heads[1], "os dois traços divergem na decisão de endosso");
}

/// O tripé sobre programas gerados: para cada semente, um programa do núcleo bem tipado e
/// linear (conferido por uma reimplementação das regras de `Typing.lean`), traduzido para a
/// superfície e aceito pelo `athc`. O journal do núcleo do avaliador do Lean, a vista do
/// núcleo do journal do ARX e a do runtime Python precisam coincidir byte a byte.
/// `ATH_TRIPOD_N` muda o número de sementes (padrão 160).
#[test]
fn core_tripod_lean_arx_python_agree_on_generated_programs() {
    let n: u64 = std::env::var("ATH_TRIPOD_N").ok().and_then(|v| v.parse().ok()).unwrap_or(160);
    let mut seen = std::collections::BTreeMap::<&str, usize>::new();
    let mut heads = std::collections::BTreeSet::new();
    let mut with_lean = 0;
    for seed in 0..n {
        let p = lambda_ath::generate(seed);
        p.check().unwrap_or_else(|e| panic!("semente {seed}: {e}"));
        let tag = format!("core/{seed}");
        let t = tripod(&tag, &p.to_ath(), &p.to_json(), &p.world(), &p.models(), vec![]);
        assert_tripod(&tag, &t);
        with_lean += usize::from(t.lean.is_some());
        for l in t.arx.lines() {
            let e: Json = serde_json::from_str(l).unwrap();
            let k = match (e["kind"].as_str().unwrap(), e["detail"]["accepted"].as_bool()) {
                ("endorse", Some(true)) => "endosso aceito",
                ("endorse", _) => "endosso recusado",
                ("act", _) => "ação",
                _ => "fim",
            };
            *seen.entry(k).or_default() += 1;
            heads.insert(e["hash"].as_str().unwrap().to_string());
        }
        let src = p.to_ath();
        for (needle, k) in [("for ", "laço"), ("if ", "ramo"), (" retry 1 else ", "refine")] {
            if src.contains(needle) {
                *seen.entry(k).or_default() += 1;
            }
        }
    }
    eprintln!("tripé: {n} programas ({with_lean} com o Lean) · cobertura {seen:?}");
    for k in ["endosso aceito", "endosso recusado", "ação", "laço", "ramo"] {
        assert!(seen.get(k).copied().unwrap_or(0) * 10 >= n as usize, "cobertura fraca de `{k}`: {seen:?}");
    }
    assert!(heads.len() as u64 > n, "os programas exercitam traços distintos");
}

/// O tripé com sagas: programas gerados com ferramentas de célula (algumas `compensable`,
/// algumas com a compensação recusada pelo provedor), sondas de intervalo, `atomic` e
/// `await`. Cada programa é bem tipado, linear e `Guarded` (todo `await` dentro de uma saga),
/// o domínio do T1–T3 com sagas. O Lean, o ARX e o Python precisam produzir o mesmo journal do
/// núcleo, agora com `fail`, `undo` e `recover`. A cobertura exige sagas que confirmam, que
/// compensam e recuperam, e que terminam em `CompensationFailed`, por compensação recusada e
/// por sonda que não observa a recuperação. `ATH_SAGA_N` muda o número de sementes (padrão 200).
#[test]
fn saga_tripod_lean_arx_python_agree_on_generated_programs() {
    let n: u64 = std::env::var("ATH_SAGA_N").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let mut seen = std::collections::BTreeMap::<&str, usize>::new();
    let mut heads = std::collections::BTreeSet::new();
    let mut with_lean = 0;
    for seed in 0..n {
        let p = lambda_ath::generate_saga(seed);
        p.check().unwrap_or_else(|e| panic!("semente {seed}: {e}"));
        let tag = format!("saga/{seed}");
        let t = tripod(&tag, &p.to_ath(), &p.to_json(), &p.world(), &p.models(), vec![]);
        assert_tripod(&tag, &t);
        with_lean += usize::from(t.lean.is_some());
        for l in t.full.lines() {
            let e: Json = serde_json::from_str(l).unwrap();
            let k = match e["kind"].as_str().unwrap() {
                "saga_commit" => "saga confirmada",
                "saga_recovered" => "saga recuperada",
                "compensation_failed" => "CompensationFailed",
                "await_ok" => "pós-condição observada",
                "await_timeout" => "pós-condição falhou",
                _ => continue,
            };
            *seen.entry(k).or_default() += 1;
        }
        for l in t.arx.lines() {
            let e: Json = serde_json::from_str(l).unwrap();
            let k = match (e["kind"].as_str().unwrap(), e["detail"]["ok"].as_bool()) {
                ("undo", Some(true)) => "compensação aplicada",
                ("undo", _) => "compensação recusada",
                ("recover", Some(false)) => "sonda de recuperação falsa",
                ("act", _) => "ação",
                _ => continue,
            };
            *seen.entry(k).or_default() += 1;
            heads.insert(e["hash"].as_str().unwrap().to_string());
        }
    }
    eprintln!("tripé com sagas: {n} programas ({with_lean} com o Lean) · cobertura {seen:?}");
    for k in [
        "saga confirmada",
        "saga recuperada",
        "CompensationFailed",
        "pós-condição observada",
        "pós-condição falhou",
        "compensação aplicada",
        "compensação recusada",
        "sonda de recuperação falsa",
    ] {
        assert!(seen.get(k).copied().unwrap_or(0) * 20 >= n as usize, "cobertura fraca de `{k}`: {seen:?}");
    }
    assert!(heads.len() as u64 > n, "os programas exercitam traços distintos");
}

/// O núcleo pede inteiros ao modelo (`infer m -> Int`): a superfície aceita saídas escalares
/// `Int` e `Bool` (o esquema e a máscara derivam do tipo, como para ADTs) e ainda recusa `()`.
#[test]
fn scalar_infer_outputs_are_accepted_and_unit_is_still_e0021() {
    let program = |out: &str, default: &str| {
        format!(
            "module core.scalar
model m0 {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 0.0  per_tok = 0.0 }}
policy P(x: {out}) -> Bool = true
agent A() budget [tok <= 100000, usd <= 1.0, wall <= 100.0] {{
  on go() -> Int ! {{ Infer }} {{
    let d = infer m0 -> {out} {{ q = 1 }}
    let v = refine d by P else {{ {default} }}
    1
  }}
}}
"
        )
    };
    let codes = |src: &str| match compile(src) {
        Ok(_) => Vec::new(),
        Err(d) => d.into_iter().map(|x| x.code).collect::<Vec<_>>(),
    };
    for (out, default) in [("Int[0..100]", "3"), ("Int", "3"), ("Bool", "false")] {
        let src = program(out, default);
        assert!(codes(&src).is_empty(), "`infer -> {out}`: {:?}", codes(&src));
    }
    assert!(codes(&program("Unit", "")).contains(&"E0021".to_string()));
}

/// Inventários confiáveis nomeados (`s.inventory`, `s.member`), como no agente do AgentDojo:
/// o mesmo inventário no prompt e na política, a fotografia no journal, e o mesmo journal nos
/// dois backends para um destinatário conhecido (endossado) e um injetado (recusado).
#[test]
fn named_inventories_are_isomorphic() {
    let src = "module bank.inventories
type Pay = | Pay { to: Text[..34], amount: Text[..16] }
model m0 { price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 0.0  per_tok = 0.0 }
tool pay(c: Cap[Bank.Transfer], to: Text[..34] ^ Endorsed, amount: Text[..16] ^ Endorsed) -> Unit
  ! { Bank.Transfer } irrevocable latency 0.0
policy Known(p: Pay) -> Bool = match p { Pay { to } => s.member(\"payees\", to) }
agent A(bank: Cap[Bank.Transfer]) budget [tok <= 100000, usd <= 1.0, wall <= 100.0] {
  on go() -> Text ! { Infer, Bank.Transfer } {
    let d = infer m0 -> Pay { request = \"refund my friend\", payees = s.inventory(\"payees\") }
    let p = refine d by Known retry 2 else { return \"recusado\" }
    match p { Pay { to, amount } => {
      commit pay(bank, to, amount)
      \"pago\"
    } }
  }
}
";
    let c = compile(src).unwrap_or_else(|d| panic!("{:?}", d.iter().map(|x| &x.code).collect::<Vec<_>>()));
    let py = ath_emit::python(&c.module, &c.check, &c.codex, "inventories.ath").expect("emissão");
    let path = scratch().join("inventories.py");
    std::fs::write(&path, py).unwrap();
    let world = json!({
        "sinks": ["pay"],
        "latency": { "pay": 0.0 },
        "inventories": { "payees": ["GB29NWBK60161331926819", "SE3550000000054910000003", "GB29NWBK60161331926819"] },
    });
    let friend = json!({ "Pay": { "to": "GB29NWBK60161331926819", "amount": "10.00" } });
    let attacker = json!({ "Pay": { "to": "US133000000121212121212", "amount": "0.01" } });
    let mut heads = Vec::new();
    for (tag, script) in [
        ("conhecido", json!({ "m0": [friend.clone()] })),
        ("injetado", json!({ "m0": [attacker.clone(), attacker.clone()] })),
        ("injetado-depois-conhecido", json!({ "m0": [attacker, friend] })),
    ] {
        let case = Case {
            name: format!("inventários/{tag}"),
            agent: "A",
            event: "go",
            world: world.clone(),
            models: script,
            masked: false,
            args: vec![],
        };
        let a = run_arx(&c, &case);
        assert!(a
            .journal
            .contains(r#""inventories":{"payees":["GB29NWBK60161331926819","SE3550000000054910000003"]}"#));
        heads.push(same(&c, &path, case));
    }
    heads.dedup();
    assert_eq!(heads.len(), 3, "os três roteiros produzem journals distintos");
}

// ------------------------------------------------------------------ Rev11: chaves no tripé

/// A renomeação canônica das chaves: a `i`-ésima chave distinta (a partir de 1), na ordem da
/// primeira aparição no journal do núcleo, passa a se chamar `k{i}`, como o ARX e o runtime
/// Python as cunham. Devolve o journal renomeado (com a
/// cadeia BLAKE3 refeita) e a bijeção `(nome original, nome canônico)`.
///
/// A renomeação é injetiva por construção (nomes distintos ganham índices distintos) e não
/// toca em mais nada: nenhum evento é removido, reordenado ou fundido. Por isso ela não
/// esconde perda, duplicação nem troca de eventos entre chaves (os testes de mutação abaixo
/// conferem): só a escolha dos nomes deixa de importar.
fn canon_keys(jsonl: &str) -> (String, Vec<(String, String)>) {
    let mut map: Vec<(String, String)> = Vec::new();
    let mut evs = Vec::new();
    for l in jsonl.lines() {
        let mut e: Json = serde_json::from_str(l).unwrap();
        let mut d = e["detail"].take();
        if let Some(k) = d.get("key").and_then(|k| k.as_str()).map(String::from) {
            let c = match map.iter().find(|(a, _)| *a == k) {
                Some((_, c)) => c.clone(),
                None => {
                    let c = format!("k{}", map.len() + 1);
                    map.push((k, c.clone()));
                    c
                }
            };
            d["key"] = json!(c);
        }
        evs.push((e["kind"].as_str().unwrap().to_string(), d));
    }
    (arx::journal::Journal::untimed(evs).to_jsonl(), map)
}

/// O desfecho de uma chave no protocolo (spec 3.5).
#[derive(Clone, Debug, PartialEq)]
enum KeyEnd {
    /// `Done` pela resposta da própria chamada (evento `act` imediatamente antes)
    DoneAtOnce,
    /// `Failed` pela recusa da própria chamada
    FailedAtOnce,
    /// depois de `pending`: as respostas das tentativas e o estado terminal
    Reconciled(Vec<String>, String),
}

/// O verificador do ciclo de vida das chaves, independente da representação: só usa a
/// igualdade de nomes dentro de um journal, nunca o nome em si. Para cada chave confere:
///
/// * exatamente um estado terminal (`settle`), que é o último evento da chave;
/// * `Done` só com evidência: a resposta da chamada (`act` logo antes) ou a última tentativa
///   da reconciliação com `done`;
/// * `Failed` só com evidência: a recusa da chamada ou a última tentativa com `not_applied`;
/// * `Transferred` só depois de esgotar o protocolo (`fuel` tentativas `unknown`, ou a única
///   tentativa `opaque`) e logo depois de um `transfer` da mesma chave;
/// * tentativas numeradas 1, 2, …; nenhuma tentativa sem `pending` antes;
/// * nenhuma obrigação desconhecida some: todo `pending` tem `settle`;
/// * `fail {cause: refused}` e `abort {cause: refused}` citam uma chave `Failed`; um `end`
///   com `aborted` exige uma chave `Transferred`.
///
/// Devolve o desfecho de cada chave, na ordem da primeira aparição.
fn lifecycle(jsonl: &str, fuel: usize) -> Result<Vec<(String, Option<String>, KeyEnd)>, String> {
    let evs: Vec<Json> = jsonl.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let mut order: Vec<String> = Vec::new();
    let mut per: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
    for (i, e) in evs.iter().enumerate() {
        let kind = e["kind"].as_str().unwrap();
        if !matches!(kind, "pending" | "query" | "settle" | "transfer") {
            continue;
        }
        let k = e["detail"]["key"].as_str().ok_or(format!("evento {i} ({kind}) sem chave"))?.to_string();
        if !per.contains_key(&k) {
            order.push(k.clone());
        }
        per.entry(k).or_default().push(i);
    }
    let mut out = Vec::new();
    let mut ended: std::collections::BTreeMap<String, String> = Default::default();
    for k in &order {
        let idx = &per[k];
        let kinds: Vec<&str> = idx.iter().map(|&i| evs[i]["kind"].as_str().unwrap()).collect();
        let settles = kinds.iter().filter(|&&x| x == "settle").count();
        if settles != 1 || *kinds.last().unwrap() != "settle" {
            return Err(format!("{k}: {settles} estado(s) terminal(is); eventos {kinds:?}"));
        }
        let last = idx[idx.len() - 1];
        let status = evs[last]["detail"]["status"].as_str().unwrap_or_default().to_string();
        // a ferramenta: a do `pending` ou, com `Done` na hora, a do `act` que é a evidência
        let tool = idx.iter().find_map(|&i| evs[i]["detail"]["tool"].as_str().map(String::from)).or_else(|| {
            (status == "done" && idx.len() == 1 && last > 0 && evs[last - 1]["kind"] == "act")
                .then(|| evs[last - 1]["detail"]["tool"].as_str().unwrap_or_default().to_string())
        });
        let end = if kinds[0] != "pending" {
            match (kinds.len(), status.as_str()) {
                (1, "done") if last > 0 && evs[last - 1]["kind"] == "act" => KeyEnd::DoneAtOnce,
                (1, "failed") => KeyEnd::FailedAtOnce,
                _ => return Err(format!("{k}: {status} sem obrigação nem evidência; eventos {kinds:?}")),
            }
        } else {
            if kinds.iter().filter(|&&x| x == "pending").count() != 1 {
                return Err(format!("{k}: mais de uma obrigação para a mesma chave"));
            }
            let qs: Vec<&Json> =
                idx.iter().filter(|&&i| evs[i]["kind"] == "query").map(|&i| &evs[i]["detail"]).collect();
            for (j, q) in qs.iter().enumerate() {
                if q["attempt"].as_u64() != Some(j as u64 + 1) {
                    return Err(format!("{k}: tentativa {} fora de ordem", q["attempt"]));
                }
            }
            let st: Vec<String> = qs.iter().map(|q| q["status"].as_str().unwrap_or_default().to_string()).collect();
            let lastq = st.last().map(String::as_str);
            let ok = match status.as_str() {
                "done" => lastq == Some("done"),
                "failed" => lastq == Some("not_applied"),
                "transferred" => {
                    let exhausted =
                        (st.len() == fuel && st.iter().all(|x| x == "unknown")) || (st.len() == 1 && st[0] == "opaque");
                    let handed = last > 0
                        && evs[last - 1]["kind"] == "transfer"
                        && evs[last - 1]["detail"]["key"].as_str() == Some(k.as_str());
                    exhausted && handed
                }
                _ => false,
            };
            let inner_ok = st.iter().take(st.len().saturating_sub(1)).all(|x| x == "unknown");
            if !ok || !inner_ok {
                return Err(format!("{k}: `{status}` sem a evidência do protocolo; tentativas {st:?}"));
            }
            KeyEnd::Reconciled(st, status.clone())
        };
        ended.insert(k.clone(), status);
        out.push((k.clone(), tool, end));
    }
    for (i, e) in evs.iter().enumerate() {
        let d = &e["detail"];
        match (e["kind"].as_str().unwrap(), d["cause"].as_str()) {
            ("fail" | "abort", Some("refused")) => {
                let k = d["key"].as_str().unwrap_or_default();
                if ended.get(k).map(String::as_str) != Some("failed") {
                    return Err(format!("evento {i}: recusa de `{k}`, que não terminou `Failed`"));
                }
            }
            ("end", _) if d["aborted"] == true && !ended.values().any(|s| s == "transferred") => {
                return Err("encerramento pelo tratador sem obrigação transferida".into());
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Compara os três backends num programa com chaves: ARX × Python byte a byte, e Lean × ARX
/// byte a byte também (Rev12): o λ_Ath cunha a chave com `mint` e a nomeia pela ordem de
/// cunhagem, como o ARX e o runtime Python. A comparação canônica (chaves renomeadas pela
/// primeira aparição) e o verificador do ciclo de vida continuam como critérios independentes
/// da representação. Devolve se os bytes crus do Lean e do ARX coincidem e o ciclo de vida das
/// chaves.
fn assert_tripod_keys(tag: &str, t: &Tripod, fuel: usize) -> (bool, Vec<(String, Option<String>, KeyEnd)>) {
    assert_eq!(t.arx_err, t.py_err, "{tag}: desfecho do ARX × Python");
    first_diff(tag, "ARX × Python (vista do núcleo)", &t.arx, &t.py);
    let core: Vec<Json> = t.arx.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(arx::journal::Journal::verify(&core), Ok(()), "{tag}: cadeia do núcleo");
    let la = lifecycle(&t.arx, fuel).unwrap_or_else(|e| panic!("{tag}: ciclo de vida no ARX: {e}\n{}", t.arx));
    let (ca, ma) = canon_keys(&t.arx);
    // o ARX cunha as chaves na ordem de execução: o seu journal já está na forma canônica
    assert!(ma.iter().all(|(a, c)| a == c), "{tag}: chaves do ARX fora da ordem de cunhagem: {ma:?}");
    let Some(lean) = &t.lean else { return (false, la) };
    let ll = lifecycle(lean, fuel).unwrap_or_else(|e| panic!("{tag}: ciclo de vida no Lean: {e}\n{lean}"));
    let (cl, ml) = canon_keys(lean);
    first_diff(tag, "Lean × ARX (journal do núcleo, chaves canônicas)", &cl, &ca);
    // a bijeção: a i-ésima chave do Lean é a i-ésima do ARX, com a mesma ferramenta e o mesmo desfecho
    assert_eq!(ml.len(), ma.len(), "{tag}: número de chaves");
    assert_eq!(ll.len(), la.len(), "{tag}: número de ciclos de vida");
    for ((kl, tl, el), (ka, ta, ea)) in ll.iter().zip(&la) {
        assert_eq!((tl, el), (ta, ea), "{tag}: a chave {kl} do Lean e a {ka} do ARX divergem");
    }
    if let Some(model) = &t.lean_arx {
        first_diff(tag, "modelo Lean do ARX × ARX (vista do núcleo)", model, &t.arx);
    }
    // sem renomeação: a identidade da operação é a mesma nos dois, evento a evento
    assert!(ml.iter().all(|(a, c)| a == c), "{tag}: chaves do Lean fora da ordem de cunhagem: {ml:?}");
    first_diff(tag, "Lean × ARX (journal do núcleo, bytes crus)", lean, &t.arx);
    (lean == &t.arx, ll)
}

/// O tripé com chaves: programas gerados com sagas e chamadas com chave (`call`), provedores
/// `idempotent`, `queryable` e `opaque`, falhas injetadas na chamada e nos reenvios e consultas
/// inconclusivas. Os três backends precisam coincidir, módulo a renomeação bijetiva das chaves,
/// e o ciclo de vida de toda chave precisa ser o do protocolo. A cobertura exige os sete
/// cenários da Fase 4. `ATH_KEYED_N` muda o número de sementes (padrão 120; a revisão roda 480).
#[test]
fn keyed_tripod_lean_arx_python_agree_on_generated_programs() {
    let n: u64 = std::env::var("ATH_KEYED_N").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
    let mut seen = std::collections::BTreeMap::<&str, usize>::new();
    let mut heads = std::collections::BTreeSet::new();
    let (mut with_lean, mut raw_equal, mut with_keys) = (0, 0, 0);
    for seed in 0..n {
        let p = lambda_ath::generate_keyed(seed);
        p.check().unwrap_or_else(|e| panic!("semente {seed}: {e}"));
        let tag = format!("keyed/{seed}");
        let t = tripod(&tag, &p.to_ath(), &p.to_json(), &p.world(), &p.models(), vec![]);
        let (raw, life) = assert_tripod_keys(&tag, &t, p.fuel);
        with_lean += usize::from(t.lean.is_some());
        with_keys += usize::from(!life.is_empty());
        raw_equal += usize::from(raw);
        // os cenários, pelo roteiro do provedor de cada chave que de fato rodou
        let runs = p.key_runs();
        // a ferramenta de cada chave do ARX, pelo journal completo (a recusa na hora não diz a
        // ferramenta na vista do núcleo; `tool_failed` diz)
        let arx_tool: std::collections::BTreeMap<String, String> = t
            .full
            .lines()
            .filter_map(|l| {
                let e: Json = serde_json::from_str(l).unwrap();
                matches!(e["kind"].as_str(), Some("tool" | "tool_failed" | "timeout_unknown"))
                    .then(|| {
                        Some((e["detail"]["key"].as_str()?.to_string(), e["detail"]["tool"].as_str()?.to_string()))
                    })
                    .flatten()
            })
            .collect();
        for (key, tool, end) in &life {
            // os nomes das chaves são os mesmos nos três backends (Rev12): a ferramenta de cada
            // chave sai do journal completo do ARX
            let ti: usize = arx_tool[key][1..].parse().unwrap();
            if let Some(tool) = tool {
                assert_eq!(tool, &format!("t{ti}"), "{tag}: a chave {key} apareceu na ferramenta {tool}");
            }
            let spec = p.tools[ti].key.as_ref().unwrap();
            let first = spec.faults.iter().find(|f| f.0 == 1).map(|f| f.1);
            let run = &runs.iter().find(|(t, _, _)| *t == ti).unwrap().2;
            match end {
                KeyEnd::DoneAtOnce => *seen.entry("Done na hora").or_default() += 1,
                KeyEnd::FailedAtOnce => *seen.entry("Failed na hora").or_default() += 1,
                KeyEnd::Reconciled(st, fin) => {
                    match first {
                        Some("timeout_lost") => *seen.entry("1 timeout antes de aplicar").or_default() += 1,
                        Some("timeout_applied") => *seen.entry("2 timeout depois de aplicar").or_default() += 1,
                        _ => {}
                    }
                    if spec.provider == lambda_ath::Prov::Idempotent && !st.is_empty() && run.applied {
                        *seen.entry("3 reenvio com a mesma chave").or_default() += 1;
                    }
                    if spec.provider == lambda_ath::Prov::Queryable && st.last().map(String::as_str) == Some("done") {
                        *seen.entry("4 consulta que confirma").or_default() += 1;
                    }
                    if st.iter().any(|s| s == "unknown" || s == "opaque") {
                        *seen.entry("5 consulta inconclusiva").or_default() += 1;
                    }
                    if fin == "transferred" {
                        *seen.entry("6 combustível esgotado e transferência").or_default() += 1;
                        if run.applied {
                            *seen.entry("6' transferida com o efeito aplicado").or_default() += 1;
                        }
                    }
                    if fin == "failed" {
                        *seen.entry("Failed reconciliado").or_default() += 1;
                    }
                }
            }
        }
        // 7: uma saga que falha por uma obrigação (encerramento ou recusa reconciliada) e não
        // consegue compensar
        let full: Vec<Json> = t.full.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        let mut pending_abort = false;
        for e in &full {
            match e["kind"].as_str().unwrap() {
                "saga_abort" => {
                    let r = e["detail"]["reason"].as_str().unwrap_or_default();
                    pending_abort = !r.starts_with('R') || r.starts_with("R0611");
                }
                "compensation_failed" if pending_abort => {
                    *seen.entry("7 compensação falha com obrigação pendente").or_default() += 1;
                    pending_abort = false;
                }
                "saga_recovered" | "saga_commit" => pending_abort = false,
                _ => {}
            }
        }
        match (&t.arx_err, t.arx.lines().last().map(|l| serde_json::from_str::<Json>(l).unwrap())) {
            (Some(_), _) => *seen.entry("evento recusado").or_default() += 1,
            (None, Some(e)) if e["detail"]["aborted"] == true => {
                *seen.entry("evento encerrado pelo tratador").or_default() += 1
            }
            _ => *seen.entry("evento com valor").or_default() += 1,
        }
        for l in t.arx.lines() {
            heads.insert(serde_json::from_str::<Json>(l).unwrap()["hash"].as_str().unwrap().to_string());
        }
    }
    eprintln!(
        "tripé com chaves: {n} programas ({with_lean} com o Lean; {with_keys} com chaves; {raw_equal} com os bytes crus Lean = ARX) · cobertura {seen:?}"
    );
    assert_eq!(raw_equal, with_lean, "Lean × ARX: bytes crus em todo programa com o Lean");
    for k in [
        "Done na hora",
        "Failed na hora",
        "1 timeout antes de aplicar",
        "2 timeout depois de aplicar",
        "3 reenvio com a mesma chave",
        "4 consulta que confirma",
        "5 consulta inconclusiva",
        "6 combustível esgotado e transferência",
        "6' transferida com o efeito aplicado",
        "7 compensação falha com obrigação pendente",
        "evento recusado",
        "evento encerrado pelo tratador",
        "evento com valor",
    ] {
        assert!(seen.get(k).copied().unwrap_or(0) * 60 >= n as usize, "cobertura fraca de `{k}`: {seen:?}");
    }
    assert!(heads.len() as u64 > n, "os programas exercitam traços distintos");
}

fn events_of(jsonl: &str) -> Vec<(String, Json)> {
    jsonl
        .lines()
        .map(|l| {
            let e: Json = serde_json::from_str(l).unwrap();
            (e["kind"].as_str().unwrap().to_string(), e["detail"].clone())
        })
        .collect()
}

fn rechain(evs: Vec<(String, Json)>) -> String {
    arx::journal::Journal::untimed(evs).to_jsonl()
}

fn is_key_event(k: &str) -> bool {
    matches!(k, "mint" | "pending" | "query" | "settle" | "transfer")
}

/// A normalização das chaves não esconde defeito. Sobre journals reais do tripé com chaves, o
/// journal do ARX é mutado de quatro formas, e cada mutante precisa ser recusado pela
/// comparação canônica com o Lean ou pelo verificador do ciclo de vida:
///
/// * **perda**: um evento de chave some;
/// * **duplicação**: um evento de chave aparece duas vezes, ou duas operações passam a
///   compartilhar a mesma chave;
/// * **troca**: dois eventos de chaves diferentes trocam de chave entre si;
/// * **consumo irregular**: o estado terminal vem antes da obrigação, ou uma tentativa vem
///   depois do estado terminal.
///
/// O controle positivo: uma renomeação bijetiva e consistente das chaves (outros nomes, outra
/// ordem) é aceita pelos dois critérios. É exatamente a diferença entre o Lean e o ARX.
#[test]
fn key_normalization_does_not_hide_loss_duplication_or_swap() {
    if lean_eval().is_none() {
        return;
    }
    let n: u64 = std::env::var("ATH_MUTANT_N").ok().and_then(|v| v.parse().ok()).unwrap_or(80);
    let mut caught = std::collections::BTreeMap::<&str, (usize, usize)>::new();
    let mut renamed = 0usize;
    let mut note = |k: &'static str, hit: bool| {
        let e = caught.entry(k).or_default();
        e.0 += 1;
        e.1 += usize::from(hit);
    };
    for seed in 0..n {
        let p = lambda_ath::generate_keyed(seed);
        let tag = format!("mutante/{seed}");
        let t = tripod(&tag, &p.to_ath(), &p.to_json(), &p.world(), &p.models(), vec![]);
        let lean = t.lean.clone().unwrap();
        let (cl, _) = canon_keys(&lean);
        let evs = events_of(&t.arx);
        let detected = |m: &str| canon_keys(m).0 != cl || lifecycle(m, p.fuel).is_err();
        assert!(!detected(&t.arx), "{tag}: o journal original precisa passar");
        let kidx: Vec<usize> = (0..evs.len()).filter(|&i| is_key_event(&evs[i].0)).collect();
        let keys: Vec<String> = {
            let mut v: Vec<String> = Vec::new();
            for &i in &kidx {
                let k = evs[i].1["key"].as_str().unwrap().to_string();
                if !v.contains(&k) {
                    v.push(k);
                }
            }
            v
        };
        // controle positivo: renomeação bijetiva consistente (ordem inversa, outro prefixo)
        if !keys.is_empty() {
            let m: Vec<(String, Json)> = evs
                .iter()
                .map(|(k, d)| {
                    let mut d = d.clone();
                    if let Some(x) = d.get("key").and_then(|x| x.as_str()) {
                        let i = keys.iter().position(|y| y == x).unwrap();
                        d["key"] = json!(format!("zz{}", keys.len() - i));
                    }
                    (k.clone(), d)
                })
                .collect();
            let m = rechain(m);
            assert!(!detected(&m), "{tag}: uma renomeação bijetiva foi recusada");
            renamed += 1;
        }
        for &i in &kidx {
            // perda
            let mut m = evs.clone();
            m.remove(i);
            note("perda de um evento de chave", detected(&rechain(m)));
            // duplicação de evento
            let mut m = evs.clone();
            m.insert(i + 1, evs[i].clone());
            note("evento de chave duplicado", detected(&rechain(m)));
        }
        for a in 0..keys.len() {
            for b in 0..keys.len() {
                if a == b {
                    continue;
                }
                // duas operações com a mesma chave: `b` passa a se chamar `a`
                let m: Vec<(String, Json)> = evs
                    .iter()
                    .map(|(k, d)| {
                        let mut d = d.clone();
                        if d.get("key").and_then(|x| x.as_str()) == Some(keys[b].as_str()) {
                            d["key"] = json!(keys[a]);
                        }
                        (k.clone(), d)
                    })
                    .collect();
                note("duas operações com a mesma chave", detected(&rechain(m)));
            }
        }
        for x in 0..kidx.len() {
            for y in x + 1..kidx.len() {
                let (i, j) = (kidx[x], kidx[y]);
                if evs[i].1["key"] == evs[j].1["key"] {
                    continue;
                }
                // se cada uma das duas chaves tem só este evento, trocar as chaves dos dois é
                // renomear as duas operações de forma consistente: não é defeito (controle)
                // (a chave também aparece em `fail`/`abort` de uma recusa: aí a troca muda qual
                // operação encerrou o evento, e precisa ser recusada)
                let alone = |e: usize| evs.iter().filter(|(_, d)| d.get("key") == Some(&evs[e].1["key"])).count() == 1;
                if alone(i) && alone(j) {
                    let mut m = evs.clone();
                    let (ki, kj) = (evs[i].1["key"].clone(), evs[j].1["key"].clone());
                    m[i].1["key"] = kj;
                    m[j].1["key"] = ki;
                    assert!(!detected(&rechain(m)), "{tag}: a troca de chaves de duas operações inteiras foi recusada");
                    renamed += 1;
                    continue;
                }
                // troca: os dois eventos trocam de chave
                let mut m = evs.clone();
                let (ki, kj) = (evs[i].1["key"].clone(), evs[j].1["key"].clone());
                m[i].1["key"] = kj;
                m[j].1["key"] = ki;
                note("dois eventos trocam de chave", detected(&rechain(m)));
            }
        }
        for k in &keys {
            let own: Vec<usize> = kidx.iter().copied().filter(|&i| evs[i].1["key"].as_str() == Some(k)).collect();
            let last = *own.last().unwrap();
            if own.len() >= 2 {
                // o estado terminal antes da obrigação
                let mut m = evs.clone();
                let s = m.remove(last);
                m.insert(own[0], s);
                note("estado terminal antes da obrigação", detected(&rechain(m)));
            }
            // uma tentativa depois do estado terminal
            let mut m = evs.clone();
            m.insert(last + 1, ("query".into(), json!({ "attempt": 1, "key": k, "status": "done" })));
            note("tentativa depois do estado terminal", detected(&rechain(m)));
        }
    }
    eprintln!("mutantes (total, recusados): {caught:?} · controle: {renamed} renomeações bijetivas aceitas");
    assert!(renamed >= 10, "poucos controles positivos");
    for (k, (total, hit)) in &caught {
        assert_eq!(total, hit, "a normalização escondeu `{k}`: {hit}/{total}");
        assert!(*total > 0);
    }
    for k in [
        "perda de um evento de chave",
        "evento de chave duplicado",
        "duas operações com a mesma chave",
        "dois eventos trocam de chave",
        "estado terminal antes da obrigação",
        "tentativa depois do estado terminal",
    ] {
        assert!(caught.get(k).is_some_and(|c| c.0 >= 10), "poucos mutantes de `{k}`: {caught:?}");
    }
}

fn ktool(cap: usize, provider: lambda_ath::Prov, faults: Vec<(u32, &'static str)>, unknown: u32) -> lambda_ath::Tool {
    lambda_ath::Tool {
        effect: cap,
        arg: lambda_ath::Ty::Int,
        cell: Some(cap),
        compensable: false,
        undo_fails: false,
        key: Some(lambda_ath::KeySpec { cap, provider, faults, unknown }),
        usd: 0,
        cusd: 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn key_prog(
    seed: u64,
    tools: Vec<lambda_ath::Tool>,
    probes: Vec<lambda_ath::Probe>,
    exp: lambda_ath::E,
    ret: lambda_ath::Ty,
    fuel: usize,
    hval: lambda_ath::V,
) -> lambda_ath::Prog {
    let caps: Vec<Vec<usize>> = (0..tools.len()).map(|i| vec![i]).collect();
    lambda_ath::Prog {
        seed,
        effects: caps.len(),
        tools,
        policies: vec![],
        caps,
        oracle: vec![],
        exp,
        ret,
        probes,
        fuel,
        hval,
        limit: None,
    }
}

/// Os sete cenários determinísticos da Fase 4, cada um com o journal do núcleo esperado,
/// conferido nos três backends byte a byte (a cunhagem `mint` abre o ciclo de vida de cada
/// chave):
///
/// 1. timeout antes da aplicação: a consulta diz que não aconteceu, `Failed`; a sonda de
///    recuperação observa o mundo intacto;
/// 2. timeout depois da aplicação e antes da confirmação: a consulta confirma, `Done`; a
///    pós-condição observa o efeito;
/// 3. repetição com a mesma chave: o reenvio devolve o recibo, sem repetir o efeito (a
///    pós-condição exige a célula em 5, não em 10);
/// 4. consulta que confirma depois de uma inconclusiva;
/// 5. consultas inconclusivas e, enfim, conclusivas (`not_applied`);
/// 6. combustível esgotado e transferência: o evento termina pelo tratador; numa saga, a sonda
///    observa que o efeito continua no mundo (`Transferred` não é "desfeito");
/// 7. falha de compensação com obrigação pendente: a obrigação é transferida, a saga não
///    consegue compensar a outra ação, `on compensation_failed` roda, e o journal mantém o
///    estado terminal da obrigação.
#[test]
fn keyed_scenarios_are_deterministic_on_the_three_backends() {
    use lambda_ath::{Probe, Prov, Tool, Ty, E, V};
    let call = |t: usize, k: usize, v: i64| E::Call(t, k, Box::new(E::Val(V::Int(v))));
    let let_ = |a: E, b: E| E::Let(Box::new(a), Box::new(b));
    let int = |n: i64| E::Val(V::Int(n));
    let saga = |body: E| E::Atomic(Box::new(body), 0, Box::new(int(1)), Box::new(int(2)));
    let cell = |c: usize, v: i64| Probe { cell: c, lo: v, hi: v };
    let cases: Vec<(&str, lambda_ath::Prog, Json, Option<&str>)> = vec![
        (
            "1 timeout antes da aplicação",
            key_prog(
                1,
                vec![ktool(0, Prov::Queryable, vec![(1, "timeout_lost")], 0)],
                vec![cell(0, 0)],
                saga(let_(call(0, 0, 5), int(0))),
                Ty::Int,
                2,
                V::Int(9),
            ),
            json!([["mint", {"key": "k1"}],
                   ["pending", {"key": "k1", "tool": "t0"}],
                   ["query", {"attempt": 1, "key": "k1", "status": "not_applied"}],
                   ["settle", {"key": "k1", "status": "failed"}],
                   ["fail", {"cause": "refused", "key": "k1"}],
                   ["recover", {"ok": true, "probe": "p0"}],
                   ["end", {"value": 1}]]),
            None,
        ),
        (
            "2 timeout depois da aplicação",
            key_prog(
                2,
                vec![ktool(0, Prov::Queryable, vec![(1, "timeout_applied")], 0)],
                vec![cell(0, 5)],
                saga(let_(call(0, 0, 5), let_(E::Await(0), int(0)))),
                Ty::Int,
                2,
                V::Int(9),
            ),
            json!([["mint", {"key": "k1"}],
                   ["pending", {"key": "k1", "tool": "t0"}],
                   ["query", {"attempt": 1, "key": "k1", "status": "done"}],
                   ["settle", {"key": "k1", "status": "done"}],
                   ["end", {"value": 0}]]),
            None,
        ),
        (
            "3 repetição com a mesma chave",
            key_prog(
                3,
                vec![ktool(0, Prov::Idempotent, vec![(1, "timeout_applied")], 0)],
                vec![cell(0, 5)],
                saga(let_(call(0, 0, 5), let_(E::Await(0), int(0)))),
                Ty::Int,
                2,
                V::Int(9),
            ),
            json!([["mint", {"key": "k1"}],
                   ["pending", {"key": "k1", "tool": "t0"}],
                   ["query", {"attempt": 1, "key": "k1", "status": "done"}],
                   ["settle", {"key": "k1", "status": "done"}],
                   ["end", {"value": 0}]]),
            None,
        ),
        (
            "4 consulta que confirma",
            key_prog(
                4,
                vec![ktool(0, Prov::Queryable, vec![(1, "timeout_applied")], 1)],
                vec![],
                call(0, 0, 5),
                Ty::Unit,
                3,
                V::Unit,
            ),
            json!([["mint", {"key": "k1"}],
                   ["pending", {"key": "k1", "tool": "t0"}],
                   ["query", {"attempt": 1, "key": "k1", "status": "unknown"}],
                   ["query", {"attempt": 2, "key": "k1", "status": "done"}],
                   ["settle", {"key": "k1", "status": "done"}],
                   ["end", {"value": null}]]),
            None,
        ),
        (
            "5 consultas inconclusivas e conclusiva",
            key_prog(
                5,
                vec![ktool(0, Prov::Queryable, vec![(1, "timeout_lost")], 2)],
                vec![],
                call(0, 0, 5),
                Ty::Unit,
                3,
                V::Unit,
            ),
            json!([["mint", {"key": "k1"}],
                   ["pending", {"key": "k1", "tool": "t0"}],
                   ["query", {"attempt": 1, "key": "k1", "status": "unknown"}],
                   ["query", {"attempt": 2, "key": "k1", "status": "unknown"}],
                   ["query", {"attempt": 3, "key": "k1", "status": "not_applied"}],
                   ["settle", {"key": "k1", "status": "failed"}],
                   ["abort", {"cause": "refused", "key": "k1"}]]),
            Some("R0611"),
        ),
        (
            "6 combustível esgotado e transferência",
            key_prog(
                6,
                vec![ktool(
                    0,
                    Prov::Idempotent,
                    vec![(1, "timeout_lost"), (2, "timeout_lost"), (3, "timeout_lost")],
                    0,
                )],
                vec![],
                let_(call(0, 0, 5), int(0)),
                Ty::Int,
                2,
                V::Int(9),
            ),
            json!([["mint", {"key": "k1"}],
                   ["pending", {"key": "k1", "tool": "t0"}],
                   ["query", {"attempt": 1, "key": "k1", "status": "unknown"}],
                   ["query", {"attempt": 2, "key": "k1", "status": "unknown"}],
                   ["transfer", {"key": "k1"}],
                   ["settle", {"key": "k1", "status": "transferred"}],
                   ["end", {"aborted": true, "value": 9}]]),
            None,
        ),
        (
            "6' transferida com o efeito no mundo",
            key_prog(
                7,
                vec![ktool(0, Prov::Queryable, vec![(1, "timeout_applied")], 5)],
                vec![cell(0, 5)],
                saga(let_(call(0, 0, 5), int(0))),
                Ty::Int,
                2,
                V::Int(9),
            ),
            json!([["mint", {"key": "k1"}],
                   ["pending", {"key": "k1", "tool": "t0"}],
                   ["query", {"attempt": 1, "key": "k1", "status": "unknown"}],
                   ["query", {"attempt": 2, "key": "k1", "status": "unknown"}],
                   ["transfer", {"key": "k1"}],
                   ["settle", {"key": "k1", "status": "transferred"}],
                   ["fail", {"cause": "abort"}],
                   ["recover", {"ok": true, "probe": "p0"}],
                   ["end", {"aborted": true, "value": 9}]]),
            None,
        ),
        (
            "7 compensação falha com obrigação pendente",
            key_prog(
                8,
                vec![
                    ktool(0, Prov::Queryable, vec![(1, "timeout_lost")], 5),
                    Tool {
                        effect: 1,
                        arg: Ty::Int,
                        cell: Some(1),
                        compensable: true,
                        undo_fails: true,
                        key: None,
                        usd: 0,
                        cusd: 0,
                    },
                ],
                vec![cell(1, 0)],
                saga(let_(E::Act(1, 0, Box::new(int(3))), let_(call(0, 2, 5), int(0)))),
                Ty::Int,
                1,
                V::Int(9),
            ),
            json!([["act", {"args": [3], "cap": ["Core.E1"], "tool": "t1"}],
                   ["mint", {"key": "k1"}],
                   ["pending", {"key": "k1", "tool": "t0"}],
                   ["query", {"attempt": 1, "key": "k1", "status": "unknown"}],
                   ["transfer", {"key": "k1"}],
                   ["settle", {"key": "k1", "status": "transferred"}],
                   ["fail", {"cause": "abort"}],
                   ["undo", {"args": [3], "ok": false, "tool": "t1"}],
                   ["recover", {"ok": false, "probe": "p0"}],
                   ["end", {"value": 2}]]),
            None,
        ),
    ];
    for (name, p, want, err) in cases {
        p.check().unwrap_or_else(|e| panic!("{name}: {e}"));
        let tag = format!("cenário {name}");
        let t = tripod(&tag, &p.to_ath(), &p.to_json(), &p.world(), &p.models(), vec![]);
        assert_tripod_keys(&tag, &t, p.fuel);
        assert_eq!(t.arx_err.as_deref(), err, "{tag}: desfecho");
        let got: Vec<Json> = events_of(&canon_keys(&t.arx).0).into_iter().map(|(k, d)| json!([k, d])).collect();
        assert_eq!(Json::Array(got), want, "{tag}: journal do núcleo");
        if let Some(lean) = &t.lean {
            assert_eq!(lean, &t.arx, "{tag}: Lean × ARX, bytes crus");
        }
    }
}

/// Rev12, M4: o contrato `requires` é um monitor na fronteira do efeito. Os argumentos são
/// confiáveis (literais do programa) e nenhuma política os julga; o contrato da ferramenta
/// recusa o valor acima do limite confiável antes do host, e os dois runtimes escrevem o mesmo
/// journal.
#[test]
fn contracts_monitor() {
    let src = r#"module contracts

tool pay(c: Cap[Bank.Transfer], to: Text[..8] ^ Endorsed, amount: Text[..16] ^ Endorsed) -> Text
  ! { Bank.Transfer } irrevocable latency 1.0
  requires s.member("payees", to) && s.within("limit", amount)

agent A(bank: Cap[Bank.Transfer]) budget [usd <= 1.0, wall <= 100.0] {
  on go(n: Int) -> Text ! { Bank.Transfer } {
    let r = commit pay(bank, "A", "50.00")
    commit pay(bank, "A", "500.00")
  }
}
"#;
    let world = json!({ "sinks": ["pay"], "inventories": { "payees": ["A"], "limit": ["100.00"] } });
    let models = json!({});
    let c = compile(src)
        .unwrap_or_else(|d| panic!("o athc recusou\n{:?}", d.iter().map(|x| (&x.code, &x.msg)).collect::<Vec<_>>()));
    let provider = Box::new(Scripted::from_json(&models).unwrap());
    let mut arx = Arx::new(&c.module, Box::new(SimWorld::from_json(&world).unwrap()), provider);
    arx.contracts = athc::contract_hashes(&c);
    let err = arx.run("A", "go", vec![Value::Int(0)]).expect_err("o segundo pagamento passa do limite");
    assert_eq!(err.code, "R0630", "{}", err.msg);
    let full = arx.journal.to_jsonl();
    let kinds: Vec<String> =
        full.lines().map(|l| serde_json::from_str::<Json>(l).unwrap()["kind"].as_str().unwrap().to_string()).collect();
    assert_eq!(kinds.iter().filter(|k| *k == "tool").count(), 1, "só o pagamento dentro do contrato chega ao host");
    assert_eq!(kinds.iter().filter(|k| *k == "contract_refused").count(), 1);
    let refused = kinds.iter().position(|k| k == "contract_refused").unwrap();
    assert!(kinds[refused + 1..].iter().all(|k| k != "tool"), "nada executa depois da recusa");
    // Python, sobre o código emitido: o mesmo journal, byte a byte
    let dir = scratch();
    let py = ath_emit::python(&c.module, &c.check, &c.codex, "contracts.ath").expect("emissão");
    let (pp, w, m, j, r) = (
        dir.join("contracts.py"),
        dir.join("contracts.world.json"),
        dir.join("contracts.models.json"),
        dir.join("contracts.journal.jsonl"),
        dir.join("contracts.report.json"),
    );
    std::fs::write(&pp, py).unwrap();
    std::fs::write(&w, world.to_string()).unwrap();
    std::fs::write(&m, models.to_string()).unwrap();
    let out = Command::new("python3")
        .env("PYTHONPATH", root().join("runtimes/python"))
        .args(["-m", "aletheia_runtime", "run"])
        .arg(&pp)
        .arg("--world")
        .arg(&w)
        .arg("--models")
        .arg(&m)
        .args(["--provider", "scripted", "--agent", "A", "--on", "go", "--journal"])
        .arg(&j)
        .arg("--report")
        .arg(&r)
        .args(["--arg-json", "0"])
        .output()
        .expect("python3");
    let report: Json = serde_json::from_str(&std::fs::read_to_string(&r).unwrap()).unwrap();
    assert_eq!(report["error"]["code"].as_str(), Some("R0630"), "{}", String::from_utf8_lossy(&out.stderr));
    first_diff("contratos", "ARX × Python (journal completo)", &full, &std::fs::read_to_string(&j).unwrap());
}

// ------------------------------------------------------------------ M-IV: orçamento com compensações pagas

/// O `athc` sem o estrato E6: o programa como o ARX o executaria se o orçamento estático não
/// tivesse sido conferido. É assim que o diferencial exercita o medidor do runtime também em
/// programas que o `athc` recusaria (B02xx): o invariante `arx_budget_closed` não depende de
/// tipagem.
fn compile_unbudgeted(src: &str) -> Compiled {
    let module = ath_syntax::parse_module(src).unwrap_or_else(|e| panic!("parse: {}\n{src}", e.msg));
    let mut check = ath_check::check_module(&module);
    check.diags.retain(|d| !d.code.starts_with("B02"));
    assert!(
        check.ok(),
        "o athc recusou por outro motivo: {:?}\n{src}",
        check.diags.iter().map(|d| (&d.code, &d.msg)).collect::<Vec<_>>()
    );
    let codex = ath_codex::address(&module).unwrap();
    Compiled { module, check, codex }
}

/// O que cada backend mediu num programa com custos.
#[derive(Debug)]
struct Paid {
    /// gasto do ARX, do Python e do modelo do ARX em Lean, em 1/1024 US$
    arx: u64,
    py: u64,
    model: u64,
    /// a reserva que sobrou no modelo no fim (os quadros fecharam: zero)
    model_resv: u64,
    /// o gasto contabilizado pelo λ_Ath, se ele terminou com valor ou falha capturável
    eval: Option<u64>,
    eval_budget: bool,
    model_outcome: String,
    arx_err: Option<String>,
    py_err: Option<String>,
    arx_core: String,
    py_core: String,
    lean_arx: Option<String>,
    lean_run: Option<String>,
    full: String,
}

fn units(x: f64) -> u64 {
    let u = x * 1024.0;
    assert!((u - u.round()).abs() < 1e-9 && u >= 0.0, "gasto fora da grade de 1/1024 US$: {x}");
    u.round() as u64
}

fn paid_run(tag: &str, p: &lambda_ath::Prog) -> Paid {
    let src = p.to_ath();
    let c = compile_unbudgeted(&src);
    let (core, world, models) = (p.to_json(), p.world(), p.models());
    let provider = Box::new(Scripted::from_json(&models).unwrap());
    let mut arx = Arx::new(&c.module, Box::new(SimWorld::from_json(&world).unwrap()), provider);
    arx.contracts = athc::contract_hashes(&c);
    let arx_err = match arx.run("A", "go", vec![]) {
        Ok(_) => None,
        Err(e) if ["R0600", "R0611", "R0801"].contains(&e.code.as_str()) => Some(e.code.clone()),
        Err(e) => panic!("{tag}: o ARX abortou ({} {})\n{src}", e.code, e.msg),
    };
    let arx_spent = units(arx.meter.usd);
    assert!(arx.meter.held_usd.abs() < 1e-12, "{tag}: reserva do ARX não liberada: {}", arx.meter.held_usd);
    let arx_core =
        arx::journal::Journal::core_view_with(&arx.journal.entries, &athc::compensable_tools(&c.module)).to_jsonl();
    let full = arx.journal.to_jsonl();
    // Python
    let dir = scratch();
    let file = tag.replace(|ch: char| !ch.is_ascii_alphanumeric(), "_");
    let py = ath_emit::python(&c.module, &c.check, &c.codex, &format!("{file}.ath")).expect("emissão");
    let (pp, w, m, j, cj, r) = (
        dir.join(format!("{file}.py")),
        dir.join(format!("{file}.world.json")),
        dir.join(format!("{file}.models.json")),
        dir.join(format!("{file}.journal.jsonl")),
        dir.join(format!("{file}.core.jsonl")),
        dir.join(format!("{file}.report.json")),
    );
    std::fs::write(&pp, py).unwrap();
    std::fs::write(&w, world.to_string()).unwrap();
    std::fs::write(&m, models.to_string()).unwrap();
    let out = Command::new("python3")
        .env("PYTHONPATH", root().join("runtimes/python"))
        .args(["-m", "aletheia_runtime", "run"])
        .arg(&pp)
        .arg("--world")
        .arg(&w)
        .arg("--models")
        .arg(&m)
        .args(["--provider", "scripted", "--agent", "A", "--on", "go", "--journal"])
        .arg(&j)
        .arg("--core-journal")
        .arg(&cj)
        .arg("--report")
        .arg(&r)
        .output()
        .expect("python3");
    let report: Json = serde_json::from_str(&std::fs::read_to_string(&r).unwrap_or_else(|_| {
        panic!("{tag}: o Python não escreveu o relatório\n{}", String::from_utf8_lossy(&out.stderr))
    }))
    .unwrap();
    let py_err = report["error"]["code"].as_str().map(String::from);
    let py_spent = units(report["spent"]["usd"].as_f64().unwrap());
    let py_core = std::fs::read_to_string(&cj).unwrap();
    // Lean
    let lean = lean_eval().expect("o diferencial do M-IV precisa do avaliador do Lean");
    let pj = dir.join(format!("{file}.core.json"));
    std::fs::write(&pj, core.to_string()).unwrap();
    let call = |mode: &str| Command::new(&lean).arg(mode).arg(&pj).output().unwrap();
    let mo = call("meter");
    assert!(mo.status.success(), "{tag}: meter: {}", String::from_utf8_lossy(&mo.stderr));
    let meter: Json = serde_json::from_slice(&mo.stdout).unwrap();
    let ok = |o: std::process::Output| o.status.success().then(|| String::from_utf8(o.stdout).unwrap());
    Paid {
        arx: arx_spent,
        py: py_spent,
        model: meter["arx"]["spent"].as_u64().unwrap(),
        model_resv: meter["arx"]["resv"].as_u64().unwrap(),
        eval: meter["eval"]["spent"].as_u64(),
        eval_budget: meter["eval"]["outcome"] == "budget",
        model_outcome: meter["arx"]["outcome"].as_str().unwrap().to_string(),
        arx_err,
        py_err,
        arx_core,
        py_core,
        lean_arx: ok(call("arx")),
        lean_run: ok(call("run")),
        full,
    }
}

/// **M-IV, o diferencial do medidor.** Programas gerados com sagas (e chaves nas sementes
/// ímpares), cada ferramenta e cada compensador com custo positivo exato, e um limite sorteado.
/// Para cada um:
///
/// * o gasto do modelo do ARX em Lean, do ARX e do Python coincidem exatamente, e nenhum passa
///   do limite — inclusive quando a saga falha e liquida compensadores pagos;
/// * a reserva termina zerada nos três;
/// * a vista do núcleo do ARX e a do Python coincidem byte a byte, e com a do modelo quando o
///   modelo termina (o ARX recusa no topo com `R0801` exatamente quando o modelo recusa);
/// * quando o λ_Ath termina sem erro de orçamento, o journal dele é o do ARX e o gasto do ARX não
///   passa do contabilizado (`arx_refines`).
///
/// A cobertura exige compensações pagas, recusas pela reserva e execuções no fio do limite.
/// `ATH_PAID_N` muda o número de sementes (padrão 200).
#[test]
fn paid_compensation_meter_agrees_and_never_exceeds_the_limit() {
    if lean_eval().is_none() {
        return;
    }
    let n: u64 = std::env::var("ATH_PAID_N").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let mut seen = std::collections::BTreeMap::<&str, usize>::new();
    for seed in 0..n {
        let p = lambda_ath::generate_paid(seed);
        p.check().unwrap_or_else(|e| panic!("semente {seed}: {e}"));
        let lim = p.limit.unwrap();
        let tag = format!("paid/{seed}");
        let r = paid_run(&tag, &p);
        assert_eq!(r.arx_err, r.py_err, "{tag}: desfecho do ARX × Python");
        assert_eq!(
            (r.model, r.arx, r.py),
            (r.model, r.model, r.model),
            "{tag}: gasto modelo × ARX × Python\n{}",
            p.to_ath()
        );
        assert!(r.model <= lim, "{tag}: o gasto {} passou do limite {lim}", r.model);
        assert_eq!(r.model_resv, 0, "{tag}: a reserva do modelo não fechou");
        first_diff(&tag, "ARX × Python (vista do núcleo)", &r.arx_core, &r.py_core);
        if r.arx_err.as_deref() == Some("R0801") {
            assert_eq!(r.model_outcome, "other", "{tag}: o ARX recusou pelo orçamento e o modelo não");
            assert!(r.lean_arx.is_none(), "{tag}: o modelo produziu journal de um desfecho recusado");
            *seen.entry("recusa no topo pela reserva").or_default() += 1;
        } else {
            let model =
                r.lean_arx.as_ref().unwrap_or_else(|| panic!("{tag}: o modelo não terminou ({})", r.model_outcome));
            first_diff(&tag, "modelo Lean do ARX × ARX (vista do núcleo)", model, &r.arx_core);
        }
        if r.eval_budget {
            *seen.entry("λ_Ath recusa antes (custo contabilizado)").or_default() += 1;
        } else if let Some(e) = r.eval {
            assert!(r.model <= e && e <= lim, "{tag}: ARX {} ≤ λ_Ath {e} ≤ limite {lim}", r.model);
            if let Some(run) = &r.lean_run {
                first_diff(&tag, "Lean × ARX (journal do núcleo)", run, &r.arx_core);
            }
            *seen.entry("λ_Ath termina").or_default() += 1;
        }
        let mut paid_comp = false;
        for l in r.full.lines() {
            let e: Json = serde_json::from_str(l).unwrap();
            match e["kind"].as_str().unwrap() {
                "compensate" if e["detail"]["ok"] == json!(true) => {
                    let comp = &p.tools[e["detail"]["of"].as_str().unwrap()[1..].parse::<usize>().unwrap()];
                    paid_comp |= comp.cusd > 0;
                }
                "saga_abort" if e["detail"]["reason"].as_str().is_some_and(|x| x.starts_with("R0801")) => {
                    *seen.entry("recusa pela reserva dentro da saga").or_default() += 1;
                }
                "resolved" if e["detail"]["status"] == "failed" => {
                    *seen.entry("R0611 (reserva liberada)").or_default() += 1;
                }
                "compensation_overrun" => panic!("{tag}: compensador acima do declarado"),
                _ => {}
            }
        }
        if paid_comp {
            *seen.entry("compensação paga").or_default() += 1;
        }
        if r.model == lim && lim > 0 {
            *seen.entry("no fio do limite").or_default() += 1;
        }
    }
    eprintln!("M-IV: {n} programas · {seen:?}");
    for (k, den) in
        [("compensação paga", 20), ("recusa no topo pela reserva", 20), ("λ_Ath termina", 4), ("no fio do limite", 50)]
    {
        assert!(seen.get(k).copied().unwrap_or(0) * den >= n as usize, "cobertura fraca de `{k}`: {seen:?}");
    }
}

/// **O contraexemplo da Rev12, nos runtimes.** `atomic { t0(c, 1); await p0 } recover p0`, com
/// `t0` de US$ 1 compensável por `u0` de US$ 1 e uma sonda que nunca confirma: na Rev12 o ARX
/// terminava em US$ 2 sob limite US$ 1 (`Arx.Counter.comp_cost_breaks_budget`). Agora, sob
/// limite 1, o `athc` recusa (B0202) e, sem a conferência estática, o medidor recusa antes do
/// efeito: nada é feito, nada é gasto. Sob limite 2 a saga compensa e os três param em 2.
#[test]
fn rev12_paid_compensation_counterexample_is_closed() {
    if lean_eval().is_none() {
        return;
    }
    use lambda_ath::{Probe, Tool, Ty, E, V};
    let mk = |limit: u64| {
        let mut p = key_prog(
            0,
            vec![Tool {
                effect: 0,
                arg: Ty::Int,
                cell: Some(0),
                compensable: true,
                undo_fails: false,
                key: None,
                usd: 1024,
                cusd: 1024,
            }],
            vec![Probe { cell: 0, lo: 7, hi: 7 }],
            E::Atomic(
                Box::new(E::Let(Box::new(E::Act(0, 0, Box::new(E::Val(V::Int(1))))), Box::new(E::Await(0)))),
                0,
                Box::new(E::Val(V::Unit)),
                Box::new(E::Val(V::Unit)),
            ),
            Ty::Unit,
            0,
            V::Unit,
        );
        p.limit = Some(limit);
        p
    };
    let p1 = mk(1024);
    let diags = compile(&p1.to_ath()).err().expect("o athc precisa recusar o limite 1");
    assert!(diags.iter().any(|d| d.code == "B0202"), "{:?}", diags.iter().map(|d| &d.code).collect::<Vec<_>>());
    let r1 = paid_run("rev12/limite-1", &p1);
    assert_eq!((r1.model, r1.arx, r1.py), (0, 0, 0), "nada é gasto: {r1:?}");
    assert!(r1.eval_budget, "o λ_Ath recusa pelo custo contabilizado (ação + estorno = 2)");
    assert!(r1.full.contains("R0801"), "o medidor do ARX recusa pela reserva:\n{}", r1.full);
    let p2 = mk(2048);
    assert!(compile(&p2.to_ath()).is_ok(), "sob limite 2 o programa é aceito");
    let r2 = paid_run("rev12/limite-2", &p2);
    assert_eq!((r2.model, r2.arx, r2.py, r2.eval), (2048, 2048, 2048, Some(2048)), "{r2:?}");
    first_diff("rev12/limite-2", "modelo × ARX", r2.lean_arx.as_ref().unwrap(), &r2.arx_core);
    first_diff("rev12/limite-2", "λ_Ath × ARX", r2.lean_run.as_ref().unwrap(), &r2.arx_core);
    assert!(r2.arx_core.contains("\"undo\""), "a saga compensou");
}

// ------------------------------------------------------------------ M-III: WAL, queda e recuperação

/// As chamadas `(ferramenta, chave)` que um host recebeu.
type Calls = std::rc::Rc<std::cell::RefCell<Vec<(String, Option<u64>)>>>;

/// O host da recuperação, que registra cada chamada `(ferramenta, chave)`: a recuperação só pode
/// reenviar a um provedor `idempotent`.
struct Counting<'a> {
    inner: Box<dyn arx::host::Host + 'a>,
    calls: Calls,
}

impl arx::host::Host for Counting<'_> {
    fn call(&mut self, tool: &str, args: &[Value]) -> Result<(Value, f64), arx::host::HostError> {
        let key = args.iter().find_map(|v| if let Value::Key(k) = v { Some(*k) } else { None });
        self.calls.borrow_mut().push((tool.to_string(), key));
        self.inner.call(tool, args)
    }
    fn status(&mut self, tool: &str, key: u64) -> (arx::host::Status, f64) {
        self.inner.status(tool, key)
    }
    fn summary(&self) -> Vec<String> {
        self.inner.summary()
    }
    fn applied(&self, tool: &str, key: u64) -> Option<bool> {
        self.inner.applied(tool, key)
    }
}

fn quiet_crashes() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let msg = info.payload().downcast_ref::<String>().map(String::as_str).unwrap_or("");
            if !msg.starts_with(arx::wal::CRASH) {
                prev(info);
            }
        }));
    });
}

/// Uma execução com queda injetada e o reinício: o WAL que sobrou, o WAL depois da recuperação,
/// o host que sobreviveu (o provedor) e as chamadas da recuperação. `None` se o programa
/// terminou antes de chegar ao ponto de queda.
#[allow(clippy::type_complexity)]
fn crash_and_recover<'m>(
    c: &'m Compiled,
    p: &lambda_ath::Prog,
    at: arx::wal::CrashPoint,
    nth: u32,
) -> Option<(arx::journal::Journal, arx::journal::Journal, Box<dyn arx::host::Host + 'm>, Vec<(String, Option<u64>)>)> {
    quiet_crashes();
    let provider = Box::new(Scripted::from_json(&p.models()).unwrap());
    let mut a = Arx::new(&c.module, Box::new(SimWorld::from_json(&p.world()).unwrap()), provider);
    let mut w = arx::wal::Wal::memory();
    w.crash = Some((at, nth));
    a.wal = Some(w);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = a.run("A", "go", vec![]);
    }));
    match r {
        Ok(()) => return None,
        Err(e) => {
            let msg = e.downcast_ref::<String>().cloned().unwrap_or_default();
            assert!(msg.starts_with(arx::wal::CRASH), "pânico inesperado: {msg}");
        }
    }
    // o reinício: um processo novo, o WAL persistido e o mesmo provedor remoto
    let host = std::mem::replace(&mut a.host, Box::new(SimWorld::from_json(&json!({})).unwrap()));
    let crashed = arx::journal::Journal::from_entries(a.wal.take().unwrap().log.entries);
    let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let provider = Box::new(Scripted::from_json(&p.models()).unwrap());
    let mut b = Arx::new(&c.module, Box::new(Counting { inner: host, calls: calls.clone() }), provider);
    b.wal = Some(arx::wal::Wal::resume(arx::journal::Journal::from_entries(crashed.entries.clone())));
    b.recover(3);
    let after = arx::journal::Journal::from_entries(b.wal.take().unwrap().log.entries);
    let host = std::mem::replace(&mut b.host, Box::new(SimWorld::from_json(&json!({})).unwrap()));
    let calls = calls.borrow().clone();
    Some((crashed, after, host, calls))
}

/// **M-III, a queda em cada ponto.** Programas gerados com chamadas com chave (os três
/// provedores, falhas na chamada e nos reenvios, consultas inconclusivas) e sagas, com a queda do
/// processo injetada em cada ponto do protocolo — depois de persistir a intenção, depois do envio,
/// com o desfecho conhecido e ainda não persistido, e depois de persisti-lo — de cada uma das
/// primeiras operações com chave. Depois do reinício e da recuperação:
///
/// * toda intenção tem **exatamente um** desfecho (`close` ou `escalate`): nenhuma obrigação some;
/// * `close done` só com o efeito presente no provedor e `close failed` só com ele ausente;
/// * a recuperação só reenvia ao provedor `idempotent`; `queryable` é consultado e `opaque`, se a
///   queda o deixou em aberto, é escalado;
/// * toda saga interrompida é encerrada, e nenhuma operação refutada é compensada.
///
/// `ATH_WAL_N` muda o número de sementes (padrão 120).
#[test]
fn wal_crash_recovery_never_loses_an_obligation() {
    use arx::wal::{CrashPoint, WalState};
    let n: u64 = std::env::var("ATH_WAL_N").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    for seed in 0..n {
        let p = lambda_ath::generate_keyed(seed);
        let src = p.to_ath();
        let c = compile(&src).unwrap_or_else(|d| panic!("{seed}: {:?}", d.iter().map(|x| &x.code).collect::<Vec<_>>()));
        let prov: std::collections::HashMap<String, String> = c
            .module
            .items
            .iter()
            .filter_map(|i| match i {
                ath_syntax::ast::Item::Tool(t) => Some((t.name.name.clone(), t.provider.name().to_string())),
                _ => None,
            })
            .collect();
        for at in [CrashPoint::Intent, CrashPoint::Sent, CrashPoint::Confirmed, CrashPoint::Closed] {
            for nth in 1..=3 {
                let tag = format!("wal/{seed}/{}/{nth}", at.name());
                let Some((crashed, after, host, calls)) = crash_and_recover(&c, &p, at, nth) else { continue };
                *seen.entry(format!("queda em {}", at.name())).or_default() += 1;
                assert_eq!(arx::journal::Journal::verify(&after.entries), Ok(()), "{tag}: cadeia do WAL");
                let before = WalState::of(&crashed);
                let st = WalState::of(&after);
                assert!(st.open().is_empty(), "{tag}: intenções em aberto depois da recuperação: {:?}", st.open());
                for (k, d) in &st.intents {
                    let outs = after
                        .entries
                        .iter()
                        .filter(|e| {
                            matches!(e["kind"].as_str(), Some("close" | "escalate")) && e["detail"]["key"] == json!(k)
                        })
                        .count();
                    assert_eq!(outs, 1, "{tag}: {k} tem {outs} desfechos\n{}", after.to_jsonl());
                    let tool = d["tool"].as_str().unwrap();
                    let kn: u64 = k[1..].parse().unwrap();
                    let truth = host.applied(tool, kn).unwrap();
                    match st.outcome[k].as_str() {
                        "done" => assert!(truth, "{tag}: {k} fechada `done` sem o efeito no provedor"),
                        "failed" => assert!(!truth, "{tag}: {k} fechada `failed` com o efeito no provedor"),
                        _ => {}
                    }
                    let was_open = before.open().iter().any(|(x, _)| x == k);
                    if was_open {
                        let p = prov[tool].as_str();
                        *seen.entry(format!("recuperada ({p} → {})", st.outcome[k])).or_default() += 1;
                        if p != "idempotent" {
                            assert!(
                                !calls.iter().any(|(t, key)| t == tool && *key == Some(kn)),
                                "{tag}: a recuperação reenviou {k} a um provedor {p}"
                            );
                        }
                        if p == "opaque" {
                            assert_eq!(st.outcome[k], "escalated", "{tag}: {k} opaco fechado sem evidência");
                        }
                    }
                }
                for (sid, sg) in &st.sagas {
                    assert!(sg.ended, "{tag}: saga {sid} sem encerramento");
                    if !before.sagas.get(sid).is_some_and(|s| s.ended) {
                        *seen.entry("saga interrompida encerrada".into()).or_default() += 1;
                    }
                }
                for e in &after.entries {
                    if e["kind"] == "compensated" {
                        if let Some(k) = e["detail"]["key"].as_str() {
                            assert_ne!(
                                st.outcome.get(k).map(String::as_str),
                                Some("failed"),
                                "{tag}: {k} refutada e compensada"
                            );
                        }
                    }
                    if e["kind"] == "saga_unlog" && e["seq"].as_u64().unwrap_or(0) as usize >= crashed.entries.len() {
                        *seen.entry("refutada fora do registro na recuperação".into()).or_default() += 1;
                    }
                }
            }
        }
    }
    eprintln!("M-III: {n} programas · {seen:?}");
    for k in
        ["queda em intent", "queda em sent", "queda em confirmed", "queda em closed", "saga interrompida encerrada"]
    {
        assert!(seen.get(k).copied().unwrap_or(0) >= 10, "cobertura fraca de `{k}`: {seen:?}");
    }
    assert!(seen.keys().any(|k| k.contains("opaque → escalated")), "{seen:?}");
    assert!(seen.keys().any(|k| k.contains("idempotent → done")), "{seen:?}");
    assert!(seen.keys().any(|k| k.contains("queryable → failed")), "{seen:?}");
}

/// **M-III, ARX × Python na recuperação.** Os mesmos programas e os mesmos pontos de queda no
/// runtime Python emitido: o WAL depois da recuperação é o mesmo, byte a byte (as intenções, os
/// desfechos com evidência, as escaladas, as compensações das sagas interrompidas, a cadeia).
/// `ATH_WAL_PY_N` muda o número de sementes (padrão 40).
#[test]
fn wal_recovery_is_the_same_in_arx_and_python() {
    use arx::wal::CrashPoint;
    let n: u64 = std::env::var("ATH_WAL_PY_N").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
    let dir = scratch();
    let mut compared = 0;
    for seed in 0..n {
        let p = lambda_ath::generate_keyed(seed);
        let src = p.to_ath();
        let c = compile(&src).unwrap();
        let py = ath_emit::python(&c.module, &c.check, &c.codex, "wal.ath").expect("emissão");
        let (pp, w, m) = (
            dir.join(format!("wal{seed}.py")),
            dir.join(format!("wal{seed}.world.json")),
            dir.join(format!("wal{seed}.models.json")),
        );
        std::fs::write(&pp, py).unwrap();
        std::fs::write(&w, p.world().to_string()).unwrap();
        std::fs::write(&m, p.models().to_string()).unwrap();
        for at in [CrashPoint::Intent, CrashPoint::Sent, CrashPoint::Confirmed, CrashPoint::Closed] {
            for nth in 1..=2 {
                let tag = format!("wal-py/{seed}/{}/{nth}", at.name());
                let out = dir.join(format!("wal{seed}-{}-{nth}.jsonl", at.name()));
                let _ = std::fs::remove_file(&out);
                let r = Command::new("python3")
                    .env("PYTHONPATH", root().join("runtimes/python"))
                    .args(["-m", "aletheia_runtime", "crash"])
                    .arg(&pp)
                    .arg("--world")
                    .arg(&w)
                    .arg("--models")
                    .arg(&m)
                    .arg("--at")
                    .arg(format!("{}:{nth}", at.name()))
                    .arg("--wal")
                    .arg(&out)
                    .args(["--agent", "A", "--on", "go"])
                    .output()
                    .unwrap();
                let rust = crash_and_recover(&c, &p, at, nth);
                match (r.status.code(), rust) {
                    (Some(4), None) => {}
                    (Some(0), Some((_, after, _, _))) => {
                        let pyw = std::fs::read_to_string(&out).unwrap();
                        first_diff(&tag, "WAL recuperado ARX × Python", &after.to_jsonl(), &pyw);
                        compared += 1;
                    }
                    (code, rust) => panic!(
                        "{tag}: desfecho difere (Python {code:?}, ARX {})\n{}",
                        if rust.is_some() { "caiu" } else { "terminou" },
                        String::from_utf8_lossy(&r.stderr)
                    ),
                }
            }
        }
    }
    eprintln!("M-III: {compared} WALs recuperados comparados ARX × Python");
    assert!(compared as u64 >= n, "poucas quedas comparadas: {compared}");
}

// ------------------------------------------------------------------ M-I + M-II: obrigações levantadas e intenção

/// O programa de `lifted_obligations_*`: o plano do modelo passa por uma política que aceita
/// tudo; o contrato de `pay` liga operação, destinatário e valor (`s.binds`). Com `amplify`, o
/// valor do efeito não é o campo endossado (é outro valor, como no `amplified` de
/// `Contracts.lean`, onde o endossado é somado a uma constante).
fn lifted_src(amplify: bool) -> String {
    let arg = if amplify { "\"1000.00\"" } else { "amount" };
    format!(
        r#"module lifted

type Write =
  | Pay {{ to: Text[..8], amount: Text[..16] }}

type Plan =
  | Plan {{ writes: List[Write, 0..4], answer: Text[..64] }}

model planner {{
  id = "m"
  provider = "groq"
  version = "1"
  price_in = 0.0
  price_out = 0.0
  ctx = 8192
  max_out = 200
  ttft = 1.0
  per_tok = 0.0
}}

policy Any(p: Plan) -> Bool = true

tool pay(c: Cap[Bank.Transfer], to: Text[..8] ^ Endorsed, amount: Text[..17] ^ Endorsed) -> Text
  ! {{ Bank.Transfer }} irrevocable latency 1.0
  requires s.binds("intents", "pay", to, amount)

agent A(bank: Cap[Bank.Transfer]) budget [tok <= 100000, usd <= 1.0, wall <= 1000.0] {{
  on go(n: Int) -> Text ! {{ Infer, Bank.Transfer }} {{
    let d = infer planner -> Plan {{ task = "pay what the user asked" }}
    let plan = refine d by Any retry 2 else {{ Plan {{ writes: [], answer: "nothing" }} }}
    match plan {{
      Plan {{ writes, answer }} => {{
        for w in writes {{
          match w {{
            Pay {{ to, amount }} => commit pay(bank, to, {arg}),
          }}
        }}
        answer
      }},
    }}
  }}
}}
"#
    )
}

/// O ARX e o Python sobre o mesmo programa: o journal completo e o relatório de erro.
fn run_both(tag: &str, src: &str, world: &Json, models: &Json) -> (String, Option<String>, String, Option<String>) {
    run_both_with(tag, src, world, models, None)
}

/// Como `run_both`, com as concessões da execução `run` (`s.authorizes`).
fn run_both_with(
    tag: &str,
    src: &str,
    world: &Json,
    models: &Json,
    grants: Option<(&str, &Json)>,
) -> (String, Option<String>, String, Option<String>) {
    run_both_full(tag, src, world, models, grants, None)
}

/// Como `run_both_with`, com a assinatura dos argumentos do evento (`arx::sign`).
fn run_both_full(
    tag: &str,
    src: &str,
    world: &Json,
    models: &Json,
    grants: Option<(&str, &Json)>,
    args_sig: Option<&str>,
) -> (String, Option<String>, String, Option<String>) {
    let c = compile(src).unwrap_or_else(|d| {
        panic!("{tag}: o athc recusou\n{:?}", d.iter().map(|x| (&x.code, &x.msg)).collect::<Vec<_>>())
    });
    let provider = Box::new(Scripted::from_json(models).unwrap());
    let mut arx = Arx::new(&c.module, Box::new(SimWorld::from_json(world).unwrap()), provider);
    arx.contracts = athc::contract_hashes(&c);
    if let Some((run, g)) = grants {
        arx.run_id = run.to_string();
        arx.grants = arx::grant::GrantStore::from_json(g).unwrap();
    }
    if let Some(s) = args_sig {
        arx.args_attestation = Some((vec!["0".to_string()], s.to_string()));
    }
    let aerr = arx.run("A", "go", vec![Value::Int(0)]).err().map(|e| e.code);
    let full = arx.journal.to_jsonl();
    let dir = scratch();
    let file = tag.replace(|ch: char| !ch.is_ascii_alphanumeric(), "_");
    let py = ath_emit::python(&c.module, &c.check, &c.codex, &format!("{file}.ath")).expect("emissão");
    let (pp, w, m, j, r) = (
        dir.join(format!("{file}.py")),
        dir.join(format!("{file}.world.json")),
        dir.join(format!("{file}.models.json")),
        dir.join(format!("{file}.journal.jsonl")),
        dir.join(format!("{file}.report.json")),
    );
    std::fs::write(&pp, py).unwrap();
    std::fs::write(&w, world.to_string()).unwrap();
    std::fs::write(&m, models.to_string()).unwrap();
    let mut cmd = Command::new("python3");
    if let Some((run, g)) = grants {
        let gp = dir.join(format!("{file}.grants.json"));
        std::fs::write(&gp, g.to_string()).unwrap();
        cmd.arg("-m").arg("aletheia_runtime").arg("run").arg(&pp).arg("--grants").arg(&gp).args(["--run-id", run]);
    } else {
        cmd.args(["-m", "aletheia_runtime", "run"]).arg(&pp);
    }
    if let Some(s) = args_sig {
        cmd.args(["--args-sig", s]);
    }
    let out = cmd
        .env("PYTHONPATH", root().join("runtimes/python"))
        .arg("--world")
        .arg(&w)
        .arg("--models")
        .arg(&m)
        .args(["--provider", "scripted", "--agent", "A", "--on", "go", "--journal"])
        .arg(&j)
        .arg("--report")
        .arg(&r)
        .args(["--arg-json", "0"])
        .output()
        .expect("python3");
    let report: Json = serde_json::from_str(
        &std::fs::read_to_string(&r)
            .unwrap_or_else(|_| panic!("{tag}: sem relatório\n{}", String::from_utf8_lossy(&out.stderr))),
    )
    .unwrap();
    (full, aerr, std::fs::read_to_string(&j).unwrap(), report["error"]["code"].as_str().map(String::from))
}

fn kinds(jsonl: &str) -> Vec<String> {
    jsonl.lines().map(|l| serde_json::from_str::<Json>(l).unwrap()["kind"].as_str().unwrap().to_string()).collect()
}

/// **M-I + M-II, nos dois runtimes.** O planejador propõe primeiro um pagamento a um
/// beneficiário legítimo com o valor errado (o R3: dentro de qualquer teto), depois o pagamento
/// que a intenção autoriza. A análise descarrega o contrato no efeito e o levanta para o
/// `refine`: a primeira proposta é recusada ali (`obligation_reject`), a segunda executa, e o
/// journal registra a linha da relação que a autorizou (`authorized`, com o BLAKE3 dela). Com o
/// valor transformado depois do endosso, a obrigação não é demonstrável: a
/// análise reporta a lacuna, o `refine` aceita, e o monitor do efeito recusa (`R0630`). Os
/// journals do ARX e do Python são iguais byte a byte nos dois casos.
#[test]
fn lifted_obligations_refuse_at_refine_and_bind_the_intent() {
    let world = json!({ "sinks": ["pay"], "inventories": { "intents": ["pay|B|10.00"] } });
    let plan = |to: &str, amount: &str| json!({ "kind": "Plan", "writes": [{ "kind": "Pay", "to": to, "amount": amount }], "answer": "ok" });
    let models = json!({ "planner": [plan("B", "2000.00"), plan("B", "10.0")] });
    // com a obrigação demonstrada
    let src = lifted_src(false);
    let c = compile(&src).unwrap();
    let ob = ath_syntax::obligations::analyze(&c.module);
    assert_eq!(ob.sites.len(), 1);
    assert!(matches!(ob.sites[0].status, ath_syntax::obligations::Status::Discharged { .. }), "{:?}", ob.sites);
    let (a, aerr, p, perr) = run_both("lifted/demonstrada", &src, &world, &models);
    assert_eq!((aerr.clone(), perr), (None, None), "{a}");
    first_diff("lifted/demonstrada", "ARX × Python (journal completo)", &a, &p);
    let k = kinds(&a);
    let rej = k.iter().position(|x| x == "obligation_reject").expect("a primeira proposta é recusada no refine");
    let tool = k.iter().position(|x| x == "tool").expect("a segunda executa");
    assert!(rej < tool && k.iter().filter(|x| *x == "tool").count() == 1, "{k:?}");
    let auth: Json =
        a.lines().map(|l| serde_json::from_str::<Json>(l).unwrap()).find(|e| e["kind"] == "authorized").unwrap();
    assert_eq!(auth["detail"]["by"][0]["row"], "pay|B|10.00");
    assert_eq!(auth["detail"]["by"][0]["id"], blake3::hash(b"pay|B|10.00").to_hex().to_string());
    // com o valor transformado: lacuna dinâmica, recusa no efeito
    let src = lifted_src(true);
    let c = compile(&src).unwrap();
    let ob = ath_syntax::obligations::analyze(&c.module);
    assert!(
        matches!(&ob.sites[0].status, ath_syntax::obligations::Status::Dynamic { reason } if reason.contains("amount")),
        "{:?}",
        ob.sites
    );
    assert!(ob.lifted.is_empty());
    let models = json!({ "planner": [plan("B", "1.00")] });
    let (a, aerr, p, perr) = run_both("lifted/lacuna", &src, &world, &models);
    assert_eq!((aerr.as_deref(), perr.as_deref()), (Some("R0630"), Some("R0630")), "{a}");
    first_diff("lifted/lacuna", "ARX × Python (journal completo)", &a, &p);
    assert!(!kinds(&a).iter().any(|x| x == "tool" || x == "obligation_reject"));
}

// ------------------------------------------------------------------ M-V: os dois registros

/// **M-V, semântica × evidência.** Para cada programa gerado (sagas, chaves, os três provedores),
/// o mesmo programa roda no ARX com as latências do mundo simulado e com latências diferentes, e
/// no Python. O registro **semântico** (a cabeça da cadeia da vista do núcleo, sem tempo) é o
/// mesmo nos três: é o que o programa fez. O registro de **evidências** (a cabeça do journal
/// integral) é o mesmo entre ARX e Python sob o mesmo relógio simulado e muda quando as latências
/// mudam: é como e quando, com a cadeia de custódia das medições. Nenhum dos dois substitui o
/// outro. `ATH_DIGEST_N` muda o número de sementes (padrão 80).
#[test]
fn digests_separate_semantics_from_evidence() {
    let n: u64 = std::env::var("ATH_DIGEST_N").ok().and_then(|v| v.parse().ok()).unwrap_or(80);
    let dir = scratch();
    let mut moved = 0;
    for seed in 0..n {
        let p = if seed % 2 == 0 { lambda_ath::generate_saga(seed) } else { lambda_ath::generate_keyed(seed) };
        // a latência declarada com folga (5 s): as medições mudam sem que nenhuma passe do
        // contrato Δ — uma latência acima do declarado é semântica (`R0701`), não ruído
        let src = p.to_ath().replace("latency 0.0", "latency 5.0");
        let c = compile(&src).unwrap();
        let comp = athc::compensable_tools(&c.module);
        let run = |world: &Json| {
            let provider = Box::new(Scripted::from_json(&p.models()).unwrap());
            let mut a = Arx::new(&c.module, Box::new(SimWorld::from_json(world).unwrap()), provider);
            a.contracts = athc::contract_hashes(&c);
            let _ = a.run("A", "go", vec![]);
            let cv = arx::journal::Journal::core_view_with(&a.journal.entries, &comp);
            if std::env::var("ATH_DIGEST_DEBUG").is_ok() {
                eprintln!("{}\n---", cv.to_jsonl());
            }
            (cv.head, a.journal.head.clone())
        };
        let w0 = p.world();
        let mut w1 = w0.clone();
        let tools: serde_json::Map<String, Json> = c
            .module
            .items
            .iter()
            .filter_map(|i| match i {
                ath_syntax::ast::Item::Tool(t) => Some((t.name.name.clone(), json!(1.75))),
                _ => None,
            })
            .collect();
        w1["latency"] = Json::Object(tools);
        let (s0, e0) = run(&w0);
        let (s1, e1) = run(&w1);
        assert_eq!(s0, s1, "semente {seed}: o registro semântico mudou com as latências\n{src}");
        if e0 != e1 {
            moved += 1;
        } else if std::env::var("ATH_DIGEST_DEBUG").is_ok() {
            eprintln!("semente {seed}: evidências iguais");
        }
        // Python, com o mundo original
        let py = ath_emit::python(&c.module, &c.check, &c.codex, "d.ath").expect("emissão");
        let (pp, w, m, r) = (
            dir.join(format!("dg{seed}.py")),
            dir.join(format!("dg{seed}.world.json")),
            dir.join(format!("dg{seed}.models.json")),
            dir.join(format!("dg{seed}.report.json")),
        );
        std::fs::write(&pp, py).unwrap();
        std::fs::write(&w, w0.to_string()).unwrap();
        std::fs::write(&m, p.models().to_string()).unwrap();
        let out = Command::new("python3")
            .env("PYTHONPATH", root().join("runtimes/python"))
            .args(["-m", "aletheia_runtime", "run"])
            .arg(&pp)
            .arg("--world")
            .arg(&w)
            .arg("--models")
            .arg(&m)
            .args(["--provider", "scripted", "--agent", "A", "--on", "go", "--report"])
            .arg(&r)
            .output()
            .unwrap();
        let rep: Json = serde_json::from_str(
            &std::fs::read_to_string(&r)
                .unwrap_or_else(|_| panic!("semente {seed}: sem relatório\n{}", String::from_utf8_lossy(&out.stderr))),
        )
        .unwrap();
        assert_eq!(rep["digests"]["semantic"].as_str(), Some(s0.as_str()), "semente {seed}: semântico ARX × Python");
        assert_eq!(rep["digests"]["evidence"].as_str(), Some(e0.as_str()), "semente {seed}: evidências ARX × Python");
    }
    eprintln!("M-V: {n} programas · o registro de evidências mudou com as latências em {moved}");
    assert!(moved * 2 >= n as usize, "as latências quase nunca aparecem nas evidências: {moved}/{n}");
}

// ------------------------------------------------------------------ v1.0-RC1: concessões lineares

/// O programa das concessões: `send` e `schedule` exigem uma concessão da tupla inteira.
fn grants_src() -> String {
    r#"module grants

type Write =
  | Send { to: Text[..8], amount: Text[..16] }
  | Schedule { to: Text[..8], amount: Text[..16] }

type Plan =
  | Plan { writes: List[Write, 0..4], answer: Text[..64] }

model planner {
  id = "m"
  provider = "groq"
  version = "1"
  price_in = 0.0
  price_out = 0.0
  ctx = 8192
  max_out = 300
  ttft = 1.0
  per_tok = 0.0
}

policy Any(p: Plan) -> Bool = true

tool send(c: Cap[Bank.Transfer], to: Text[..8] ^ Endorsed, amount: Text[..16] ^ Endorsed) -> Text
  ! { Bank.Transfer } irrevocable latency 1.0
  requires s.authorizes("send", to, amount)

tool schedule(c: Cap[Bank.Transfer], to: Text[..8] ^ Endorsed, amount: Text[..16] ^ Endorsed) -> Text
  ! { Bank.Transfer } irrevocable latency 1.0
  requires s.authorizes("schedule", to, amount)

agent A(bank: Cap[Bank.Transfer]) budget [tok <= 100000, usd <= 1.0, wall <= 1000.0] {
  on go(n: Int) -> Text ! { Infer, Bank.Transfer } {
    let d = infer planner -> Plan { task = "do what the user confirmed" }
    let plan = refine d by Any retry 1 else { Plan { writes: [], answer: "nothing" } }
    match plan {
      Plan { writes, answer } => {
        for w in writes {
          match w {
            Send { to, amount } => commit send(bank, to, amount),
            Schedule { to, amount } => commit schedule(bank, to, amount),
          }
        }
        answer
      },
    }
  }
}
"#
    .to_string()
}

/// **A. Sem produto cartesiano, sem repetição.** O principal confirmou duas operações:
/// `send 100.00 → A` e `schedule 250.00 → B`. A relação da Rev13 (`intents_of`: todo IBAN × todo
/// valor × as duas operações) autorizava as oito combinações; as concessões autorizam as duas
/// confirmadas, uma vez cada. Cada tentativa adversarial é recusada **no `refine`** (a obrigação
/// é levantada) e nenhuma ferramenta executa; o plano legítimo executa as duas e consome as duas
/// concessões; repetir uma operação confirmada pede uma segunda concessão. Os journals do ARX e
/// do Python são iguais byte a byte em todos os casos.
#[test]
fn grants_are_whole_tuples_bound_to_the_run_and_spent_once() {
    let src = grants_src();
    let c = compile(&src).unwrap();
    let ob = ath_syntax::obligations::analyze(&c.module);
    assert_eq!(ob.sites.len(), 2);
    assert!(
        ob.sites.iter().all(|s| matches!(s.status, ath_syntax::obligations::Status::Discharged { .. })),
        "{:?}",
        ob.sites
    );
    let world = json!({ "sinks": ["send", "schedule"] });
    let g = |op: &str, to: &str, amount: &str, nonce: u64| json!({ "op": op, "args": [to, amount], "origin": "confirmation:teste", "run": "run-1", "nonce": nonce });
    let grants = json!({ "grants": [g("send", "A", "100.00", 0), g("schedule", "B", "250.00", 0)] });
    let w = |kind: &str, to: &str, amount: &str| json!({ "kind": kind, "to": to, "amount": amount });
    let plan = |ws: Vec<Json>| json!({ "planner": [{ "kind": "Plan", "writes": ws, "answer": "ok" }] });
    let tools = |j: &str| kinds(j).iter().filter(|k| *k == "tool").count();

    // o plano legítimo: as duas operações confirmadas, cada uma com a sua concessão no journal
    let (a, aerr, p, perr) = run_both_with(
        "grants/legitimo",
        &src,
        &world,
        &plan(vec![w("Send", "A", "100.0"), w("Schedule", "B", "250.00")]),
        Some(("run-1", &grants)),
    );
    assert_eq!((aerr, perr), (None, None), "{a}");
    first_diff("grants/legitimo", "ARX × Python (journal completo)", &a, &p);
    assert_eq!(tools(&a), 2);
    let auth: Vec<Json> =
        a.lines().map(|l| serde_json::from_str::<Json>(l).unwrap()).filter(|e| e["kind"] == "authorized").collect();
    assert_eq!(auth.len(), 2);
    assert_eq!(auth[0]["detail"]["by"][0]["op"], "send");
    assert_eq!(auth[0]["detail"]["by"][0]["origin"], "confirmation:teste");
    assert_eq!(auth[0]["detail"]["by"][0]["run"], "run-1");
    assert_ne!(auth[0]["detail"]["by"][0]["grant"], auth[1]["detail"]["by"][0]["grant"]);

    // dois beneficiários, dois valores, duas operações: as seis combinações nunca confirmadas
    let attacks = [
        ("Send", "A", "250.00"),
        ("Send", "B", "100.00"),
        ("Send", "B", "250.00"),
        ("Schedule", "A", "100.00"),
        ("Schedule", "A", "250.00"),
        ("Schedule", "B", "100.00"),
    ];
    for (i, (kind, to, amount)) in attacks.iter().enumerate() {
        let tag = format!("grants/cartesiano{i}");
        let (a, aerr, p, perr) =
            run_both_with(&tag, &src, &world, &plan(vec![w(kind, to, amount)]), Some(("run-1", &grants)));
        assert_eq!((aerr, perr), (None, None), "{a}");
        first_diff(&tag, "ARX × Python (journal completo)", &a, &p);
        assert_eq!(tools(&a), 0, "{kind} {to} {amount} executou\n{a}");
        assert!(kinds(&a).iter().any(|k| k == "obligation_reject"), "{a}");
    }

    // a mesma operação confirmada, duas vezes na proposta: uma concessão não paga duas
    let twice = plan(vec![w("Send", "A", "100.00"), w("Send", "A", "100.00")]);
    let (a, _, p, _) = run_both_with("grants/repetida", &src, &world, &twice, Some(("run-1", &grants)));
    first_diff("grants/repetida", "ARX × Python (journal completo)", &a, &p);
    assert_eq!(tools(&a), 0, "{a}");
    // com duas concessões confirmadas (nonce), as duas executam
    let two = json!({ "grants": [g("send", "A", "100.00", 0), g("send", "A", "100.00", 1)] });
    let (a, _, p, _) = run_both_with("grants/duas", &src, &world, &twice, Some(("run-1", &two)));
    first_diff("grants/duas", "ARX × Python (journal completo)", &a, &p);
    assert_eq!(tools(&a), 2, "{a}");

    // a concessão é desta execução: outra execução não a usa
    let (a, _, p, _) = run_both_with(
        "grants/outra-execucao",
        &src,
        &world,
        &plan(vec![w("Send", "A", "100.00")]),
        Some(("run-2", &grants)),
    );
    first_diff("grants/outra-execucao", "ARX × Python (journal completo)", &a, &p);
    assert_eq!(tools(&a), 0, "{a}");

    // sem concessão alguma, nada executa (falha fechado)
    let (a, _, _, _) = run_both("grants/nenhuma", &src, &world, &plan(vec![w("Send", "A", "100.00")]));
    assert_eq!(tools(&a), 0, "{a}");
}

/// A fronteira do efeito decide mesmo quando a checagem no `refine` não cobre o sítio: o mesmo
/// elemento endossado executado duas vezes consome a concessão na primeira e é recusado na
/// segunda com `R0632` (recusa linear), não `R0631` (defeito da análise). E `s.authorizes` fora
/// de `requires` é erro estático (`E0425`): uma política é pura e não consome concessões.
#[test]
fn a_grant_is_consumed_by_the_effect_and_only_exists_in_requires() {
    let src = grants_src().replace(
        "Send { to, amount } => commit send(bank, to, amount),",
        "Send { to, amount } => { commit send(bank, to, amount)\n commit send(bank, to, amount) },",
    );
    let world = json!({ "sinks": ["send", "schedule"] });
    let grants =
        json!({ "grants": [{ "op": "send", "args": ["A", "100.00"], "origin": "confirmation:teste", "run": "r" }] });
    let models = json!({ "planner": [{ "kind": "Plan", "writes": [{ "kind": "Send", "to": "A", "amount": "100.00" }], "answer": "ok" }] });
    let (a, aerr, p, perr) = run_both_with("grants/linear", &src, &world, &models, Some(("r", &grants)));
    assert_eq!((aerr.as_deref(), perr.as_deref()), (Some("R0632"), Some("R0632")), "{a}");
    first_diff("grants/linear", "ARX × Python (journal completo)", &a, &p);
    assert_eq!(kinds(&a).iter().filter(|k| *k == "tool").count(), 1, "{a}");

    let bad = grants_src().replace(
        "policy Any(p: Plan) -> Bool = true",
        "policy Any(p: Plan) -> Bool = s.authorizes(\"send\", \"A\", \"1\")",
    );
    let diags = compile(&bad).err().expect("E0425");
    assert!(diags.iter().any(|d| d.code == "E0425"), "{:?}", diags.iter().map(|d| &d.code).collect::<Vec<_>>());
}

// ------------------------------------------------------------------ v1.0-RC2: concessão e intenção, uma transação

/// Três operações com chave, uma por classe de provedor, cada uma paga por uma concessão.
fn grant_txn_src() -> &'static str {
    r#"module txn

tool send(c: Cap[Bank.Transfer], to: Text[..8], amount: Text[..16], k: own IdemKey) -> Unit
  ! { Bank.Transfer } irrevocable provider idempotent latency 1.0
  requires s.authorizes("send", to, amount)

tool schedule(c: Cap[Bank.Transfer], to: Text[..8], amount: Text[..16], k: own IdemKey) -> Unit
  ! { Bank.Transfer } irrevocable provider queryable latency 1.0
  requires s.authorizes("schedule", to, amount)

tool wire(c: Cap[Bank.Transfer], to: Text[..8], amount: Text[..16], k: own IdemKey) -> Unit
  ! { Bank.Transfer } irrevocable provider opaque latency 1.0
  requires s.authorizes("wire", to, amount)

agent A(bank: Cap[Bank.Transfer], keys: Cap[IdemKey.Mint]) budget [tok <= 1000, usd <= 1.0, wall <= 1000.0] {
  on go() -> Text ! { Bank.Transfer, TimeoutUnknown } {
    commit send(bank, "A", "100.00", keys.mint())
    commit schedule(bank, "B", "250.00", keys.mint())
    commit wire(bank, "C", "300.00", keys.mint())
    "feito"
  }

  handle timeout_unknown(p: own Pending[_]) -> Text {
    reconcile p fuel 3 else {
      transfer p
      "pendente"
    }
  }
}
"#
}

/// A decisão de `Aletheia.Txn.expected` para o ponto de queda, lida do fonte Lean: o modelo
/// formal é o oráculo do teste, e o Lean prova que a tabela é a função `recover`.
fn lean_decision(point: &str) -> String {
    let src = std::fs::read_to_string(root().join("lean/Aletheia/Txn.lean")).unwrap();
    let table = src.split("def expected : List (Point × Decision) :=").nth(1).expect("Txn.lean: `expected`");
    let table = &table[..table.find(']').expect("Txn.lean: fim de `expected`")];
    let pairs: Vec<(String, String)> = table
        .split('(')
        .skip(1)
        .map(|p| {
            let mut it = p.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty());
            (it.next().unwrap().to_string(), it.next().unwrap().to_string())
        })
        .collect();
    assert_eq!(pairs.len(), 4, "Txn.lean: `expected` tem quatro pontos de queda");
    pairs.into_iter().find(|(p, _)| p == point).unwrap_or_else(|| panic!("Txn.lean: ponto `{point}`")).1
}

/// **RC2, Lean × ARX × Python na fronteira da autorização.** O mesmo programa, as mesmas concessões e
/// os mesmos pontos de queda (incluídos os dois novos: `authorized`, antes de qualquer
/// persistência, e `logged`, entre a intenção e o consumo) nos dois runtimes: o WAL depois da
/// recuperação é o mesmo, byte a byte. E as decisões são as de `lean/Aletheia/Txn.lean`:
/// intenção sem consumo fecha como não enviada (`recovery:unspent`), para as três classes de
/// provedor; intenção com consumo segue o protocolo da classe.
#[test]
fn the_grant_transaction_is_the_same_in_arx_and_python() {
    use arx::wal::{CrashPoint, WalState};
    quiet_crashes();
    let dir = scratch();
    let c = compile(grant_txn_src()).unwrap();
    let py = ath_emit::python(&c.module, &c.check, &c.codex, "txn.ath").expect("emissão");
    let world = json!({ "sinks": ["send", "schedule", "wire"] });
    let g = |op: &str, to: &str, amount: &str| json!({ "op": op, "args": [to, amount], "origin": "confirmation:t", "run": "r1", "nonce": 0 });
    let grants =
        json!({ "grants": [g("send", "A", "100.00"), g("schedule", "B", "250.00"), g("wire", "C", "300.00")] });
    let (pp, wp, gp) = (dir.join("txn.py"), dir.join("txn.world.json"), dir.join("txn.grants.json"));
    std::fs::write(&pp, py).unwrap();
    std::fs::write(&wp, world.to_string()).unwrap();
    std::fs::write(&gp, grants.to_string()).unwrap();
    let mut compared = 0;
    for at in [CrashPoint::Authorized, CrashPoint::Logged, CrashPoint::Intent, CrashPoint::Sent] {
        for nth in 1..=3u32 {
            let tag = format!("txn/{}/{nth}", at.name());
            // ARX: a queda, e o reinício com o WAL e o registro de consumo que sobraram
            let mut a =
                Arx::new(&c.module, Box::new(SimWorld::from_json(&world).unwrap()), Box::new(Scripted::default()));
            a.run_id = "r1".into();
            a.grants = arx::grant::GrantStore::from_json(&grants).unwrap();
            let mut w = arx::wal::Wal::memory();
            w.crash = Some((at, nth));
            a.wal = Some(w);
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = a.run("A", "go", vec![]);
            }));
            assert!(r.is_err(), "{tag}: o programa terminou antes da queda");
            let host = std::mem::replace(&mut a.host, Box::new(SimWorld::from_json(&json!({})).unwrap()));
            let crashed = arx::journal::Journal::from_entries(a.wal.take().unwrap().log.entries);
            // o invariante: todo consumo cita uma intenção que está no WAL
            let hashes: Vec<&str> =
                crashed.entries.iter().filter(|e| e["kind"] == "intent").filter_map(|e| e["hash"].as_str()).collect();
            for (id, o) in &a.grants.owners {
                assert!(
                    o.intent.as_deref().is_some_and(|h| hashes.contains(&h)),
                    "{tag}: consumo de {id} sem intenção"
                );
            }
            let mut b = Arx::new(&c.module, host, Box::new(Scripted::default()));
            b.run_id = "r1".into();
            b.spent_view = Some(a.grants.owners.clone());
            b.wal = Some(arx::wal::Wal::resume(arx::journal::Journal::from_entries(crashed.entries.clone())));
            b.recover(3);
            let after = arx::journal::Journal::from_entries(b.wal.take().unwrap().log.entries);
            // a decisão sobre a operação interrompida
            let key = format!("k{nth}");
            let st = WalState::of(&after);
            let ev = after
                .entries
                .iter()
                .rev()
                .find(|e| (e["kind"] == "close" || e["kind"] == "escalate") && e["detail"]["key"] == key.as_str())
                .map(|e| e["detail"]["evidence"].as_str().unwrap_or("escalated").to_string());
            // a decisão vem da tabela `expected` de `Txn.lean` (provada igual a `recover`);
            // `byClass` é o protocolo da classe do provedor da operação
            let expect = match (lean_decision(at.name()).as_str(), nth) {
                ("nothing", _) => None,
                ("unsent", _) => Some("recovery:unspent"),
                ("byClass", 1) => Some("recovery:resend"),
                ("byClass", 2) => Some("recovery:status"),
                ("byClass", _) => Some("escalated"),
                (d, _) => panic!("decisão desconhecida em Txn.lean: {d}"),
            };
            assert_eq!(ev.as_deref(), expect, "{tag}");
            assert!(st.open().is_empty(), "{tag}: intenção sem desfecho");
            // Python: o mesmo WAL, byte a byte
            let out = dir.join(format!("txn-{}-{nth}.jsonl", at.name()));
            let _ = std::fs::remove_file(&out);
            let r = Command::new("python3")
                .env("PYTHONPATH", root().join("runtimes/python"))
                .args(["-m", "aletheia_runtime", "crash"])
                .arg(&pp)
                .arg("--world")
                .arg(&wp)
                .arg("--grants")
                .arg(&gp)
                .args(["--run-id", "r1", "--at", &format!("{}:{nth}", at.name()), "--wal"])
                .arg(&out)
                .args(["--agent", "A", "--on", "go"])
                .output()
                .unwrap();
            assert_eq!(r.status.code(), Some(0), "{tag}: {}", String::from_utf8_lossy(&r.stderr));
            first_diff(&tag, "WAL recuperado ARX × Python", &after.to_jsonl(), &std::fs::read_to_string(&out).unwrap());
            compared += 1;
        }
    }
    assert_eq!(compared, 12);
}

/// **RC2, autenticidade, ARX × Python.** O programa das concessões com uma autoridade declarada
/// e um argumento `Trusted`. Com tudo assinado pela autoridade, o plano confirmado executa e o
/// journal registra quem atestou; uma concessão sem assinatura (ou assinada por outra chave)
/// não autoriza nada, mesmo bem formada e com origem `confirmation:`; sem a assinatura dos
/// argumentos (ou com a de outra execução) o tratador nem começa (`R0636`); e `s.member` sobre
/// uma fotografia que ninguém assinou é recusado (`R0635`). Os journals são iguais byte a byte.
#[test]
fn only_what_the_declared_authority_signed_confers_authority() {
    // RFC 8032, §7.1, vetor de teste 1: um par público
    let sk = arx::sign::parse_secret("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60").unwrap();
    let other = arx::sign::parse_secret(&"42".repeat(32)).unwrap();
    let src = grants_src()
        .replace(
            "policy Any",
            &format!("authority principal {{ key = \"{}\" }}\n\npolicy Any", arx::sign::public_of(&sk)),
        )
        .replace("on go(n: Int)", "on go(n: Int ^ Trusted)");
    assert!(src.contains("authority principal") && src.contains("^ Trusted"));
    let world = json!({ "sinks": ["send", "schedule"] });
    let g = |op: &str, to: &str, amount: &str, key: Option<&[u8; 32]>| {
        let args = [to.to_string(), amount.to_string()];
        let mut x = arx::grant::Grant::new("run-1", op, &args, "confirmation:msg-17", 0);
        x.sig = key.map(|k| arx::sign::sign(k, "grant", &x.id));
        x.to_json()
    };
    let w = |kind: &str, to: &str, amount: &str| json!({ "kind": kind, "to": to, "amount": amount });
    let plan = |writes: Vec<Json>| json!({ "planner": [{ "kind": "Plan", "writes": writes, "answer": "ok" }] });
    let both = plan(vec![w("Send", "A", "100.00"), w("Schedule", "B", "250.00")]);
    let args =
        |run: &str, key: &[u8; 32]| arx::sign::sign(key, "args", &arx::sign::args_payload(run, &["0".to_string()]));
    let ok_args = args("run-1", &sk);
    let executed = |j: &str| j.lines().filter(|l| l.contains("\"kind\":\"tool\"")).count();

    // tudo assinado pela autoridade: executa, e o journal diz quem atestou
    let signed = json!({ "grants": [g("send", "A", "100.00", Some(&sk)), g("schedule", "B", "250.00", Some(&sk))] });
    let (a, ae, p, pe) = run_both_full("auth/ok", &src, &world, &both, Some(("run-1", &signed)), Some(&ok_args));
    first_diff("auth/ok", "ARX × Python (journal completo)", &a, &p);
    assert_eq!((ae, pe), (None, None));
    assert_eq!(executed(&a), 2, "{a}");
    assert!(a.contains("\"what\":\"args\"") && a.contains("\"authority\":\"principal\""), "{a}");

    // uma das duas sem assinatura, ou assinada por outra chave: nada executa
    for (tag, key) in [("auth/sem-assinatura", None), ("auth/outra-chave", Some(&other))] {
        let mixed = json!({ "grants": [g("send", "A", "100.00", Some(&sk)), g("schedule", "B", "250.00", key)] });
        let (a, _, p, _) = run_both_full(tag, &src, &world, &both, Some(("run-1", &mixed)), Some(&ok_args));
        first_diff(tag, "ARX × Python (journal completo)", &a, &p);
        assert_eq!(executed(&a), 0, "{tag}: {a}");
        assert!(a.contains("\"unauthenticated\":[\""), "{tag}: {a}");
    }

    // os argumentos `Trusted`: sem assinatura, de outra execução, de outra chave
    for (tag, sig) in [
        ("auth/args-sem", None),
        ("auth/args-outra-execucao", Some(args("run-2", &sk))),
        ("auth/args-outra-chave", Some(args("run-1", &other))),
    ] {
        let (a, ae, p, pe) = run_both_full(tag, &src, &world, &both, Some(("run-1", &signed)), sig.as_deref());
        first_diff(tag, "ARX × Python (journal completo)", &a, &p);
        assert_eq!((ae.as_deref(), pe.as_deref()), (Some("R0636"), Some("R0636")), "{tag}");
        assert_eq!(executed(&a), 0, "{tag}");
    }

    // a fotografia confiável que ninguém assinou não é consultada
    let reads = src
        .replace("policy Any(p: Plan) -> Bool = true", "policy Any(p: Plan) -> Bool = s.member(\"payees\", \"A\")")
        .replace(
            "task = \"do what the user confirmed\"",
            "task = \"do what the user confirmed\", payees = s.inventory(\"payees\")",
        );
    let world_p = json!({ "sinks": ["send", "schedule"], "inventories": { "payees": ["A"] } });
    let (a, ae, p, pe) = run_both_full("auth/foto", &reads, &world_p, &both, Some(("run-1", &signed)), Some(&ok_args));
    first_diff("auth/foto", "ARX × Python (journal completo)", &a, &p);
    assert_eq!((ae.as_deref(), pe.as_deref()), (Some("R0635"), Some("R0635")));
    assert_eq!(executed(&a), 0);
}
