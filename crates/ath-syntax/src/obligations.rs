//! Rev13, M-I: **efeitos com prova**. O contrato `tool … requires φ` deixa de ser só um monitor
//! na fronteira do efeito: o compilador rastreia, pelo fluxo de dados, de onde vem cada argumento
//! que `φ` usa. Se todos vêm, sem transformação, de campos de um mesmo elemento de um valor
//! endossado por `refine`, a obrigação é **levantada** para esse `refine`: o runtime a confere
//! sobre o valor proposto, junto com a política, e uma proposta que a viola é recusada ali —
//! com retentativa e `else`, como uma recusa da política. No efeito, a obrigação está então
//! **descarregada**: `Precondições ∧ fatos demonstrados ⇒ φ`, porque
//!
//! * os argumentos do efeito são exatamente os campos conferidos (valores imutáveis, ligados por
//!   `let`, `match` e `for`, nada calculado no caminho);
//! * `φ` é pura e só lê os parâmetros e a fotografia confiável `s`, que é uma só na execução;
//! * todo valor que chega ao efeito por esse caminho passou pelo `refine` que conferiu `φ` — ou
//!   vem de um literal que não contém o elemento (uma lista vazia: a obrigação é vazia).
//!
//! O rótulo `Endorsed` sozinho **não** carrega a obrigação: um argumento endossado e depois
//! transformado (`amount ++ "0"`, `n + 64000`) não tem `φ` demonstrada, e a análise o reporta
//! como lacuna dinâmica — o monitor do efeito continua lá (`Contracts.lean`, `amplified`).

use crate::ast::*;
use crate::span::Span;
use std::collections::{BTreeMap, BTreeSet};

/// Um passo de um caminho dentro de um valor: o campo `f` da variante `V`, ou um elemento de lista.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    Field(String, String),
    Elem,
}

/// De onde vem um valor.
#[derive(Clone, Debug, PartialEq)]
enum Src {
    /// do valor devolvido pelo `refine` do sítio, no caminho
    Refine(Span, Vec<Step>),
    /// de um literal de construtor: a variante e as fontes de cada campo
    Lit(String, Vec<(String, Vec<Src>)>),
    /// de uma lista literal: as fontes de cada elemento
    List(Vec<Vec<Src>>),
    /// de qualquer outra coisa (um cálculo, uma inferência, um parâmetro do agente…)
    Unknown(String),
}

fn extend(srcs: &[Src], step: &Step) -> Vec<Src> {
    let mut out = Vec::new();
    for s in srcs {
        match (s, step) {
            (Src::Refine(sp, p), _) => {
                let mut p = p.clone();
                p.push(step.clone());
                out.push(Src::Refine(*sp, p));
            }
            (Src::Lit(v, fields), Step::Field(vv, f)) => {
                if v == vv {
                    if let Some((_, fs)) = fields.iter().find(|(n, _)| n == f) {
                        out.extend(fs.iter().cloned());
                    }
                }
                // outra variante: o braço não casa com este literal; nenhum valor chega
            }
            (Src::List(items), Step::Elem) => {
                for it in items {
                    out.extend(it.iter().cloned());
                }
            }
            (Src::Unknown(r), _) => out.push(Src::Unknown(r.clone())),
            _ => out.push(Src::Unknown("forma incompatível".into())),
        }
    }
    out
}

/// Uma obrigação levantada para um `refine`: para cada elemento do valor endossado no caminho
/// `path` que for da variante `variant`, o contrato de `tool` com os parâmetros ligados aos
/// campos (`binds`: parâmetro ← campo).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lifted {
    pub tool: String,
    pub path: Vec<Step>,
    pub variant: String,
    pub binds: Vec<(String, String)>,
}

/// O destino da obrigação num sítio de chamada.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// demonstrada: levantada para os `refine` listados (ou vazia)
    Discharged { refines: Vec<Span> },
    /// não demonstrável estaticamente: o monitor do efeito confere em runtime
    Dynamic { reason: String },
}

#[derive(Clone, Debug)]
pub struct Site {
    pub call: Span,
    pub tool: String,
    pub status: Status,
}

#[derive(Clone, Debug, Default)]
pub struct Obligations {
    /// por sítio de `refine`, as obrigações levantadas (sem repetição, em ordem)
    pub lifted: BTreeMap<(u32, u32), Vec<Lifted>>,
    /// por sítio de chamada a uma ferramenta com `requires`
    pub sites: Vec<Site>,
}

impl Obligations {
    pub fn lifted_at(&self, s: Span) -> &[Lifted] {
        self.lifted.get(&(s.lo, s.hi)).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn discharged(&self, call: Span) -> bool {
        self.sites.iter().any(|x| x.call == call && matches!(x.status, Status::Discharged { .. }))
    }
}

/// Os nomes livres de uma expressão (as variáveis que ela lê).
fn free_vars(e: &Expr, out: &mut BTreeSet<String>) {
    match &e.kind {
        ExprKind::Var(x) => {
            out.insert(x.clone());
        }
        ExprKind::Field(r, _)
        | ExprKind::Unary(_, r)
        | ExprKind::Quote(r)
        | ExprKind::Commit(r)
        | ExprKind::Return(r) => free_vars(r, out),
        ExprKind::Call { args, .. } => args.iter().for_each(|a| free_vars(a, out)),
        ExprKind::Method { recv, args, .. } => {
            free_vars(recv, out);
            args.iter().for_each(|a| free_vars(a, out));
        }
        ExprKind::Binary(_, a, b) => {
            free_vars(a, out);
            free_vars(b, out);
        }
        ExprKind::List(xs) => xs.iter().for_each(|a| free_vars(a, out)),
        ExprKind::Ctor { fields, .. } => fields.iter().for_each(|(_, a)| free_vars(a, out)),
        _ => {}
    }
}

struct An<'m> {
    tools: BTreeMap<&'m str, &'m ToolDecl>,
    out: Obligations,
}

type Env = Vec<(String, Vec<Src>)>;

/// Por sítio de `refine`: o caminho até o elemento, a variante e as ligações parâmetro ← campo.
type Lift = Option<(Vec<Step>, String, Vec<(String, String)>)>;

fn lookup(env: &Env, x: &str) -> Vec<Src> {
    env.iter()
        .rev()
        .find(|(n, _)| n == x)
        .map(|(_, s)| s.clone())
        .unwrap_or_else(|| vec![Src::Unknown(format!("`{x}` não vem de um valor endossado"))])
}

impl<'m> An<'m> {
    fn block(&mut self, b: &Block, env: &mut Env) -> Vec<Src> {
        let n = env.len();
        for s in &b.stmts {
            match s {
                Stmt::Let { name, value, .. } => {
                    let v = self.expr(value, env);
                    env.push((name.name.clone(), v));
                }
                Stmt::Expr(e) => {
                    self.expr(e, env);
                }
            }
        }
        let r = match &b.tail {
            Some(t) => self.expr(t, env),
            None => vec![Src::Unknown("bloco sem valor".into())],
        };
        env.truncate(n);
        r
    }

    fn expr(&mut self, e: &Expr, env: &mut Env) -> Vec<Src> {
        match &e.kind {
            ExprKind::Var(x) => lookup(env, x),
            ExprKind::Refine { draft, els, .. } => {
                self.expr(draft, env);
                let mut r = vec![Src::Refine(e.span, Vec::new())];
                r.extend(self.block(els, env));
                r
            }
            ExprKind::Block(b) => self.block(b, env),
            ExprKind::If { cond, then, els } => {
                self.expr(cond, env);
                let mut r = self.block(then, env);
                match els {
                    Some(b) => r.extend(self.block(b, env)),
                    None => r.push(Src::Unknown("`if` sem `else`".into())),
                }
                r
            }
            ExprKind::Match { scrut, arms } => {
                let s = self.expr(scrut, env);
                let mut r = Vec::new();
                for a in arms {
                    let n = env.len();
                    for b in &a.binds {
                        env.push((b.name.clone(), extend(&s, &Step::Field(a.variant.name.clone(), b.name.clone()))));
                    }
                    r.extend(self.expr(&a.body, env));
                    env.truncate(n);
                }
                r
            }
            ExprKind::For { var, iter, body } => {
                let s = self.expr(iter, env);
                env.push((var.name.clone(), extend(&s, &Step::Elem)));
                self.block(body, env);
                env.pop();
                vec![Src::Unknown("o valor de um laço".into())]
            }
            ExprKind::Ctor { name, fields } => {
                let fs = fields.iter().map(|(f, x)| (f.name.clone(), self.expr(x, env))).collect();
                vec![Src::Lit(name.name.clone(), fs)]
            }
            ExprKind::List(items) => vec![Src::List(items.iter().map(|x| self.expr(x, env)).collect())],
            ExprKind::Commit(x) => self.expr(x, env),
            ExprKind::Call { callee, args } => {
                let srcs: Vec<Vec<Src>> = args.iter().map(|a| self.expr(a, env)).collect();
                if let Some(t) = self.tools.get(callee.name.as_str()).copied() {
                    if let Some(req) = &t.requires {
                        let status = self.site(t, req, &srcs);
                        self.out.sites.push(Site { call: e.span, tool: t.name.name.clone(), status });
                    }
                }
                vec![Src::Unknown(format!("o resultado de `{}`", callee.name))]
            }
            ExprKind::Atomic { recover, body, on_failure, on_cf } => {
                self.expr(recover, env);
                self.block(body, env);
                for (x, b) in on_failure.iter().chain(on_cf.iter()) {
                    env.push((x.name.clone(), vec![Src::Unknown("a causa de uma falha".into())]));
                    self.block(b, env);
                    env.pop();
                }
                vec![Src::Unknown("o valor de uma saga".into())]
            }
            ExprKind::Reconcile { els, .. } => {
                self.block(els, env);
                vec![Src::Unknown("reconciliação".into())]
            }
            // `return` diverge: o ramo não produz valor algum para quem o cerca. Um
            // `refine … else { return … }` só tem a origem endossada.
            ExprKind::Return(r) => {
                self.expr(r, env);
                Vec::new()
            }
            other => {
                // percorre os filhos, para achar chamadas aninhadas; o valor é calculado
                match other {
                    ExprKind::Field(r, _) | ExprKind::Unary(_, r) | ExprKind::Quote(r) => {
                        self.expr(r, env);
                    }
                    ExprKind::Binary(_, a, b) => {
                        self.expr(a, env);
                        self.expr(b, env);
                    }
                    ExprKind::Method { recv, args, .. } => {
                        self.expr(recv, env);
                        args.iter().for_each(|a| {
                            self.expr(a, env);
                        });
                    }
                    ExprKind::Infer { fields, .. } => fields.iter().for_each(|(_, a)| {
                        self.expr(a, env);
                    }),
                    _ => {}
                }
                vec![Src::Unknown("um valor calculado".into())]
            }
        }
    }

    /// Decide o sítio: todos os parâmetros que `φ` lê vêm de campos do mesmo elemento de um
    /// valor endossado (em cada `refine` que alcança o sítio)? Então levanta.
    fn site(&mut self, t: &ToolDecl, req: &Expr, args: &[Vec<Src>]) -> Status {
        let mut used = BTreeSet::new();
        free_vars(req, &mut used);
        let params: Vec<(usize, &str)> = t
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.name.name.as_str()))
            .filter(|(_, n)| used.contains(*n))
            .collect();
        // por sítio de `refine`: (prefixo, variante) → parâmetro ← campo
        let mut per: BTreeMap<(u32, u32), Lift> = BTreeMap::new();
        for (i, name) in &params {
            for s in args.get(*i).into_iter().flatten() {
                match s {
                    Src::Refine(sp, path) => {
                        let Some((Step::Field(v, f), prefix)) = path.split_last() else {
                            return Status::Dynamic {
                                reason: format!("`{name}` é o valor endossado inteiro, não um campo"),
                            };
                        };
                        let e = per.entry((sp.lo, sp.hi)).or_insert(None);
                        match e {
                            None => *e = Some((prefix.to_vec(), v.clone(), vec![(name.to_string(), f.clone())])),
                            Some((p0, v0, b)) => {
                                if p0.as_slice() != prefix || v0 != v {
                                    return Status::Dynamic {
                                        reason: format!("`{name}` vem de outro elemento do valor endossado"),
                                    };
                                }
                                if !b.iter().any(|(n, _)| n == name) {
                                    b.push((name.to_string(), f.clone()));
                                }
                            }
                        }
                    }
                    Src::Unknown(r) => return Status::Dynamic { reason: format!("`{name}`: {r}") },
                    Src::Lit(..) | Src::List(..) => {
                        return Status::Dynamic { reason: format!("`{name}` vem de um literal") };
                    }
                }
            }
        }
        // cada parâmetro precisa estar ligado em cada `refine` que alcança o sítio
        for e in per.values() {
            let Some((_, _, b)) = e else { continue };
            if b.len() != params.len() {
                return Status::Dynamic { reason: "parte dos argumentos não vem do mesmo endosso".into() };
            }
        }
        let mut refines = Vec::new();
        for ((lo, hi), e) in per {
            let Some((path, variant, mut binds)) = e else { continue };
            binds.sort();
            let l = Lifted { tool: t.name.name.clone(), path, variant, binds };
            let v = self.out.lifted.entry((lo, hi)).or_default();
            if !v.contains(&l) {
                v.push(l);
            }
            refines.push(Span { lo, hi });
        }
        Status::Discharged { refines }
    }
}

/// A análise do módulo: as obrigações levantadas por `refine` e o destino de cada sítio.
pub fn analyze(m: &Module) -> Obligations {
    let mut an = An { tools: BTreeMap::new(), out: Obligations::default() };
    for it in &m.items {
        if let Item::Tool(t) = it {
            an.tools.insert(t.name.name.as_str(), t);
        }
    }
    for it in &m.items {
        if let Item::Agent(a) = it {
            for h in &a.handlers {
                let mut env: Env = Vec::new();
                an.block(&h.body, &mut env);
            }
            for h in &a.handles {
                let mut env: Env = Vec::new();
                an.block(&h.body, &mut env);
            }
        }
    }
    an.out
}
