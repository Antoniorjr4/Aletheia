//! E1 — multiplicidade. Uma `IdemKey` vinculada por `let` deve ser consumida
//! exatamente uma vez em todo caminho, inclusive nos que terminam em `return`.
//!
//! Interpretação abstrata sobre contagens saturadas em {0, 1, 2+}:
//! `fall` são as contagens possíveis nos caminhos que continuam,
//! `ret` as dos caminhos que saem por `return`.

use crate::check::Facts;
use crate::diag::{Diag, Stratum};
use ath_syntax::ast::*;
use ath_syntax::Span;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
struct Cnt {
    fall: BTreeSet<u8>,
    ret: BTreeSet<u8>,
}

impl Cnt {
    fn zero() -> Cnt {
        Cnt { fall: [0].into(), ret: BTreeSet::new() }
    }

    fn one() -> Cnt {
        Cnt { fall: [1].into(), ret: BTreeSet::new() }
    }

    fn seq(self, b: Cnt) -> Cnt {
        let mut fall = BTreeSet::new();
        let mut ret = self.ret.clone();
        for &x in &self.fall {
            for &y in &b.fall {
                fall.insert((x + y).min(2));
            }
            for &y in &b.ret {
                ret.insert((x + y).min(2));
            }
        }
        Cnt { fall, ret }
    }

    fn alt(self, b: Cnt) -> Cnt {
        Cnt { fall: self.fall.union(&b.fall).copied().collect(), ret: self.ret.union(&b.ret).copied().collect() }
    }

    fn diverge(self) -> Cnt {
        Cnt { fall: BTreeSet::new(), ret: self.fall.union(&self.ret).copied().collect() }
    }
}

fn seq_all<'a>(it: impl Iterator<Item = &'a Expr>, x: &str) -> Cnt {
    it.fold(Cnt::zero(), |acc, e| acc.seq(count_expr(e, x)))
}

fn count_expr(e: &Expr, x: &str) -> Cnt {
    match &e.kind {
        ExprKind::Var(n) if n == x => Cnt::one(),
        ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Var(_) => Cnt::zero(),
        ExprKind::Field(r, _) => count_expr(r, x),
        ExprKind::Call { args, .. } => seq_all(args.iter(), x),
        ExprKind::Method { recv, args, .. } => count_expr(recv, x).seq(seq_all(args.iter(), x)),
        ExprKind::Ctor { fields, .. } => seq_all(fields.iter().map(|(_, e)| e), x),
        ExprKind::Binary(_, l, r) => count_expr(l, x).seq(count_expr(r, x)),
        ExprKind::Unary(_, v) | ExprKind::Quote(v) | ExprKind::Commit(v) => count_expr(v, x),
        ExprKind::If { cond, then, els } => {
            let other = els.as_ref().map(|b| count_block(b, x)).unwrap_or_else(Cnt::zero);
            count_expr(cond, x).seq(count_block(then, x).alt(other))
        }
        ExprKind::Match { scrut, arms } => {
            let mut acc: Option<Cnt> = None;
            for a in arms {
                let c = if a.binds.iter().any(|b| b.name == x) { Cnt::zero() } else { count_expr(&a.body, x) };
                acc = Some(match acc {
                    None => c,
                    Some(p) => p.alt(c),
                });
            }
            count_expr(scrut, x).seq(acc.unwrap_or_else(Cnt::zero))
        }
        ExprKind::Block(b) => count_block(b, x),
        ExprKind::Return(v) => count_expr(v, x).diverge(),
        ExprKind::Infer { fields, .. } => seq_all(fields.iter().map(|(_, e)| e), x),
        ExprKind::Refine { draft, els, .. } => count_expr(draft, x).seq(Cnt::zero().alt(count_block(els, x))),
    }
}

fn count_stmts(stmts: &[Stmt], tail: Option<&Expr>, x: &str) -> Cnt {
    let mut acc = Cnt::zero();
    for s in stmts {
        match s {
            Stmt::Let { name, value, .. } => {
                acc = acc.seq(count_expr(value, x));
                if name.name == x {
                    return acc; // sombreamento: daqui em diante é outra variável
                }
            }
            Stmt::Expr(e) => acc = acc.seq(count_expr(e, x)),
        }
    }
    if let Some(t) = tail {
        acc = acc.seq(count_expr(t, x));
    }
    acc
}

fn count_block(b: &Block, x: &str) -> Cnt {
    count_stmts(&b.stmts, b.tail.as_deref(), x)
}

/// Spans de cada uso de `x` (para apontar os pontos de consumo nos diagnósticos).
fn uses(stmts: &[Stmt], tail: Option<&Expr>, x: &str, out: &mut Vec<Span>) {
    fn ex(e: &Expr, x: &str, out: &mut Vec<Span>) {
        match &e.kind {
            ExprKind::Var(n) if n == x => out.push(e.span),
            ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Var(_) => {}
            ExprKind::Field(r, _) => ex(r, x, out),
            ExprKind::Call { args, .. } => args.iter().for_each(|a| ex(a, x, out)),
            ExprKind::Method { recv, args, .. } => {
                ex(recv, x, out);
                args.iter().for_each(|a| ex(a, x, out));
            }
            ExprKind::Ctor { fields, .. } => fields.iter().for_each(|(_, a)| ex(a, x, out)),
            ExprKind::Binary(_, l, r) => {
                ex(l, x, out);
                ex(r, x, out);
            }
            ExprKind::Unary(_, v) | ExprKind::Quote(v) | ExprKind::Commit(v) | ExprKind::Return(v) => ex(v, x, out),
            ExprKind::If { cond, then, els } => {
                ex(cond, x, out);
                uses(&then.stmts, then.tail.as_deref(), x, out);
                if let Some(b) = els {
                    uses(&b.stmts, b.tail.as_deref(), x, out);
                }
            }
            ExprKind::Match { scrut, arms } => {
                ex(scrut, x, out);
                for a in arms {
                    if !a.binds.iter().any(|b| b.name == x) {
                        ex(&a.body, x, out);
                    }
                }
            }
            ExprKind::Block(b) => uses(&b.stmts, b.tail.as_deref(), x, out),
            ExprKind::Infer { fields, .. } => fields.iter().for_each(|(_, a)| ex(a, x, out)),
            ExprKind::Refine { draft, els, .. } => {
                ex(draft, x, out);
                uses(&els.stmts, els.tail.as_deref(), x, out);
            }
        }
    }
    for s in stmts {
        match s {
            Stmt::Let { name, value, .. } => {
                ex(value, x, out);
                if name.name == x {
                    return;
                }
            }
            Stmt::Expr(e) => ex(e, x, out),
        }
    }
    if let Some(t) = tail {
        ex(t, x, out);
    }
}

pub fn check_block(b: &Block, facts: &Facts, diags: &mut Vec<Diag>) {
    for (i, s) in b.stmts.iter().enumerate() {
        match s {
            Stmt::Let { name, value, span } => {
                walk(value, facts, diags);
                if !facts.linear_lets.contains(span) {
                    continue;
                }
                let rest = &b.stmts[i + 1..];
                let c = count_stmts(rest, b.tail.as_deref(), &name.name);
                let all: BTreeSet<u8> = c.fall.union(&c.ret).copied().collect();
                let mut spans = Vec::new();
                uses(rest, b.tail.as_deref(), &name.name, &mut spans);
                if all.contains(&2) {
                    let mut d = Diag::new(
                        Stratum::E1,
                        "E1104",
                        "valor linear usado mais de uma vez",
                        name.span,
                        format!("`{}: IdemKey` é consumida em mais de um ponto", name.name),
                    );
                    for (k, sp) in spans.iter().enumerate() {
                        d = d.secondary(*sp, format!("consumo {}", k + 1));
                    }
                    diags.push(
                        d.note("uma chave de idempotência identifica uma única operação lógica")
                            .help("cunhe uma chave por operação: `keys.mint()` em cada chamada"),
                    );
                } else if all.contains(&0) {
                    diags.push(
                        Diag::new(
                            Stratum::E1,
                            "E1105",
                            "valor linear possivelmente descartado",
                            name.span,
                            format!("`{}: IdemKey` não é consumida em todos os caminhos", name.name),
                        )
                        .help("consuma a chave numa chamada em cada caminho, ou não a cunhe"),
                    );
                }
            }
            Stmt::Expr(e) => walk(e, facts, diags),
        }
    }
    if let Some(t) = &b.tail {
        walk(t, facts, diags);
    }
}

fn walk(e: &Expr, facts: &Facts, diags: &mut Vec<Diag>) {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Var(_) => {}
        ExprKind::Field(r, _) => walk(r, facts, diags),
        ExprKind::Call { args, .. } => args.iter().for_each(|a| walk(a, facts, diags)),
        ExprKind::Method { recv, args, .. } => {
            walk(recv, facts, diags);
            args.iter().for_each(|a| walk(a, facts, diags));
        }
        ExprKind::Ctor { fields, .. } => fields.iter().for_each(|(_, a)| walk(a, facts, diags)),
        ExprKind::Binary(_, l, r) => {
            walk(l, facts, diags);
            walk(r, facts, diags);
        }
        ExprKind::Unary(_, v) | ExprKind::Quote(v) | ExprKind::Commit(v) | ExprKind::Return(v) => walk(v, facts, diags),
        ExprKind::If { cond, then, els } => {
            walk(cond, facts, diags);
            check_block(then, facts, diags);
            if let Some(b) = els {
                check_block(b, facts, diags);
            }
        }
        ExprKind::Match { scrut, arms } => {
            walk(scrut, facts, diags);
            arms.iter().for_each(|a| walk(&a.body, facts, diags));
        }
        ExprKind::Block(b) => check_block(b, facts, diags),
        ExprKind::Infer { fields, .. } => fields.iter().for_each(|(_, a)| walk(a, facts, diags)),
        ExprKind::Refine { draft, els, .. } => {
            walk(draft, facts, diags);
            check_block(els, facts, diags);
        }
    }
}
