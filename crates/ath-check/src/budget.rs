//! E6 — orçamento estático sob o contrato Δ (spec v0.2, seção 2.7).
//!
//! Custo de pior caminho, distinguindo caminhos que continuam (`fall`) dos que
//! saem por `return` (`ret`). Em sequência tudo soma; em alternativa toma-se o
//! máximo por componente. `refine … retry n` custa n amostras no pior caso.

use crate::check::Ctx;
use crate::diag::{Diag, Stratum};
use crate::fmt;
use ath_syntax::ast::*;
use ath_syntax::Span;

#[derive(Clone, Debug, Default)]
pub struct Cost {
    pub tok: u64,
    pub usd: f64,
    pub wall: f64,
    /// Contribuições de tempo do caminho de maior latência.
    pub trace: Vec<(String, f64)>,
}

impl Cost {
    fn add(&self, o: &Cost) -> Cost {
        Cost {
            tok: self.tok + o.tok,
            usd: self.usd + o.usd,
            wall: self.wall + o.wall,
            trace: self.trace.iter().chain(o.trace.iter()).cloned().collect(),
        }
    }

    fn max(&self, o: &Cost) -> Cost {
        Cost {
            tok: self.tok.max(o.tok),
            usd: self.usd.max(o.usd),
            wall: self.wall.max(o.wall),
            trace: if o.wall > self.wall { o.trace.clone() } else { self.trace.clone() },
        }
    }

    pub fn path(&self) -> String {
        self.trace.iter().map(|(n, w)| format!("{} {}", n, fmt::dec(*w, 1))).collect::<Vec<_>>().join(" → ")
    }
}

fn max_opt(a: Option<Cost>, b: Option<Cost>) -> Option<Cost> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(&y)),
        (x, None) => x,
        (None, y) => y,
    }
}

#[derive(Clone, Debug)]
struct PCost {
    fall: Option<Cost>,
    ret: Option<Cost>,
}

impl PCost {
    fn zero() -> Self {
        PCost { fall: Some(Cost::default()), ret: None }
    }

    fn leaf(c: Cost) -> Self {
        PCost { fall: Some(c), ret: None }
    }

    fn seq(self, b: PCost) -> PCost {
        let fall = match (&self.fall, &b.fall) {
            (Some(x), Some(y)) => Some(x.add(y)),
            _ => None,
        };
        let via = match (&self.fall, &b.ret) {
            (Some(x), Some(y)) => Some(x.add(y)),
            _ => None,
        };
        PCost { fall, ret: max_opt(self.ret, via) }
    }

    fn alt(self, b: PCost) -> PCost {
        PCost { fall: max_opt(self.fall, b.fall), ret: max_opt(self.ret, b.ret) }
    }

    fn diverge(self) -> PCost {
        PCost { fall: None, ret: max_opt(self.fall, self.ret) }
    }

    fn worst(self) -> Cost {
        max_opt(self.fall, self.ret).unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub struct HandlerReport {
    pub agent: String,
    pub event: String,
    pub worst: Cost,
    pub tok: Option<u64>,
    pub usd: Option<f64>,
    pub wall: Option<f64>,
}

pub fn analyze(cx: &Ctx, a: &AgentDecl, h: &Handler) -> HandlerReport {
    let worst = Walker { cx }.block(&h.body).worst();
    HandlerReport {
        agent: a.name.name.clone(),
        event: h.event.name.clone(),
        worst,
        tok: a.budget("tok").map(|v| v as u64),
        usd: a.budget("usd"),
        wall: a.budget("wall"),
    }
}

pub fn compare(r: &HandlerReport, a: &AgentDecl, diags: &mut Vec<Diag>) {
    let w = &r.worst;
    let at = a.budget_span;
    let whose = format!("pior caminho de `{}.{}` sob o contrato Δ", r.agent, r.event);
    if let Some(lim) = r.tok {
        if w.tok > lim {
            diags.push(
                Diag::new(
                    Stratum::E6,
                    "B0201",
                    "orçamento de tokens excedido",
                    at,
                    format!("pior caso: {}", fmt::tok(w.tok)),
                )
                .note(whose.clone())
                .help(format!("reduza `retry` ou o prompt, ou eleve `tok` para ≥ {}", fmt::int(w.tok))),
            );
        }
    }
    if let Some(lim) = r.usd {
        if w.usd > lim + 1e-12 {
            diags.push(
                Diag::new(
                    Stratum::E6,
                    "B0202",
                    "orçamento em dólares excedido",
                    at,
                    format!("pior caso: {}", fmt::usd(w.usd)),
                )
                .note(whose.clone())
                .help("reduza `retry` ou escolha um modelo mais barato"),
            );
        }
    }
    if let Some(lim) = r.wall {
        if w.wall > lim + 1e-9 {
            diags.push(
                Diag::new(
                    Stratum::E6,
                    "B0207",
                    "orçamento de tempo excedido",
                    at,
                    format!("pior caso: {}", fmt::secs(w.wall)),
                )
                .note(format!("caminho: {} = {}", w.path(), fmt::secs(w.wall)))
                .note(whose)
                .help(format!("reduza `retry` ou eleve `wall` para ≥ {}", fmt::dec(w.wall.ceil(), 0))),
            );
        }
    }
}

struct Walker<'a, 'm> {
    cx: &'a Ctx<'m>,
}

impl Walker<'_, '_> {
    fn infer_cost(&self, site: Span, times: u32) -> Cost {
        let Some(isp) = self.cx.facts.draft_src.get(&site) else { return Cost::default() };
        let Some(ic) = self.cx.facts.infer_costs.get(isp) else { return Cost::default() };
        let n = times as f64;
        let label = if times > 1 { format!("infer {} ×{}", ic.model, times) } else { format!("infer {}", ic.model) };
        Cost {
            tok: times as u64 * (ic.tok_in + ic.tok_out),
            usd: n * ic.usd,
            wall: n * ic.wall,
            trace: vec![(label, n * ic.wall)],
        }
    }

    fn seq_all<'b>(&self, it: impl Iterator<Item = &'b Expr>) -> PCost {
        it.fold(PCost::zero(), |acc, x| acc.seq(self.expr(x)))
    }

    fn block(&self, b: &Block) -> PCost {
        let mut acc = PCost::zero();
        for s in &b.stmts {
            let x = match s {
                Stmt::Let { value, .. } => value,
                Stmt::Expr(x) => x,
            };
            acc = acc.seq(self.expr(x));
        }
        if let Some(t) = &b.tail {
            acc = acc.seq(self.expr(t));
        }
        acc
    }

    fn expr(&self, e: &Expr) -> PCost {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Var(_) => PCost::zero(),
            ExprKind::Field(r, _) => self.expr(r),
            ExprKind::Call { callee, args } => {
                let mut acc = self.seq_all(args.iter());
                if let Some(t) = self.cx.tools.get(&callee.name) {
                    acc = acc.seq(PCost::leaf(Cost {
                        tok: 0,
                        usd: t.usd,
                        wall: t.latency,
                        trace: vec![(callee.name.clone(), t.latency)],
                    }));
                } else if callee.name == "sample" {
                    acc = acc.seq(PCost::leaf(self.infer_cost(e.span, 1)));
                }
                acc
            }
            ExprKind::Method { recv, args, .. } => self.expr(recv).seq(self.seq_all(args.iter())),
            ExprKind::Ctor { fields, .. } => self.seq_all(fields.iter().map(|(_, x)| x)),
            ExprKind::Binary(_, l, r) => self.expr(l).seq(self.expr(r)),
            ExprKind::Unary(_, v) | ExprKind::Quote(v) | ExprKind::Commit(v) => self.expr(v),
            ExprKind::If { cond, then, els } => {
                let other = els.as_ref().map(|b| self.block(b)).unwrap_or_else(PCost::zero);
                self.expr(cond).seq(self.block(then).alt(other))
            }
            ExprKind::Match { scrut, arms } => {
                let mut acc: Option<PCost> = None;
                for a in arms {
                    let c = self.expr(&a.body);
                    acc = Some(match acc {
                        None => c,
                        Some(p) => p.alt(c),
                    });
                }
                self.expr(scrut).seq(acc.unwrap_or_else(PCost::zero))
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Return(v) => self.expr(v).diverge(),
            ExprKind::Infer { fields, .. } => self.seq_all(fields.iter().map(|(_, x)| x)),
            ExprKind::Refine { draft, retry, els, .. } => self
                .expr(draft)
                .seq(PCost::leaf(self.infer_cost(e.span, *retry)))
                .seq(PCost::zero().alt(self.block(els))),
        }
    }
}
