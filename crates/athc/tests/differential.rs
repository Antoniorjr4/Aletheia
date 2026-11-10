//! Testes diferenciais: o mesmo programa `.ath`, no mesmo mundo e com o mesmo roteiro de
//! modelo, executado pelo ARX (Rust) e pelo código emitido por `athc emit --target py` sobre o
//! `aletheia_runtime`. Os dois backends precisam produzir journals **idênticos byte a byte**
//! (logo, a mesma cabeça de hash), o mesmo resultado ou erro, o mesmo estado final do mundo e o
//! mesmo orçamento gasto. Isso cobre o grafo de eventos (ordem e conteúdo de cada entrada), as
//! decisões de política (`endorse`/`policy_reject`) e os estados de recuperação (sagas,
//! compensações, `TimeoutUnknown`, reconciliação, transferência).
//!
//! No fim do arquivo está o **tripé**: o avaliador formal do Lean (`ath-lean-eval`, o mesmo
//! `eval` dos teoremas T1–T3) entra como terceiro backend. Para programas do núcleo λ_Ath, as
//! três realizações precisam produzir o mesmo journal do núcleo, byte a byte.

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
    arx: String,
    py: String,
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
    if let Err(e) = arx.run("A", "go", args.clone()) {
        panic!("{tag}: o ARX abortou ({} {})\n{src}", e.code, e.msg);
    }
    let arx_core = arx::journal::Journal::core_view(&arx.journal.entries).to_jsonl();
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
    assert!(out.status.success(), "{tag}: o runtime Python falhou\n{}", String::from_utf8_lossy(&out.stderr));
    let py_core = std::fs::read_to_string(&cj).unwrap();
    // Lean: o avaliador canônico
    let lean = lean_eval().map(|bin| {
        let pj = dir.join(format!("{file}.core.json"));
        std::fs::write(&pj, core.to_string()).unwrap();
        let out = Command::new(bin).arg("run").arg(&pj).output().unwrap();
        assert!(
            out.status.success(),
            "{tag}: o avaliador do Lean falhou\n{}\n{src}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    });
    Tripod { lean, arx: arx_core, py: py_core }
}

fn assert_tripod(tag: &str, t: &Tripod) {
    first_diff(tag, "ARX × Python (vista do núcleo)", &t.arx, &t.py);
    if let Some(lean) = &t.lean {
        first_diff(tag, "Lean × ARX (journal do núcleo)", lean, &t.arx);
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
