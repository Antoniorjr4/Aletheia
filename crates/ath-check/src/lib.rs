//! Verificador estratificado de Aletheia Core 0.
//!
//! Estratos implementados no Marco Zero (numeração da spec v0.2, seção 2.10):
//!
//! | Estrato | Domínio                                   |
//! |---------|-------------------------------------------|
//! | E0      | estrutura: tipos, ADTs, exaustividade     |
//! | E1      | multiplicidade: `IdemKey` linear          |
//! | E2      | efeitos e capacidades, `commit`           |
//! | E3      | integridade com `pc` (fluxos implícitos)  |
//! | E6      | orçamento estático no pior caminho        |
//!
//! Erros são relatados apenas para o primeiro estrato que falha.

pub mod budget;
pub mod check;
pub mod diag;
pub mod fmt;
pub mod linear;
pub mod ty;

pub use budget::{Cost, HandlerReport};
pub use check::{Facts, InferCost};
pub use diag::{Diag, Stratum};

use ath_syntax::ast::{Item, Module};

pub struct CheckResult {
    /// Diagnósticos do primeiro estrato que falhou (vazio = programa aceito).
    pub diags: Vec<Diag>,
    /// Avisos experimentais: não impedem a compilação nem certificam nada (hoje só a E0453).
    pub warnings: Vec<Diag>,
    pub reports: Vec<HandlerReport>,
    pub facts: Facts,
    pub definitions: usize,
}

impl CheckResult {
    pub fn ok(&self) -> bool {
        self.diags.is_empty()
    }
}

pub fn check_module(m: &Module) -> CheckResult {
    let mut cx = check::Ctx::new(m);
    cx.check_all();
    let mut diags = std::mem::take(&mut cx.diags);

    // E1 só faz sentido sobre um programa estruturalmente bem tipado.
    if !diags.iter().any(|d| d.stratum == Stratum::E0) {
        for item in &m.items {
            if let Item::Agent(a) = item {
                for h in &a.handlers {
                    linear::check_block(&h.body, &cx.facts, &mut diags);
                }
                for h in &a.handles {
                    linear::check_block(&h.body, &cx.facts, &mut diags);
                    linear::check_pending(h, &cx.facts, &mut diags);
                }
            }
        }
    }

    // E0453 (28/11/2026), **aviso experimental**: uma chamada de ferramenta que parece combinar
    // dados de origens aprovadas distintas. A análise de origem (`ath_syntax::operations`) é
    // parcial, tem fugas conhecidas e supõe um programa bem tipado: não recusa o programa.
    let mut warnings = Vec::new();
    if !diags.iter().any(|d| d.stratum == Stratum::E0) {
        operations_check(m, &mut warnings);
    }

    let mut reports = Vec::new();
    if !diags.iter().any(|d| d.stratum <= Stratum::E5) {
        for item in &m.items {
            if let Item::Agent(a) = item {
                for h in &a.handlers {
                    let r = budget::analyze(&cx, a, h);
                    budget::compare(&r, a, &mut diags);
                    reports.push(r);
                }
            }
        }
    }

    // Reporta só o primeiro estrato que falha, em ordem de posição no texto.
    if let Some(first) = diags.iter().map(|d| d.stratum).min() {
        diags.retain(|d| d.stratum == first);
        diags.sort_by_key(|d| d.span.lo);
    }

    CheckResult { diags, warnings, reports, facts: cx.facts, definitions: m.items.len() }
}

/// E0453, **aviso experimental** (não recusa): a chamada parece combinar dados de duas ou mais operações aprovadas (elementos escolhidos de
/// listas endossadas, valores endossados inteiros, ou a agregação de uma lista com outra origem).
///
/// O que ela **não** afirma: que os argumentos sejam idênticos aos de uma operação aprovada. Uma
/// operação só, com campos transformados (`fit(memo ++ "!", 12)`), continua aceita; e a agregação
/// sobre uma lista inteira, sozinha, também (`join(notas.issue, …)`), porque ela não escolhe
/// elementos: essas duas classes ficam fora da garantia e são declaradas como tal.
fn operations_check(m: &Module, diags: &mut Vec<Diag>) {
    use ath_syntax::operations::{analyze, Finding};
    for site in analyze(m) {
        let Finding::Mistura(ops) = &site.finding else { continue };
        let mut d = Diag::new(
            Stratum::E3,
            "E0453",
            format!("a chamada de `{}` parece combinar dados de {} origens aprovadas distintas", site.tool, ops.len()),
            site.call,
            "argumentos de origens aprovadas diferentes",
        );
        let mut seen = std::collections::BTreeSet::new();
        for o in ops {
            if seen.insert((o.at.lo, o.at.hi, o.what.clone())) {
                d = d.secondary(o.at, format!("origem: {}", o.what));
            }
        }
        d = d
            .note(
                "aviso experimental: a análise de origem é parcial e tem fugas conhecidas; ela não \
                 recusa o programa nem certifica a integridade da operação quando não avisa",
            )
            .note("a política aprovou cada origem; nenhuma aprovou esta combinação numa chamada")
            .note(
                "a E0453 não prova que os argumentos sejam os aprovados: os campos de uma só operação \
                 podem ser transformados, e a agregação sobre uma lista inteira, sozinha, não é coberta",
            )
            .help(
                "use os campos de uma só operação; se a combinação é legítima, declare-a como uma \
                 variante do plano, proposta pelo modelo e decidida pela política no `refine`",
            );
        diags.push(d);
    }
}
