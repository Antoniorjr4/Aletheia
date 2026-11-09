//! Fachada do compilador: parse → estratos E0–E6 → Codex.

use ath_check::fmt;
use ath_check::{check_module, CheckResult, Diag, Stratum};
use ath_codex::Codex;
use ath_syntax::ast::Module;
use ath_syntax::span::{line_col, line_text};
use std::fmt::Write as _;

pub struct Compiled {
    pub module: Module,
    pub check: CheckResult,
    pub codex: Codex,
}

pub fn compile(src: &str) -> Result<Compiled, Vec<Diag>> {
    let module =
        ath_syntax::parse_module(src).map_err(|e| vec![Diag::new(Stratum::Parse, "P0001", e.msg, e.span, "aqui")])?;
    let check = check_module(&module);
    if !check.ok() {
        return Err(check.diags);
    }
    let codex = ath_codex::address(&module)
        .map_err(|m| vec![Diag::new(Stratum::E0, "C0001", m, ath_syntax::Span::default(), "")])?;
    Ok(Compiled { module, check, codex })
}

/// Hash do Codex de cada modelo declarado: a identidade do contrato Δ que o ARX grava no journal.
pub fn contract_hashes(c: &Compiled) -> std::collections::HashMap<String, String> {
    c.module
        .items
        .iter()
        .filter_map(|i| match i {
            ath_syntax::ast::Item::Model(m) => {
                c.codex.hash_of(&m.name.name).map(|h| (m.name.name.clone(), h.to_string()))
            }
            _ => None,
        })
        .collect()
}

/// Estilo de saída: com ou sem cores ANSI.
#[derive(Clone, Copy)]
pub struct Style {
    pub color: bool,
}

impl Style {
    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    pub fn red(&self, s: &str) -> String {
        self.paint("1;31", s)
    }
    pub fn green(&self, s: &str) -> String {
        self.paint("32", s)
    }
    pub fn blue(&self, s: &str) -> String {
        self.paint("1;34", s)
    }
    pub fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
    pub fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }
}

fn underline(src: &str, lo: u32, hi: u32) -> (usize, usize, usize) {
    let (line, col) = line_col(src, lo);
    let (end_line, end_col) = line_col(src, hi);
    let text = line_text(src, line);
    let width = if end_line == line {
        end_col.saturating_sub(col).max(1)
    } else {
        text.chars().count().saturating_sub(col - 1).max(1)
    };
    (line, col, width)
}

/// Diagnóstico no estilo do rustc, com a linha de código e o trecho sublinhado.
pub fn render(src: &str, path: &str, d: &Diag, st: Style) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "{}: {}", st.red(&format!("error[{}]", d.code)), st.bold(&d.msg));
    if d.span.hi == 0 && d.span.lo == 0 && d.code.starts_with('C') {
        return o;
    }
    let (line, col, width) = underline(src, d.span.lo, d.span.hi);
    let mut lines: Vec<usize> = vec![line];
    for (s, _) in &d.secondary {
        lines.push(line_col(src, s.lo).0);
    }
    let gw = lines.iter().max().unwrap().to_string().len();
    let pad = " ".repeat(gw);
    let _ = writeln!(o, "{pad}{} {path}:{line}:{col}", st.blue("-->"));
    let _ = writeln!(o, "{pad} {}", st.blue("|"));
    // Todas as marcas (principal e secundárias) em ordem de posição; marcas da
    // mesma linha compartilham uma única cópia da linha de código.
    let mut marks: Vec<(u32, usize, usize, usize, &str, bool)> =
        vec![(d.span.lo, line, col, width, d.label.as_str(), true)];
    for (s, label) in &d.secondary {
        let (l, c, w) = underline(src, s.lo, s.hi);
        marks.push((s.lo, l, c, w, label.as_str(), false));
    }
    marks.sort_by_key(|m| m.0);
    let mut last_line = 0;
    for (_, l, c, w, label, primary) in marks {
        if l != last_line {
            let _ = writeln!(o, "{} {} {}", st.blue(&format!("{l:>gw$}")), st.blue("|"), line_text(src, l));
            last_line = l;
        }
        let glyphs = if primary { "^" } else { "-" }.repeat(w);
        let painted =
            if primary { st.red(&format!("{glyphs} {label}")) } else { st.blue(&format!("{glyphs} {label}")) };
        let _ = writeln!(o, "{pad} {} {}{}", st.blue("|"), " ".repeat(c - 1), painted);
    }
    if !d.notes.is_empty() || d.help.is_some() {
        let _ = writeln!(o, "{pad} {}", st.blue("|"));
    }
    for n in &d.notes {
        let _ = writeln!(o, "{pad} {} {n}", st.blue("="));
    }
    if let Some(h) = &d.help {
        let _ = writeln!(o, "{pad} {} {}", st.blue("="), st.bold(&format!("ajuda: {h}")));
    }
    o
}

/// Resumo de verificação: uma linha por estrato.
pub fn summary(c: &Compiled, st: Style) -> String {
    let mut o = String::new();
    let ok = st.green("✓");
    let f = &c.check.facts;
    let _ = writeln!(o, "{ok} {:<18} {} definições, todas resolvidas", "E0 estrutura", c.check.definitions);
    let _ = writeln!(
        o,
        "{ok} {:<18} {} chave(s) IdemKey e as obrigações Pending consumidas exatamente uma vez",
        "E1 multiplicidade", f.keyed_calls
    );
    let _ = writeln!(
        o,
        "{ok} {:<18} {{{}}} cobertos por capacidades e linhas de efeito",
        "E2 efeitos",
        f.effects.iter().cloned().collect::<Vec<_>>().join(", ")
    );
    if f.atomic_blocks > 0 || f.keyed_calls > 0 {
        let comps: Vec<String> = f.compensations.iter().map(|(a, b)| format!("{a} ↩ {b}")).collect();
        let _ = writeln!(
            o,
            "{ok} {:<18} {} saga(s); compensações: {}; irrevogáveis só via commit, fora de atomic",
            "E2 sagas",
            f.atomic_blocks,
            if comps.is_empty() { "nenhuma".to_string() } else { comps.join(", ") }
        );
        if let Some(b) = f.reconcile_bound {
            let _ = writeln!(
                o,
                "  {:<18} TimeoutUnknown tratado; reconciliação fora do contrato Δ: ≤ {} por chave",
                "",
                fmt::secs(b)
            );
        }
    }
    let _ = writeln!(
        o,
        "{ok} {:<18} {} exigência(s) de integridade satisfeitas, inclusive sob pc",
        "E3 integridade", f.integrity_checks
    );
    let _ = writeln!(
        o,
        "{ok} {:<18} {} obrigação(ões) provadas por literal, campo refinado ou saída tipada",
        "E5 refinamentos", f.refinement_checks
    );
    let mut infers: Vec<_> = f.infer_costs.iter().collect();
    infers.sort_by_key(|(span, _)| span.lo);
    for (_, ic) in infers {
        let content = ic.tok_out - ic.tok_reasoning;
        let mut ceiling = if ic.out_by_type {
            format!("{} do tipo `{}`", fmt::tok(content), ic.out_ty)
        } else {
            format!("{} do max_out do modelo", fmt::tok(content))
        };
        if ic.tok_reasoning > 0 {
            ceiling.push_str(&format!(" + {} de raciocínio", fmt::tok(ic.tok_reasoning)));
        }
        let entrada = if ic.overhead > 0 {
            format!(
                "{} ({} de prompt + {} de overhead)",
                fmt::tok(ic.tok_in),
                fmt::int(ic.tok_in - ic.overhead),
                fmt::int(ic.overhead)
            )
        } else {
            fmt::tok(ic.tok_in)
        };
        let _ = writeln!(
            o,
            "  {:<18} infer {} -> {}: entrada ≤ {} · saída ≤ {} ({})",
            "",
            ic.model,
            ic.out_ty,
            entrada,
            fmt::tok(ic.tok_out),
            ceiling
        );
    }
    for item in &c.module.items {
        if let ath_syntax::ast::Item::Model(m) = item {
            let Some(label) = m.contract_label() else { continue };
            let hash = c.codex.hash_of(&m.name.name).map(ath_codex::short).unwrap_or("?");
            let ov = m.prop("overhead").map(|v| format!(" · overhead {}", fmt::tok(v as u64))).unwrap_or_default();
            let _ = writeln!(o, "{ok} {:<18} `{}` = {label} #{hash}{ov}", "Δ contrato", m.name.name);
        }
    }
    for r in &c.check.reports {
        let w = &r.worst;
        let lim = |v: Option<String>| v.unwrap_or_else(|| "∞".into());
        let _ = writeln!(
            o,
            "{ok} {:<18} {}.{}: pior caminho {} · {} · {}",
            "E6 orçamento",
            r.agent,
            r.event,
            fmt::tok(w.tok),
            fmt::usd(w.usd),
            fmt::secs(w.wall)
        );
        let _ = writeln!(
            o,
            "  {:<18} ≤ [{} · {} · {}]",
            "",
            lim(r.tok.map(fmt::tok)),
            lim(r.usd.map(fmt::usd)),
            lim(r.wall.map(fmt::secs))
        );
        if !w.trace.is_empty() {
            let _ = writeln!(o, "  {:<18} {}", "", st.dim(&format!("caminho: {}", w.path())));
        }
    }
    if f.loops > 0 {
        let _ = writeln!(
            o,
            "  {:<18} {} laço(s)/quantificador(es) delimitado(s): β(laço) ⪯ teto · β(corpo), aninhados por multiplicação",
            "", f.loops
        );
    }
    let _ = writeln!(
        o,
        "{ok} {:<18} {} objetos BLAKE3, raiz #{}",
        "Codex",
        c.codex.entries.len(),
        ath_codex::short(&c.codex.root)
    );
    o
}
