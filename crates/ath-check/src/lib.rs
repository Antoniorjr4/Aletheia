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
                    linear::check_pending(h, &mut diags);
                }
            }
        }
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

    CheckResult { diags, reports, facts: cx.facts, definitions: m.items.len() }
}
