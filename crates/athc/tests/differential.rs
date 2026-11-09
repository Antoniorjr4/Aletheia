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
    for path in cassettes {
        let tag = path.file_name().unwrap().to_string_lossy().replace(".cassette.json", "");
        let cas = Cassette::load(&path).unwrap();
        let (arx_journal, _) = replay_arx(&cas);
        assert_same(&format!("{tag}: gravação × replay ARX"), &cas.journal, &arx_journal);
        assert_same(
            &format!("{tag}: gravação × replay Python"),
            &cas.journal,
            &replay_py(&cas, &format!("real-{tag}")),
        );
    }
}
