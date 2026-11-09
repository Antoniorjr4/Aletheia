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
use std::collections::{BTreeSet, HashMap};

/// Limites de iterações de cada laço (`for`, `all`, `any`), indexados pelo span.
type Lb = HashMap<Span, (u32, u32)>;

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

fn seq_all<'a>(it: impl Iterator<Item = &'a Expr>, x: &str, lb: &Lb) -> Cnt {
    it.fold(Cnt::zero(), |acc, e| acc.seq(count_expr(e, x, lb)))
}

fn count_expr(e: &Expr, x: &str, lb: &Lb) -> Cnt {
    match &e.kind {
        ExprKind::Var(n) if n == x => Cnt::one(),
        ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Var(_) => Cnt::zero(),
        ExprKind::Field(r, _) => count_expr(r, x, lb),
        ExprKind::Call { args, .. } => seq_all(args.iter(), x, lb),
        ExprKind::Method { recv, args, .. } => count_expr(recv, x, lb).seq(seq_all(args.iter(), x, lb)),
        ExprKind::Ctor { fields, .. } => seq_all(fields.iter().map(|(_, e)| e), x, lb),
        ExprKind::Binary(_, l, r) => count_expr(l, x, lb).seq(count_expr(r, x, lb)),
        ExprKind::Unary(_, v) | ExprKind::Quote(v) | ExprKind::Commit(v) => count_expr(v, x, lb),
        ExprKind::If { cond, then, els } => {
            let other = els.as_ref().map(|b| count_block(b, x, lb)).unwrap_or_else(Cnt::zero);
            count_expr(cond, x, lb).seq(count_block(then, x, lb).alt(other))
        }
        ExprKind::Match { scrut, arms } => {
            let mut acc: Option<Cnt> = None;
            for a in arms {
                let c = if a.binds.iter().any(|b| b.name == x) { Cnt::zero() } else { count_expr(&a.body, x, lb) };
                acc = Some(match acc {
                    None => c,
                    Some(p) => p.alt(c),
                });
            }
            count_expr(scrut, x, lb).seq(acc.unwrap_or_else(Cnt::zero))
        }
        ExprKind::Block(b) => count_block(b, x, lb),
        ExprKind::Return(v) => count_expr(v, x, lb).diverge(),
        ExprKind::Infer { fields, .. } => seq_all(fields.iter().map(|(_, e)| e), x, lb),
        ExprKind::Refine { draft, els, .. } => count_expr(draft, x, lb).seq(Cnt::zero().alt(count_block(els, x, lb))),
        ExprKind::Atomic { recover, body, on_failure, on_cf } => {
            let mut handlers = Cnt::zero();
            for (bind, b) in on_failure.iter().chain(on_cf.iter()) {
                if bind.name != x {
                    handlers = handlers.alt(count_block(b, x, lb));
                }
            }
            count_expr(recover, x, lb).seq(count_block(body, x, lb)).seq(handlers)
        }
        ExprKind::Await { cond, .. } => count_expr(cond, x, lb),
        // `reconcile p … else { … }` consome `p`; no `else`, `p` volta como outra obrigação.
        ExprKind::Reconcile { pending, els, .. } => {
            if pending.name == x {
                Cnt::one()
            } else {
                Cnt::zero().alt(count_block(els, x, lb))
            }
        }
        ExprKind::Transfer(p) if p.name == x => Cnt::one(),
        ExprKind::Transfer(_) => Cnt::zero(),
        ExprKind::List(items) => seq_all(items.iter(), x, lb),
        // Um laço repete o corpo de `a` a `b` vezes: usar um linear de fora no corpo de um laço
        // com teto ≥ 2 é consumi-lo mais de uma vez.
        ExprKind::For { var, iter, body } => {
            let inner = if var.name == x { Cnt::zero() } else { count_block(body, x, lb) };
            count_expr(iter, x, lb).seq(repeat(inner, lb.get(&e.span).copied()))
        }
        ExprKind::Quant { var, iter, body, .. } => {
            let inner = if var.name == x { Cnt::zero() } else { count_expr(body, x, lb) };
            count_expr(iter, x, lb).seq(repeat(inner, lb.get(&e.span).copied()))
        }
    }
}

/// Contagens de `k` repetições do corpo, para todo `k` entre os limites do laço (saturadas em 2).
fn repeat(body: Cnt, bounds: Option<(u32, u32)>) -> Cnt {
    let (lo, hi) = bounds.unwrap_or((0, 2));
    let (lo, hi) = (lo.min(2), hi.min(2));
    let mut acc = Cnt::zero();
    let mut out: Option<Cnt> = None;
    for k in 0..=hi {
        if k > 0 {
            acc = acc.seq(body.clone());
        }
        if k >= lo {
            out = Some(match out {
                None => acc.clone(),
                Some(o) => o.alt(acc.clone()),
            });
        }
    }
    out.unwrap_or_else(Cnt::zero)
}

fn count_stmts(stmts: &[Stmt], tail: Option<&Expr>, x: &str, lb: &Lb) -> Cnt {
    let mut acc = Cnt::zero();
    for s in stmts {
        match s {
            Stmt::Let { name, value, .. } => {
                acc = acc.seq(count_expr(value, x, lb));
                if name.name == x {
                    return acc; // sombreamento: daqui em diante é outra variável
                }
            }
            Stmt::Expr(e) => acc = acc.seq(count_expr(e, x, lb)),
        }
    }
    if let Some(t) = tail {
        acc = acc.seq(count_expr(t, x, lb));
    }
    acc
}

fn count_block(b: &Block, x: &str, lb: &Lb) -> Cnt {
    count_stmts(&b.stmts, b.tail.as_deref(), x, lb)
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
            ExprKind::Atomic { recover, body, on_failure, on_cf } => {
                ex(recover, x, out);
                uses(&body.stmts, body.tail.as_deref(), x, out);
                for (bind, b) in on_failure.iter().chain(on_cf.iter()) {
                    if bind.name != x {
                        uses(&b.stmts, b.tail.as_deref(), x, out);
                    }
                }
            }
            ExprKind::Await { cond, .. } => ex(cond, x, out),
            ExprKind::Reconcile { pending, .. } if pending.name == x => out.push(e.span),
            ExprKind::Reconcile { els, .. } => uses(&els.stmts, els.tail.as_deref(), x, out),
            ExprKind::Transfer(p) if p.name == x => out.push(e.span),
            ExprKind::Transfer(_) => {}
            ExprKind::List(items) => items.iter().for_each(|a| ex(a, x, out)),
            ExprKind::For { var, iter, body } => {
                ex(iter, x, out);
                if var.name != x {
                    uses(&body.stmts, body.tail.as_deref(), x, out);
                }
            }
            ExprKind::Quant { var, iter, body, .. } => {
                ex(iter, x, out);
                if var.name != x {
                    ex(body, x, out);
                }
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
                let c = count_stmts(rest, b.tail.as_deref(), &name.name, &facts.loop_bounds);
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
                    if spans.len() == 1 {
                        d = d.note(
                            "o consumo fica dentro de um laço com teto ≥ 2: cada iteração consome a chave de novo",
                        );
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
        ExprKind::Atomic { recover, body, on_failure, on_cf } => {
            walk(recover, facts, diags);
            check_block(body, facts, diags);
            for (_, b) in on_failure.iter().chain(on_cf.iter()) {
                check_block(b, facts, diags);
            }
        }
        ExprKind::Await { cond, .. } => walk(cond, facts, diags),
        ExprKind::Reconcile { els, .. } => check_block(els, facts, diags),
        ExprKind::Transfer(_) => {}
        ExprKind::List(items) => items.iter().for_each(|a| walk(a, facts, diags)),
        ExprKind::For { iter, body, .. } => {
            walk(iter, facts, diags);
            check_block(body, facts, diags);
        }
        ExprKind::Quant { iter, body, .. } => {
            walk(iter, facts, diags);
            walk(body, facts, diags);
        }
    }
}

/// Obrigação linear `p: own Pending[_]` no tratador de `TimeoutUnknown`: consumida exatamente
/// uma vez em todo caminho, por `reconcile` ou `transfer`. No `else` de `reconcile`, a obrigação
/// volta (o protocolo esgotou o combustível) e precisa ser consumida de novo.
pub fn check_pending(h: &HandleClause, facts: &Facts, diags: &mut Vec<Diag>) {
    let lb = &facts.loop_bounds;
    let Some(p) = h.params.first() else { return };
    let name = &p.name.name;
    let c = count_block(&h.body, name, lb);
    report_pending(c, p.name.span, name, &h.body, diags, "no tratador");
    fn reconciles<'a>(b: &'a Block, x: &str, out: &mut Vec<&'a Block>) {
        fn ex<'a>(e: &'a Expr, x: &str, out: &mut Vec<&'a Block>) {
            match &e.kind {
                ExprKind::Reconcile { pending, els, .. } => {
                    if pending.name == x {
                        out.push(els);
                    }
                    reconciles(els, x, out);
                }
                ExprKind::Block(b) => reconciles(b, x, out),
                ExprKind::If { then, els, .. } => {
                    reconciles(then, x, out);
                    if let Some(b) = els {
                        reconciles(b, x, out);
                    }
                }
                _ => {}
            }
        }
        for s in &b.stmts {
            let (Stmt::Let { value: e, .. } | Stmt::Expr(e)) = s;
            ex(e, x, out);
        }
        if let Some(t) = &b.tail {
            ex(t, x, out);
        }
    }
    let mut elses = Vec::new();
    reconciles(&h.body, name, &mut elses);
    for els in elses {
        report_pending(count_block(els, name, lb), els.span, name, els, diags, "no `else` de `reconcile`");
    }
}

fn report_pending(c: Cnt, at: Span, name: &str, b: &Block, diags: &mut Vec<Diag>, place: &str) {
    let all: BTreeSet<u8> = c.fall.union(&c.ret).copied().collect();
    let mut spans = Vec::new();
    uses(&b.stmts, b.tail.as_deref(), name, &mut spans);
    if all.contains(&2) {
        let mut d = Diag::new(
            Stratum::E1,
            "E1104",
            "valor linear usado mais de uma vez",
            at,
            format!("`{name}: Pending[k]` é consumido em mais de um ponto {place}"),
        );
        for (k, sp) in spans.iter().enumerate() {
            d = d.secondary(*sp, format!("consumo {}", k + 1));
        }
        diags.push(d.note("uma operação em aberto tem um único destino: reconciliada ou transferida"));
    } else if all.contains(&0) {
        diags.push(
            Diag::new(
                Stratum::E1,
                "E1107",
                "valor linear descartado: Pending[k]",
                at,
                format!("`{name}` não é consumido em todos os caminhos {place}"),
            )
            .note("descartar a obrigação deixaria a chave em `k ↦ ?`: ninguém saberia se o efeito aconteceu")
            .help(format!("consuma `{name}` com `reconcile {name} fuel n else {{ … }}` ou `transfer {name}`")),
        );
    }
}
