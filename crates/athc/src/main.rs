use arx::host::SimWorld;
use arx::journal::Journal;
use arx::model::Scripted;
use arx::{Arx, Value};
use ath_check::fmt;
use ath_syntax::ast::{Item, TypeExpr};
use athc::{compile, render, summary, Compiled, Style};
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
      verifica e executa um handler no runtime ARX
      (sem --world/--models, usa fixtures/world.json e fixtures/models.json ao lado do arquivo)
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
            let tail = if stratum < ath_check::Stratum::E6 { "; os estratos seguintes não foram avaliados" } else { "" };
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
        let deps: Vec<String> = e.deps.iter().map(|d| format!("{d}#{}", &c.codex.hash_of(d).unwrap_or("?")[..6])).collect();
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
    println!("{} verificado: E0 E1 E2 E3 E6 · Codex #{}", st.green("✓"), ath_codex::short(&c.codex.root));

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
    let models = match fixture(opts, "models", &path, "models.json") {
        Some(p) => Scripted::from_json(&read_json(&p)?)?,
        None => Scripted::default(),
    };
    let mut arx = Arx::new(&c.module, Box::new(world), Box::new(models));
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
        let ty = match &p.ty {
            TypeExpr::Labeled(inner, ..) => match inner.as_ref() {
                TypeExpr::Named(id) => id.name.clone(),
                _ => "?".into(),
            },
            TypeExpr::Named(id) => id.name.clone(),
            _ => "?".into(),
        };
        let v = arx
            .literal_from_str(r, &ty)
            .ok_or_else(|| format!("`{r}` não é um literal do tipo `{ty}` (texto vai entre aspas: --arg '\"api\"')"))?;
        args.push(v);
    }

    let shown: Vec<String> = args.iter().map(Value::show).collect();
    println!("{} {}.{}({})", st.bold("▶"), agent, event, shown.join(", "));
    let result = arx.run(&agent, &event, args);
    println!("  {}", st.dim(&format!("{:>7}  {:>7}", "t", "+Δt")));
    for ev in &arx.events {
        println!("  {:>7}  {:>7}  {}  {}", fmt::dec(ev.t, 1), format!("+{}", fmt::dec(ev.dt, 1)), ev.what, st.dim(&ev.detail));
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
    Ok(ok)
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
