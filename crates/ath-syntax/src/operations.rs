//! **Protótipo de diagnóstico, sem efeito na aceitação** (27/11/2026): a integridade da operação
//! endossada.
//!
//! Pergunta: numa chamada de ferramenta, os argumentos que vêm de um valor endossado por `refine`
//! vêm todos da **mesma operação aprovada**? Uma operação é um elemento de lista endossada, ou o
//! valor endossado inteiro quando não há lista. Ela é identificada como na análise de obrigações:
//! o `refine`, o caminho até o elemento e o último ponto de escolha (`for`, ou `if`/`match` cujos
//! ramos têm origens diferentes).
//!
//! Diferenças para a análise de obrigações:
//!
//! * vale para **toda** chamada de ferramenta, com ou sem `requires`;
//! * um valor calculado (`a ++ b`, `fit(x, n)`, aritmética) **não** perde a origem: herda a união
//!   das origens de quem o formou e é marcado como transformado. Assim, `fit(a.subject, 9) ++
//!   b.amount` ainda conta como mistura de duas operações.
//!
//! Achados por sítio:
//!
//! * `mistura`: os argumentos vêm de duas ou mais operações aprovadas distintas. É o caso X2;
//! * `transformada`: os argumentos vêm de uma só operação, mas algum passou por um cálculo. Isso
//!   não é mistura, mas também não é a operação aprovada intacta.
//!
//! Nada aqui muda o que o compilador aceita: é a medida para decidir se a regra proposta (mistura
//! exige autorização explícita) bloquearia programas legítimos.

use crate::ast::*;
use crate::span::Span;
use std::collections::BTreeSet;

/// Uma operação aprovada: (`refine`, caminho até o elemento, último ponto de escolha).
pub type Op = ((u32, u32), Vec<String>, Option<(u32, u32)>);

#[derive(Clone, Debug, PartialEq)]
struct Src {
    refine: (u32, u32),
    path: Vec<String>,
    ids: Vec<(u32, u32)>,
    transformed: bool,
}

impl Src {
    fn op(&self) -> Op {
        // o elemento: o caminho até o último `[*]` (ou o valor inteiro, sem lista)
        let cut = self.path.iter().rposition(|p| p == "[*]").map(|i| i + 1).unwrap_or(0);
        // com um ponto de escolha, ele identifica o valor sozinho: `refine A else { refine B }`
        // seguido de `for w in …` produz um elemento por iteração, venha de A ou de B
        match self.ids.last() {
            Some(id) => ((0, 0), self.path[..cut].to_vec(), Some(*id)),
            None => (self.refine, self.path[..cut].to_vec(), None),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Finding {
    /// uma operação só, intacta (ou nenhuma origem endossada)
    Uma,
    /// sem argumento vindo de valor endossado
    SemOrigem,
    Mistura(Vec<Op>),
    Transformada,
}

#[derive(Clone, Debug)]
pub struct Site {
    pub call: Span,
    pub tool: String,
    pub finding: Finding,
}

type Env = Vec<(String, Vec<Src>)>;

struct An<'m> {
    tools: BTreeSet<&'m str>,
    out: Vec<Site>,
}

fn step(srcs: &[Src], s: &str, elem: Option<(u32, u32)>) -> Vec<Src> {
    srcs.iter()
        .map(|x| {
            let mut y = x.clone();
            y.path.push(s.to_string());
            if let Some(id) = elem {
                y.ids.push(id);
            }
            y
        })
        .collect()
}

fn calc(parts: Vec<Vec<Src>>) -> Vec<Src> {
    let mut out: Vec<Src> = Vec::new();
    for p in parts.into_iter().flatten() {
        let q = Src { transformed: true, ..p };
        if !out.contains(&q) {
            out.push(q);
        }
    }
    out
}

fn join(branches: Vec<Vec<Src>>, at: Span) -> Vec<Src> {
    let dedup = |v: Vec<Src>| {
        v.into_iter().fold(Vec::new(), |mut acc: Vec<Src>, s| {
            if !acc.contains(&s) {
                acc.push(s);
            }
            acc
        })
    };
    let sets: Vec<Vec<Src>> = branches.into_iter().map(dedup).filter(|b| !b.is_empty()).collect();
    let same = sets.windows(2).all(|w| w[0].len() == w[1].len() && w[0].iter().all(|s| w[1].contains(s)));
    let mut srcs = dedup(sets.into_iter().flatten().collect());
    if !same {
        for s in &mut srcs {
            s.ids.push((at.lo, at.hi));
        }
    }
    srcs
}

impl<'m> An<'m> {
    fn block(&mut self, b: &Block, env: &mut Env) -> Vec<Src> {
        let n = env.len();
        for s in &b.stmts {
            match s {
                Stmt::Let { name, value, .. } => {
                    let v = self.expr(value, env);
                    env.push((name.name.clone(), v));
                    if let ExprKind::Refine { keep: Some(Keep { bind: Some(b), .. }), .. } = &value.kind {
                        env.push((b.name.clone(), Vec::new()));
                    }
                }
                Stmt::Expr(e) => {
                    self.expr(e, env);
                }
            }
        }
        let r = b.tail.as_ref().map(|t| self.expr(t, env)).unwrap_or_default();
        env.truncate(n);
        r
    }

    fn expr(&mut self, e: &Expr, env: &mut Env) -> Vec<Src> {
        match &e.kind {
            ExprKind::Var(x) => env.iter().rev().find(|(n, _)| n == x).map(|(_, s)| s.clone()).unwrap_or_default(),
            ExprKind::Refine { draft, els, .. } => {
                self.expr(draft, env);
                let me =
                    vec![Src { refine: (e.span.lo, e.span.hi), path: Vec::new(), ids: Vec::new(), transformed: false }];
                let r = vec![me, self.block(els, env)];
                join(r, e.span)
            }
            ExprKind::Block(b) => self.block(b, env),
            ExprKind::If { cond, then, els } => {
                self.expr(cond, env);
                let mut r = vec![self.block(then, env)];
                if let Some(b) = els {
                    r.push(self.block(b, env));
                }
                join(r, e.span)
            }
            ExprKind::Match { scrut, arms } => {
                let s = self.expr(scrut, env);
                let mut r = Vec::new();
                for a in arms {
                    let n = env.len();
                    for (b, f) in a.binds.iter().zip(&a.fields) {
                        env.push((b.name.clone(), step(&s, &format!("{}.{}", a.variant.name, f.name), None)));
                    }
                    r.push(self.expr(&a.body, env));
                    env.truncate(n);
                }
                join(r, e.span)
            }
            ExprKind::For { var, iter, body } => {
                let s = self.expr(iter, env);
                env.push((var.name.clone(), step(&s, "[*]", Some((var.span.lo, var.span.hi)))));
                self.block(body, env);
                env.pop();
                Vec::new()
            }
            ExprKind::List(items) => {
                // `for x in [a, b]` escolhe um item: os itens levam o caminho de lista literal
                let parts: Vec<Vec<Src>> = items.iter().map(|x| self.expr(x, env)).collect();
                parts
                    .into_iter()
                    .flatten()
                    .map(|s| Src {
                        path: {
                            let mut p = s.path.clone();
                            p.push("[lit]".into());
                            p
                        },
                        ..s
                    })
                    .collect()
            }
            ExprKind::Ctor { fields, .. } => {
                // reconstruir um valor a partir de campos: a origem dos campos continua, transformada
                calc(fields.iter().map(|(_, x)| self.expr(x, env)).collect())
            }
            ExprKind::Commit(x) => self.expr(x, env),
            ExprKind::Return(x) => {
                self.expr(x, env);
                Vec::new()
            }
            ExprKind::Field(r, f) => step(&self.expr(r, env), &format!("?.{}", f.name), None),
            ExprKind::Call { callee, args } => {
                let srcs: Vec<Vec<Src>> = args.iter().map(|a| self.expr(a, env)).collect();
                if self.tools.contains(callee.name.as_str()) {
                    let finding = finding(&srcs);
                    self.out.push(Site { call: e.span, tool: callee.name.clone(), finding });
                    Vec::new()
                } else {
                    calc(srcs)
                }
            }
            ExprKind::Method { recv, args, .. } => {
                let mut parts = vec![self.expr(recv, env)];
                parts.extend(args.iter().map(|a| self.expr(a, env)));
                calc(parts)
            }
            ExprKind::Binary(_, a, b) => {
                let (x, y) = (self.expr(a, env), self.expr(b, env));
                calc(vec![x, y])
            }
            ExprKind::Unary(_, a) | ExprKind::Quote(a) => {
                let x = self.expr(a, env);
                calc(vec![x])
            }
            ExprKind::Infer { fields, .. } => {
                fields.iter().for_each(|(_, a)| {
                    self.expr(a, env);
                });
                Vec::new()
            }
            ExprKind::Atomic { recover, body, on_failure, on_cf } => {
                self.expr(recover, env);
                self.block(body, env);
                for (x, b) in on_failure.iter().chain(on_cf.iter()) {
                    env.push((x.name.clone(), Vec::new()));
                    self.block(b, env);
                    env.pop();
                }
                Vec::new()
            }
            ExprKind::Reconcile { els, .. } => {
                self.block(els, env);
                Vec::new()
            }
            ExprKind::Quant { iter, body, var, .. } => {
                let s = self.expr(iter, env);
                env.push((var.name.clone(), step(&s, "[*]", Some((var.span.lo, var.span.hi)))));
                let b = self.expr(body, env);
                env.pop();
                calc(vec![s, b])
            }
            _ => Vec::new(),
        }
    }
}

fn finding(args: &[Vec<Src>]) -> Finding {
    let all: Vec<&Src> = args.iter().flatten().collect();
    if all.is_empty() {
        return Finding::SemOrigem;
    }
    let ops: BTreeSet<Op> = all.iter().map(|s| s.op()).collect();
    if ops.len() > 1 {
        return Finding::Mistura(ops.into_iter().collect());
    }
    if all.iter().any(|s| s.transformed) {
        return Finding::Transformada;
    }
    Finding::Uma
}

/// Os sítios de chamada de ferramenta do módulo, com o achado de cada um.
pub fn analyze(m: &Module) -> Vec<Site> {
    let mut an = An { tools: BTreeSet::new(), out: Vec::new() };
    for it in &m.items {
        if let Item::Tool(t) = it {
            an.tools.insert(t.name.name.as_str());
        }
    }
    for it in &m.items {
        if let Item::Agent(a) = it {
            for h in &a.handlers {
                an.block(&h.body, &mut Vec::new());
            }
            for h in &a.handles {
                an.block(&h.body, &mut Vec::new());
            }
        }
    }
    an.out
}
