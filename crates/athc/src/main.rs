use arx::host::SimWorld;
use arx::journal::Journal;
use arx::model::{Constraint, Masked, ModelProvider, OpenAiCompat, Scripted};
use arx::{Arx, Value};
use ath_check::fmt;
use ath_syntax::ast::{Item, TypeExpr};
use ath_syntax::parse_module;
use athc::{compile, render, summary, Compiled, Style};
use serde_json::json;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "athc — compilador de Aletheia Core 0 (Marco Zero)

uso:
  athc check <arquivo.ath>
      verifica os estratos E0, E1, E2, E3 e E6 e endereça o Codex
  athc run <arquivo.ath> [--agent A] [--on evento] [--arg literal]...
                         [--world mundo.json] [--models modelos.json] [--journal saída.jsonl]
                         [--provider masked|scripted|openai] [--report relatório.json]
      verifica e executa um handler no runtime ARX
      (sem --world/--models, usa fixtures/world.json e fixtures/models.json ao lado do arquivo)
      provedores:
        masked    (padrão) roteiro como proposta + máscara derivada do tipo: saída válida por construção
        scripted  roteiro literal, sem restrição; o ARX valida e rejeita saídas fora do tipo
        openai    API compatível com OpenAI (OpenAI, vLLM, llama.cpp…):
                  --base-url URL --remote-model ID [--constraint json-schema|gbnf|none]
                  chave em ATH_API_KEY ou OPENAI_API_KEY
  athc grammar <arquivo.ath> --type T [--format all|schema|gbnf|shape] [--decode 'proposta']
      mostra a gramática derivada do tipo T e, com --decode, a decodificação mascarada de uma proposta
  athc hash <arquivo.ath> [--codex dir]
      grava as formas canônicas no Codex (padrão: .codex) e lista os hashes
  athc verify-journal <journal.jsonl>
      recalcula a cadeia de hashes do journal";

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
    let world = match fixture(opts, "world", &path, "world.json") {
        Some(p) => SimWorld::from_json(&read_json(&p)?)?,
        None => SimWorld::default(),
    };
    let provider = build_provider(opts, &path)?;
    let provider_name = provider.name();
    let mut arx = Arx::new(&c.module, Box::new(world), provider);
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
        let ty = p.ty.base_name().to_string();
        let v = arx
            .literal_from_str(r, &ty)
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
        "openai" => {
            let base =
                opts.one("base-url").ok_or("--provider openai exige --base-url (ex.: http://localhost:8000/v1)")?;
            let model = opts.one("remote-model").ok_or("--provider openai exige --remote-model")?;
            let constraint = Constraint::parse(opts.one("constraint").unwrap_or("json-schema"))
                .ok_or("--constraint deve ser json-schema, gbnf ou none")?;
            let key = std::env::var("ATH_API_KEY").or_else(|_| std::env::var("OPENAI_API_KEY")).ok();
            Ok(Box::new(OpenAiCompat::new(base, model, key, constraint)))
        }
        other => Err(format!("provedor desconhecido `{other}` (use masked, scripted ou openai)")),
    }
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
    match Journal::verify(&entries) {
        Ok(()) => {
            println!("{} {} entradas, cadeia de hashes íntegra", st.green("✓"), entries.len());
            Ok(true)
        }
        Err(i) => {
            println!("{} cadeia quebrada na entrada {i} (seq {i}): conteúdo ou encadeamento adulterado", st.red("✗"));
            Ok(false)
        }
    }
}
