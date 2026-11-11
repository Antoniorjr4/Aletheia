use arx::cassette::{first_divergence, Cassette};
use arx::host::SimWorld;
use arx::journal::Journal;
use arx::model::{Constraint, Masked, ModelProvider, OpenAiCompat, Recorder, Replay, Scripted};
use arx::{Arx, Value};
use ath_check::fmt;
use ath_grammar::Dialect;
use ath_syntax::ast::{Item, TypeExpr};
use ath_syntax::parse_module;
use athc::{compile, render, summary, Compiled, Style};
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

const USAGE: &str = "athc — compilador de Aletheia Core 0 (Marco Zero)

uso:
  athc check <arquivo.ath>
      verifica os estratos E0, E1, E2, E3 e E6 e endereça o Codex
  athc run <arquivo.ath> [--agent A] [--on evento] [--arg literal]...
                         [--world mundo.json] [--models modelos.json] [--journal saída.jsonl]
                         [--provider masked|scripted|openai] [--report relatório.json]
                         [--core-journal núcleo.jsonl]
      verifica e executa um handler no runtime ARX
      (sem --world/--models, usa fixtures/world.json e fixtures/models.json ao lado do arquivo)
      provedores:
        masked    (padrão) roteiro como proposta + máscara derivada do tipo: saída válida por construção
        scripted  roteiro literal, sem restrição; o ARX valida e rejeita saídas fora do tipo
        openai    API compatível com OpenAI (OpenAI, Groq, vLLM, llama.cpp…), configurada pelo ambiente:
                  OPENAI_API_KEY, OPENAI_BASE_URL (ex.: https://api.groq.com/openai/v1), ALETHEIA_MODEL
                  ou pelos argumentos --model ID --base-url URL
                  [--constraint json-schema|gbnf|none] [--schema core|full]
  athc grammar <arquivo.ath> --type T [--format all|schema|gbnf|shape] [--decode 'proposta']
      mostra a gramática derivada do tipo T e, com --decode, a decodificação mascarada de uma proposta
  athc hash <arquivo.ath> [--codex dir]
      grava as formas canônicas no Codex (padrão: .codex) e lista os hashes
  athc verify-journal <journal.jsonl>
      recalcula a cadeia de hashes do journal
  athc contract-report <journal.jsonl> [--journal outro.jsonl]...
      aderência ao contrato Δ medida nos journals: latência contra o contrato, overhead de entrada
      observado, raciocínio, violações; sugere o Δ recalibrado pela cauda observada
  athc run … --provider openai --record cassete.json
      grava cada requisição e cada resposta HTTP crua do modelo real, com o programa, o mundo,
      os argumentos e o journal da execução
  athc run … --replay cassete.json
      reproduz um cassete: as requisições precisam ser idênticas às gravadas
  athc replay <cassete.json> [--journal saída.jsonl] [--emit-py programa.py]
      reexecuta o programa embutido no cassete contra as respostas gravadas e exige o mesmo
      journal, byte a byte
  athc emit <arquivo.ath> --target py [--out programa.py]
      emite o programa para Python sobre o aletheia_runtime (segunda realização da semântica do ARX)";

struct Opts {
    file: Option<String>,
    flags: HashMap<String, Vec<String>>,
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut file = None;
    let mut flags: HashMap<String, Vec<String>> = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(name) = a.strip_prefix("--") {
            let v = args.get(i + 1).ok_or_else(|| format!("`--{name}` exige um valor"))?;
            flags.entry(name.to_string()).or_default().push(v.clone());
            i += 2;
        } else {
            if file.is_some() {
                return Err(format!("argumento inesperado `{a}`"));
            }
            file = Some(a.clone());
            i += 1;
        }
    }
    Ok(Opts { file, flags })
}

impl Opts {
    fn one(&self, k: &str) -> Option<&str> {
        self.flags.get(k).and_then(|v| v.last()).map(String::as_str)
    }
}

fn main() -> ExitCode {
    // Saída redirecionada para quem fecha o pipe cedo (`athc … | head`): encerra sem pânico.
    std::panic::set_hook(Box::new(|info| {
        let msg = info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| info.payload().downcast_ref::<&str>().copied())
            .unwrap_or("");
        if msg.contains("Broken pipe") {
            std::process::exit(0);
        }
        eprintln!("{info}");
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let st = Style { color: std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() };
    let Some(cmd) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let opts = match parse_opts(&args[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("erro: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let r = match cmd.as_str() {
        "check" => cmd_check(&opts, st),
        "run" => cmd_run(&opts, st),
        "hash" => cmd_hash(&opts, st),
        "grammar" => cmd_grammar(&opts, st),
        "verify-journal" => cmd_verify(&opts, st),
        "contract-report" => cmd_contract_report(&opts, st),
        "obligations" => cmd_obligations(&opts, st),
        "emit" => cmd_emit(&opts, st),
        "replay" => cmd_replay(&opts, st),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(true)
        }
        other => Err(format!("comando desconhecido `{other}`\n\n{USAGE}")),
    };
    match r {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("erro: {e}");
            ExitCode::from(2)
        }
    }
}

fn load(opts: &Opts) -> Result<(String, String), String> {
    let path = opts.file.clone().ok_or("informe o arquivo .ath")?;
    let src = std::fs::read_to_string(&path).map_err(|e| format!("não foi possível ler `{path}`: {e}"))?;
    Ok((path, src))
}

/// Compila e imprime os diagnósticos; `None` se o programa foi rejeitado.
fn compile_or_report(path: &str, src: &str, st: Style) -> Option<Compiled> {
    match compile(src) {
        Ok(c) => Some(c),
        Err(diags) => {
            for d in &diags {
                println!("{}", render(src, path, d, st));
            }
            let stratum = diags[0].stratum;
            let tail =
                if stratum < ath_check::Stratum::E6 { "; os estratos seguintes não foram avaliados" } else { "" };
            println!("{}: {} diagnóstico(s) no estrato {}{}", st.red("rejeitado"), diags.len(), stratum.name(), tail);
            None
        }
    }
}

fn cmd_check(opts: &Opts, st: Style) -> Result<bool, String> {
    let (path, src) = load(opts)?;
    let Some(c) = compile_or_report(&path, &src, st) else { return Ok(false) };
    println!("$ athc check {path}");
    print!("{}", summary(&c, st));
    Ok(true)
}

/// `athc obligations prog.ath [--json]`: o destino de cada contrato `requires` (Rev13, M-I) —
/// descarregado estaticamente (levantado para um `refine`) ou lacuna dinâmica, com o motivo.
fn cmd_obligations(opts: &Opts, st: Style) -> Result<bool, String> {
    let (path, src) = load(opts)?;
    let Some(c) = compile_or_report(&path, &src, st) else { return Ok(false) };
    let ob = ath_syntax::obligations::analyze(&c.module);
    let line = |sp: ath_syntax::Span| src[..sp.lo as usize].matches('\n').count() + 1;
    let mut rows = Vec::new();
    for s in &ob.sites {
        let (status, why) = match &s.status {
            ath_syntax::obligations::Status::Discharged { refines } => (
                "descarregada",
                format!(
                    "levantada para o refine em {}",
                    refines.iter().map(|r| format!("L{}", line(*r))).collect::<Vec<_>>().join(", ")
                ),
            ),
            ath_syntax::obligations::Status::Dynamic { reason } => ("dinâmica", reason.clone()),
        };
        rows.push(serde_json::json!({ "tool": s.tool, "line": line(s.call), "status": status, "reason": why }));
    }
    let lifted: Vec<serde_json::Value> = ob
        .lifted
        .iter()
        .flat_map(|((lo, _), ls)| {
            let l = src[..*lo as usize].matches('\n').count() + 1;
            ls.iter().map(move |x| {
                serde_json::json!({ "refine_line": l, "tool": x.tool, "variant": x.variant,
                "path": x.path.iter().map(|p| match p { ath_syntax::obligations::Step::Elem => "[*]".to_string(),
                    ath_syntax::obligations::Step::Field(v, f) => format!("{v}.{f}") }).collect::<Vec<_>>(),
                "binds": x.binds })
            })
        })
        .collect();
    if opts.one("format") == Some("json") {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "sites": rows, "lifted": lifted })).unwrap());
        return Ok(true);
    }
    println!("$ athc obligations {path}");
    for r in &rows {
        println!(
            "  L{:<4} {:<24} {:<13} {}",
            r["line"],
            r["tool"].as_str().unwrap_or(""),
            r["status"].as_str().unwrap_or(""),
            r["reason"].as_str().unwrap_or("")
        );
    }
    for l in &lifted {
        println!(
            "  refine L{} ⇐ {} ({} em {})",
            l["refine_line"],
            l["tool"].as_str().unwrap_or(""),
            l["variant"].as_str().unwrap_or(""),
            l["path"]
        );
    }
    Ok(true)
}

fn cmd_hash(opts: &Opts, st: Style) -> Result<bool, String> {
    let (path, src) = load(opts)?;
    let Some(c) = compile_or_report(&path, &src, st) else { return Ok(false) };
    let dir = PathBuf::from(opts.one("codex").unwrap_or(".codex"));
    ath_codex::write_store(&c.codex, &dir).map_err(|e| format!("falha ao gravar o Codex: {e}"))?;
    println!("{:<8} {:<18} {:<14} depende de", "tipo", "nome", "hash");
    for e in &c.codex.entries {
        let deps: Vec<String> =
            e.deps.iter().map(|d| format!("{d}#{}", &c.codex.hash_of(d).unwrap_or("?")[..6])).collect();
        println!("{:<8} {:<18} #{:<13} {}", e.kind, e.name, ath_codex::short(&e.hash), deps.join(", "));
    }
    println!("{:<8} {:<18} #{}", "raiz", "-", ath_codex::short(&c.codex.root));
    println!("{}", st.dim(&format!("Codex gravado em {}", dir.display())));
    Ok(true)
}

fn read_json(p: &Path) -> Result<serde_json::Value, String> {
    let s = std::fs::read_to_string(p).map_err(|e| format!("não foi possível ler `{}`: {e}", p.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("JSON inválido em `{}`: {e}", p.display()))
}

fn fixture(opts: &Opts, flag: &str, file: &str, default_name: &str) -> Option<PathBuf> {
    if let Some(p) = opts.one(flag) {
        return Some(PathBuf::from(p));
    }
    let candidate = Path::new(file).parent().unwrap_or(Path::new(".")).join("fixtures").join(default_name);
    candidate.exists().then_some(candidate)
}

fn cmd_run(opts: &Opts, st: Style) -> Result<bool, String> {
    let (path, src) = load(opts)?;
    let Some(c) = compile_or_report(&path, &src, st) else { return Ok(false) };
    println!("{} verificado: E0 E1 E2 E3 E5 E6 · Codex #{}", st.green("✓"), ath_codex::short(&c.codex.root));

    let agent = match opts.one("agent") {
        Some(a) => a.to_string(),
        None => c
            .module
            .items
            .iter()
            .find_map(|i| if let Item::Agent(a) = i { Some(a.name.name.clone()) } else { None })
            .ok_or("o módulo não declara agentes")?,
    };
    let world_json = match fixture(opts, "world", &path, "world.json") {
        Some(p) => read_json(&p)?,
        None => json!({}),
    };
    let world = SimWorld::from_json(&world_json)?;
    let mut tape: Option<(Rc<RefCell<Vec<arx::model::Exchange>>>, serde_json::Value)> = None;
    let provider: Box<dyn ModelProvider> = if let Some(cp) = opts.one("replay") {
        let c = Cassette::load(Path::new(cp))?;
        Box::new(OpenAiCompat::from_config(&c.config, Box::new(Replay::new(c.exchanges)))?)
    } else if opts.one("record").is_some() {
        if opts.one("provider") != Some("openai") {
            return Err("`--record` grava o tráfego de um modelo real: use `--provider openai`".into());
        }
        let mut p = openai_provider(opts)?;
        let t = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut p.transport, Box::new(Replay::new(Vec::new())));
        p.transport = Box::new(Recorder { inner, tape: t.clone() });
        tape = Some((t, p.config_json()));
        Box::new(p)
    } else {
        build_provider(opts, &path)?
    };
    let provider_name = provider.name();
    let mut arx = Arx::new(&c.module, Box::new(world), provider);
    arx.contracts = athc::contract_hashes(&c);
    let args_src: Vec<String> = opts.flags.get("arg").cloned().unwrap_or_default();
    let event = match opts.one("on") {
        Some(e) => e.to_string(),
        None => c
            .module
            .items
            .iter()
            .find_map(|i| match i {
                Item::Agent(a) if a.name.name == agent => a.handlers.first().map(|h| h.event.name.clone()),
                _ => None,
            })
            .ok_or_else(|| format!("agente `{agent}` sem handlers"))?,
    };
    let (_, h) = arx.handler(&agent, &event).ok_or_else(|| format!("handler `{agent}.{event}` não encontrado"))?;
    let raw: Vec<String> = opts.flags.get("arg").cloned().unwrap_or_default();
    if raw.len() != h.params.len() {
        return Err(format!("`{agent}.{event}` recebe {} argumento(s) via --arg", h.params.len()));
    }
    let mut args = Vec::new();
    for (p, r) in h.params.iter().zip(&raw) {
        let ty = p.ty.unlabeled().sig();
        let v = arx
            .literal_from_str(r, &p.ty)
            .ok_or_else(|| format!("`{r}` não é um literal do tipo `{ty}` (texto vai entre aspas: --arg '\"api\"')"))?;
        args.push(v);
    }

    let shown: Vec<String> = args.iter().map(Value::show).collect();
    println!(
        "{} {}.{}({})  {}",
        st.bold("▶"),
        agent,
        event,
        shown.join(", "),
        st.dim(&format!("provedor: {provider_name}"))
    );
    let result = arx.run(&agent, &event, args);
    println!("  {}", st.dim(&format!("{:>7}  {:>7}", "t", "+Δt")));
    for ev in &arx.events {
        println!(
            "  {:>7}  {:>7}  {}  {}",
            fmt::dec(ev.t, 1),
            format!("+{}", fmt::dec(ev.dt, 1)),
            ev.what,
            st.dim(&ev.detail)
        );
        if let Some(n) = &ev.note {
            println!("  {:>7}  {:>7}  {}", "", "", st.dim(&format!("↳ {n}")));
        }
        if let Some((ok, msg)) = &ev.verdict {
            let mark = if *ok { st.green(&format!("✓ {msg}")) } else { st.red(&format!("✗ {msg}")) };
            println!("  {:>7}  {:>7}  {}", "", "", mark);
        }
    }
    let ok = match &result {
        Ok(v) => {
            println!("{} resultado: {}", st.bold("◀"), v.show());
            true
        }
        Err(e) => {
            println!("{} abortado: {} {}", st.red("✗"), e.code, e.msg);
            false
        }
    };
    let m = &arx.meter;
    let lim_tok = m.lim_tok.map(fmt::int).unwrap_or_else(|| "∞".into());
    let lim_usd = m.lim_usd.map(|v| fmt::dec(v, 2)).unwrap_or_else(|| "∞".into());
    let lim_wall = m.lim_wall.map(fmt::secs).unwrap_or_else(|| "∞".into());
    println!();
    println!(
        "  {:<9} {} / {} tok · US$ {} / {} · {} / {}",
        "orçamento",
        fmt::int(m.tok),
        lim_tok,
        fmt::dec(m.usd, 4),
        lim_usd,
        fmt::secs(m.wall),
        lim_wall
    );
    let world = arx.host.summary();
    if !world.is_empty() {
        println!("  {:<9} {}", "mundo", world.join("; "));
    }
    let jpath = PathBuf::from(opts.one("journal").unwrap_or(".arx/journal.jsonl"));
    if let Some(dir) = jpath.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(&jpath, arx.journal.to_jsonl()).map_err(|e| format!("falha ao gravar o journal: {e}"))?;
    let chain = if Journal::verify(&arx.journal.entries).is_ok() { "cadeia íntegra" } else { "cadeia INVÁLIDA" };
    println!(
        "  {:<9} {} · {} entradas · cabeça #{} · {}",
        "journal",
        jpath.display(),
        arx.journal.entries.len(),
        &arx.journal.head[..12],
        chain
    );
    if let Some(cp) = opts.one("core-journal") {
        let core = Journal::core_view_with(&arx.journal.entries, &athc::compensable_tools(&c.module));
        std::fs::write(cp, core.to_jsonl()).map_err(|e| format!("falha ao gravar o journal do núcleo: {e}"))?;
        println!(
            "  {:<9} {cp} · {} entradas · cabeça #{} (vista do núcleo λ_Ath)",
            "núcleo",
            core.entries.len(),
            &core.head[..12]
        );
    }
    if let (Some((t, config)), Some(out)) = (&tape, opts.one("record")) {
        let cassette = Cassette {
            provider: provider_name.clone(),
            config: config.clone(),
            manifest: json!({
                "source_path": path,
                "program": src,
                "codex_root": c.codex.root,
                "agent": agent,
                "event": event,
                "args": arx.journal.entries.first().map(|e| e["detail"]["args"].clone()),
                "args_src": args_src,
                "world": world_json,
            }),
            exchanges: t.borrow().clone(),
            journal: arx.journal.to_jsonl(),
        };
        cassette.save(Path::new(out))?;
        println!("  {:<9} {out} · {} troca(s) HTTP gravada(s)", "cassete", cassette.exchanges.len());
    }
    if let Some(rp) = opts.one("report") {
        let actions: Vec<serde_json::Value> = arx
            .journal
            .entries
            .iter()
            .filter(|e| e["kind"] == "tool")
            .map(|e| json!({ "tool": e["detail"]["tool"], "args": e["detail"]["args"], "commit": e["detail"]["commit"] }))
            .collect();
        let report = json!({
            "ok": ok,
            "result": result.as_ref().ok().map(Value::to_json),
            "error": result.as_ref().err().map(|e| json!({ "code": e.code, "msg": e.msg })),
            "provider": provider_name,
            "world": arx.host.state(),
            "actions": actions,
            "journal": arx.journal.entries.iter().map(|e| e["kind"].clone()).collect::<Vec<_>>(),
            "infers": arx
                .journal
                .entries
                .iter()
                .filter(|e| e["kind"] == "infer")
                .map(|e| json!({
                    "attempt": e["detail"]["attempt"],
                    "output": e["detail"]["output"],
                    "note": e["detail"]["note"],
                    "invalid": e["detail"]["invalid"],
                }))
                .collect::<Vec<_>>(),
            "spent": { "tok": arx.meter.tok, "usd": arx.meter.usd, "wall": arx.meter.wall },
            "journal_head": arx.journal.head,
            // Rev13 (M-V): dois registros separados. O semântico é a cabeça da cadeia da vista do
            // núcleo — determinístico, sem tempo nem latência: o que o programa fez. O de
            // evidências é a cabeça do journal integral — com as medições reais: como e quando.
            "digests": {
                "semantic": arx::journal::Journal::core_view_with(&arx.journal.entries, &athc::compensable_tools(&c.module)).head,
                "evidence": arx.journal.head,
            },
        });
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(rp, text).map_err(|e| format!("falha ao gravar o relatório: {e}"))?;
    }
    Ok(ok)
}

fn build_provider(opts: &Opts, path: &str) -> Result<Box<dyn ModelProvider>, String> {
    let scripted = || -> Result<Scripted, String> {
        match fixture(opts, "models", path, "models.json") {
            Some(p) => Scripted::from_json(&read_json(&p)?),
            None => Ok(Scripted::default()),
        }
    };
    match opts.one("provider").unwrap_or("masked") {
        "masked" => Ok(Box::new(Masked { inner: Box::new(scripted()?) })),
        "scripted" => Ok(Box::new(scripted()?)),
        "openai" => Ok(Box::new(openai_provider(opts)?)),
        other => Err(format!("provedor desconhecido `{other}` (use masked, scripted ou openai)")),
    }
}

fn openai_provider(opts: &Opts) -> Result<OpenAiCompat, String> {
    let model = opts.one("model").or(opts.one("remote-model"));
    let mut p = OpenAiCompat::from_env(model, opts.one("base-url"))?;
    if let Some(c) = opts.one("constraint") {
        p.constraint = Constraint::parse(c).ok_or("--constraint deve ser json-schema, gbnf ou none")?;
    }
    if let Some(d) = opts.one("schema") {
        p.dialect = Dialect::parse(d).ok_or("--schema deve ser core ou full")?;
    }
    if p.api_key.is_none() && !p.base_url.contains("localhost") && !p.base_url.contains("127.0.0.1") {
        return Err(format!("sem chave para {}: defina OPENAI_API_KEY", p.base_url));
    }
    Ok(p)
}

/// Lê uma expressão de tipo (`Action`, `Int[2..64]`, `Text[..80]`) no contexto do módulo.
fn parse_type(src: &str) -> Result<TypeExpr, String> {
    let probe = format!("module probe\ntool probe(x: {src}) -> Unit ! {{}}");
    let m = parse_module(&probe).map_err(|e| format!("tipo inválido `{src}`: {}", e.msg))?;
    match m.items.into_iter().next() {
        Some(Item::Tool(t)) => Ok(t.params.into_iter().next().map(|p| p.ty).ok_or("tipo ausente")?),
        _ => Err(format!("tipo inválido `{src}`")),
    }
}

fn cmd_grammar(opts: &Opts, st: Style) -> Result<bool, String> {
    let (path, src) = load(opts)?;
    let Some(c) = compile_or_report(&path, &src, st) else { return Ok(false) };
    let ty_src = opts.one("type").ok_or("informe --type <Tipo>")?;
    let te = parse_type(ty_src)?;
    let shape = ath_grammar::shape_of(&c.module, &te)?;
    let fmt_sel = opts.one("format").unwrap_or("all");
    let bytes = ath_grammar::max_bytes(&shape);
    let show = |name: &str| fmt_sel == "all" || fmt_sel == name;
    if show("shape") {
        println!("{}", st.bold(&format!("# forma de `{ty_src}`")));
        println!("{shape:#?}");
        match bytes {
            Some(b) => println!("teto da codificação: {b} bytes ⇒ no máximo {b} tokens de saída por amostra"),
            None => println!("sem teto: algum `Text` não tem refinamento `Text[..n]`"),
        }
        println!();
    }
    if show("schema") {
        println!("{}", st.bold("# JSON Schema (response_format, strict)"));
        println!("{}", serde_json::to_string_pretty(&ath_grammar::to_json_schema(&shape)).unwrap_or_default());
        println!();
    }
    if show("gbnf") {
        println!("{}", st.bold("# GBNF (llama.cpp)"));
        print!("{}", ath_grammar::to_gbnf(&shape));
        println!();
    }
    if let Some(p) = opts.one("decode") {
        let limit = bytes.map_or(16_384, |b| b as usize);
        let t = ath_grammar::decode_following(&shape, p, limit)?;
        println!("{}", st.bold("# decodificação mascarada"));
        println!("proposta  {p}");
        println!("saída     {}", t.text);
        match t.deviation {
            None => println!("{} a proposta já pertencia ao tipo", st.green("✓")),
            Some(d) => {
                let allowed: Vec<String> = d.allowed.iter().map(|c| c.to_string()).collect();
                println!(
                    "{} desvio na posição {}: após {:?} o modelo queria {}; o tipo só admitia {}",
                    st.red("↳"),
                    d.pos,
                    d.prefix,
                    d.wanted.map(|c| format!("`{c}`")).unwrap_or_else(|| "encerrar".into()),
                    allowed.join(" | ")
                );
            }
        }
    }
    Ok(true)
}

fn cmd_verify(opts: &Opts, st: Style) -> Result<bool, String> {
    let path = opts.file.clone().ok_or("informe o journal")?;
    let s = std::fs::read_to_string(&path).map_err(|e| format!("não foi possível ler `{path}`: {e}"))?;
    let mut entries = Vec::new();
    for (i, line) in s.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        entries.push(serde_json::from_str(line).map_err(|e| format!("linha {}: {e}", i + 1))?);
    }
    let chain_ok = match Journal::verify(&entries) {
        Ok(()) => {
            println!("{} {} entradas, cadeia de hashes íntegra", st.green("✓"), entries.len());
            true
        }
        Err(i) => {
            println!("{} cadeia quebrada na entrada {i} (seq {i}): conteúdo ou encadeamento adulterado", st.red("✗"));
            false
        }
    };
    // T9: toda chave que terminou em `k ↦ ?` precisa terminar resolvida (done/failed) ou transferida.
    let mut open: Vec<String> = Vec::new();
    let (mut done, mut failed, mut transferred) = (0, 0, 0);
    for e in &entries {
        let key = e["detail"]["key"].as_str().unwrap_or_default().to_string();
        match e["kind"].as_str() {
            Some("timeout_unknown") => open.push(key),
            Some("resolved") => {
                open.retain(|k| *k != key);
                if e["detail"]["status"] == "done" {
                    done += 1;
                } else {
                    failed += 1;
                }
            }
            Some("transfer") => {
                open.retain(|k| *k != key);
                transferred += 1;
            }
            _ => {}
        }
    }
    let minted = entries.iter().filter(|e| e["kind"] == "mint").count();
    let t9_ok = open.is_empty();
    if t9_ok {
        println!(
            "{} T9 · {minted} chave(s) cunhada(s); resultados desconhecidos: {} resolvido(s) Done, {} Failed, {} transferido(s); nenhuma em aberto",
            st.green("✓"),
            done,
            failed,
            transferred
        );
    } else {
        println!("{} T9 · chave(s) em aberto sem destino: {}", st.red("✗"), open.join(", "));
    }
    Ok(chain_ok && t9_ok)
}

fn read_journal(path: &str) -> Result<Vec<serde_json::Value>, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("não foi possível ler `{path}`: {e}"))?;
    s.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("{path}: linha {}: {e}", i + 1)))
        .collect()
}

fn quantile(xs: &mut [f64], q: f64) -> f64 {
    xs.sort_by(|a, b| a.total_cmp(b));
    let i = ((xs.len() as f64 - 1.0) * q).round() as usize;
    xs[i.min(xs.len() - 1)]
}

/// Aderência ao contrato Δ medida em journals reais e o Δ recalibrado pela cauda observada.
fn cmd_contract_report(opts: &Opts, st: Style) -> Result<bool, String> {
    let mut paths: Vec<String> = opts.file.iter().cloned().collect();
    paths.extend(opts.flags.get("journal").cloned().unwrap_or_default());
    if paths.is_empty() {
        return Err("informe ao menos um journal".into());
    }
    #[derive(Default)]
    struct Acc {
        label: String,
        lat: Vec<f64>,
        excess: Vec<f64>,
        ratio: Vec<f64>,
        overhead_seen: Vec<u64>,
        declared_overhead: Option<u64>,
        reasoning: Vec<u64>,
        tok_out: Vec<u64>,
        samples: usize,
        estimated: usize,
        breaches: std::collections::BTreeMap<String, usize>,
    }
    let mut by: std::collections::BTreeMap<String, Acc> = std::collections::BTreeMap::new();
    for p in &paths {
        for e in read_journal(p)? {
            let kind = e["kind"].as_str().unwrap_or_default();
            if kind != "infer" && kind != "contract_breach" {
                continue;
            }
            let d = &e["detail"];
            let model = d["model"].as_str().unwrap_or("?").to_string();
            let c = &d["contract"];
            let key = c["hash"].as_str().map(|h| format!("{model}#{}", &h[..12.min(h.len())])).unwrap_or(model.clone());
            let a = by.entry(key).or_default();
            a.label = match (c["provider"].as_str(), c["id"].as_str(), c["version"].as_str()) {
                (p, Some(id), v) => format!(
                    "{}{id}{}",
                    p.map(|p| format!("{p}:")).unwrap_or_default(),
                    v.map(|v| format!("@{v}")).unwrap_or_default()
                ),
                _ => "(sem identidade declarada)".into(),
            };
            a.declared_overhead = c["overhead"].as_u64().or(a.declared_overhead);
            if kind == "contract_breach" {
                *a.breaches.entry(d["code"].as_str().unwrap_or("?").to_string()).or_default() += 1;
                if let (Some(l), Some(cs)) = (d["observed_s"].as_f64(), d["declared_s"].as_f64()) {
                    a.lat.push(l);
                    a.excess.push(l - cs);
                    a.ratio.push(l / cs);
                }
                continue;
            }
            a.samples += 1;
            let server = d["tok_source"].as_str() == Some("server") || d["finish_reason"].is_string();
            if !server {
                a.estimated += 1;
            }
            if let (Some(l), Some(cs)) = (d["latency_s"].as_f64(), d["contract_s"].as_f64()) {
                a.lat.push(l);
                a.excess.push(l - cs);
                a.ratio.push(l / cs);
            }
            if server {
                if let Some(r) = d["tok_reasoning"].as_u64() {
                    a.reasoning.push(r);
                }
                if let Some(t) = d["tok_out"].as_u64() {
                    a.tok_out.push(t);
                }
                if let (Some(tin), Some(bound), Some(ov)) =
                    (d["tok_in"].as_u64(), d["tok_in_contract"].as_u64(), c["overhead"].as_u64())
                {
                    // prompt estimado = limite do contrato − overhead declarado
                    a.overhead_seen.push(tin.saturating_sub(bound - ov));
                }
            }
        }
    }
    if by.is_empty() {
        println!("nenhuma amostra de modelo nos journals");
        return Ok(true);
    }
    for (key, a) in &mut by {
        println!("{} `{key}` = {}", st.bold("contrato"), a.label);
        println!("  amostras          {} ({} com tokens estimados)", a.samples, a.estimated);
        if !a.lat.is_empty() {
            let max_ratio = a.ratio.iter().cloned().fold(f64::MIN, f64::max);
            let max_excess = a.excess.iter().cloned().fold(f64::MIN, f64::max);
            let (p50, p95, max) =
                (quantile(&mut a.lat.clone(), 0.5), quantile(&mut a.lat.clone(), 0.95), quantile(&mut a.lat, 1.0));
            println!(
                "  latência medida   p50 {} · p95 {} · máx {} · maior razão latência/contrato {}",
                fmt::secs(p50),
                fmt::secs(p95),
                fmt::secs(max),
                fmt::dec(max_ratio, 2)
            );
            if max_excess > 0.0 {
                println!(
                    "  {}  a cauda passou do contrato em até {}: eleve `ttft` em ≥ {}",
                    st.red("Δ recalibrado"),
                    fmt::secs(max_excess),
                    fmt::secs((max_excess * 100.0).ceil() / 100.0)
                );
            } else {
                println!("  {}  a cauda observada cabe no contrato de latência", st.green("Δ"));
            }
        }
        if let Some(m) = a.overhead_seen.iter().max() {
            let decl = a.declared_overhead.unwrap_or(0);
            let suggest = ((*m as f64) * 1.25).ceil() as u64;
            let verdict = if *m <= decl { st.green("cobre") } else { st.red("NÃO cobre") };
            println!(
                "  overhead medido   máx {} · declarado {} ({verdict}) · sugerido ≥ {} (máx × 1,25)",
                fmt::tok(*m),
                fmt::tok(decl),
                fmt::tok(suggest)
            );
        } else {
            println!("  overhead medido   — (journal sem `tok_in_contract` ou sem `usage` do servidor)");
        }
        if let (Some(r), Some(t)) = (a.reasoning.iter().max(), a.tok_out.iter().max()) {
            println!("  saída medida      máx {} (raciocínio máx {})", fmt::tok(*t), fmt::tok(*r));
        }
        if a.breaches.is_empty() {
            println!("  violações         nenhuma");
        } else {
            let v: Vec<String> = a.breaches.iter().map(|(k, n)| format!("{k} ×{n}")).collect();
            println!("  violações         {}", v.join(", "));
        }
    }
    Ok(true)
}

fn cmd_emit(opts: &Opts, st: Style) -> Result<bool, String> {
    let (path, src) = load(opts)?;
    let Some(c) = compile_or_report(&path, &src, st) else { return Ok(false) };
    match opts.one("target").unwrap_or("py") {
        "py" | "python" => {}
        other => return Err(format!("alvo desconhecido `{other}` (o Marco Zero emite `py`)")),
    }
    let py = ath_emit::python(&c.module, &c.check, &c.codex, &path).map_err(|e| format!("emissão: {e}"))?;
    match opts.one("out") {
        Some(out) => {
            std::fs::write(out, &py).map_err(|e| format!("falha ao gravar `{out}`: {e}"))?;
            println!("{} {path} → {out} · Codex #{}", st.green("✓"), ath_codex::short(&c.codex.root));
        }
        None => print!("{py}"),
    }
    Ok(true)
}

/// Reexecuta o programa embutido num cassete contra as respostas gravadas.
fn cmd_replay(opts: &Opts, st: Style) -> Result<bool, String> {
    let cpath = opts.file.clone().ok_or("informe o cassete")?;
    let cas = Cassette::load(Path::new(&cpath))?;
    let m = &cas.manifest;
    let src = m["program"].as_str().ok_or("cassete sem o programa embutido")?;
    let label = m["source_path"].as_str().unwrap_or("cassete");
    let Some(c) = compile_or_report(label, src, st) else { return Ok(false) };
    if let Some(out) = opts.one("emit-py") {
        let py = ath_emit::python(&c.module, &c.check, &c.codex, label).map_err(|e| format!("emissão: {e}"))?;
        std::fs::write(out, py).map_err(|e| format!("falha ao gravar `{out}`: {e}"))?;
    }
    let agent = m["agent"].as_str().ok_or("manifesto sem `agent`")?;
    let event = m["event"].as_str().ok_or("manifesto sem `event`")?;
    let world = SimWorld::from_json(&m["world"])?;
    let provider = OpenAiCompat::from_config(&cas.config, Box::new(Replay::new(cas.exchanges.clone())))?;
    let mut arx = Arx::new(&c.module, Box::new(world), Box::new(provider));
    arx.contracts = athc::contract_hashes(&c);
    let (_, h) = arx.handler(agent, event).ok_or_else(|| format!("handler `{agent}.{event}` não encontrado"))?;
    let raw: Vec<serde_json::Value> = m["args"].as_array().cloned().unwrap_or_default();
    let mut args = Vec::new();
    for (p, r) in h.params.iter().zip(&raw) {
        args.push(arx.value_from_json(r, &p.ty).ok_or_else(|| format!("argumento `{r}` inválido no manifesto"))?);
    }
    let result = arx.run(agent, event, args);
    let journal = arx.journal.to_jsonl();
    if let Some(out) = opts.one("journal") {
        std::fs::write(out, &journal).map_err(|e| format!("falha ao gravar o journal: {e}"))?;
    }
    let shown = match &result {
        Ok(v) => v.show(),
        Err(e) => format!("{} {}", e.code, e.msg),
    };
    println!("{} {label} · {} troca(s) · {} → {shown}", st.bold("replay"), cas.exchanges.len(), cas.provider);
    match first_divergence(&cas.journal, &journal) {
        None => {
            println!(
                "{} journal idêntico ao gravado: {} entradas · cabeça #{}",
                st.green("✓"),
                arx.journal.entries.len(),
                &arx.journal.head[..12]
            );
            Ok(true)
        }
        Some((i, a, b)) => {
            println!("{} journal diverge na entrada {i}\n  gravado: {a}\n  replay:  {b}", st.red("✗"));
            Ok(false)
        }
    }
}
