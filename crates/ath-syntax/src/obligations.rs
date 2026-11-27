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
    /// do valor devolvido pelo `refine` do sítio, no caminho. O terceiro campo são os pontos de
    /// escolha pelos quais o valor passou, em ordem: o laço que escolheu um elemento (o span da
    /// variável do `for`, B1) e cada `if`/`match` que juntou valores de origens diferentes (o span
    /// da expressão, F2). Dois valores vêm do mesmo elemento, na mesma execução, se têm o mesmo
    /// caminho e o mesmo **último** ponto de escolha: esse ponto produz um valor só de cada vez.
    Refine(Span, Vec<Step>, Vec<(u32, u32)>),
    /// de um literal de construtor: a variante e as fontes de cada campo
    Lit(String, Vec<(String, Vec<Src>)>),
    /// de uma lista literal: as fontes de cada elemento
    List(Vec<Vec<Src>>),
    /// de qualquer outra coisa (um cálculo, uma inferência, um parâmetro do agente…)
    Unknown(String),
}

fn extend(srcs: &[Src], step: &Step) -> Vec<Src> {
    extend_at(srcs, step, None)
}

/// Um `if` ou `match` cujo valor vem de mais de uma origem é um ponto de escolha: o valor que sai
/// dele é um só, mas não se sabe estaticamente qual. Marca as origens endossadas com o span.
fn join(branches: Vec<Vec<Src>>, at: Span) -> Vec<Src> {
    fn dedup(v: Vec<Src>) -> Vec<Src> {
        v.into_iter().fold(Vec::new(), |mut acc: Vec<Src>, s| {
            if !acc.contains(&s) {
                acc.push(s);
            }
            acc
        })
    }
    let sets: Vec<Vec<Src>> = branches.into_iter().map(dedup).filter(|b| !b.is_empty()).collect();
    // ramos com o mesmo conjunto de origens (ou um ramo só, como um `match` de um braço): não há escolha
    let same = sets.windows(2).all(|w| w[0].len() == w[1].len() && w[0].iter().all(|s| w[1].contains(s)));
    let mut srcs = dedup(sets.into_iter().flatten().collect());
    if !same {
        for s in &mut srcs {
            if let Src::Refine(_, _, ids) = s {
                ids.push((at.lo, at.hi));
            }
        }
    }
    srcs
}

/// Como `extend`; num passo `Elem`, `elem` é o laço que escolhe o elemento.
fn extend_at(srcs: &[Src], step: &Step, elem: Option<(u32, u32)>) -> Vec<Src> {
    let mut out = Vec::new();
    for s in srcs {
        match (s, step) {
            (Src::Refine(sp, p, ids), _) => {
                let mut p = p.clone();
                p.push(step.clone());
                let mut ids = ids.clone();
                if let (Step::Elem, Some(id)) = (step, elem) {
                    ids.push(id);
                }
                out.push(Src::Refine(*sp, p, ids));
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
                // o laço escolhe um item da lista literal de cada vez: os itens endossados recebem o
                // ponto de escolha do laço, como um elemento de lista endossada (`for x in [a, b]`)
                for it in items {
                    for src in it {
                        match (src, elem) {
                            (Src::Refine(sp, p, ids), Some(id)) => {
                                let mut ids = ids.clone();
                                ids.push(id);
                                out.push(Src::Refine(*sp, p.clone(), ids));
                            }
                            _ => out.push(src.clone()),
                        }
                    }
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

/// Os nomes que uma expressão lê, sobre-aproximados: todo `Var` em qualquer subexpressão, inclusive
/// dentro de `if`, `match`, blocos e quantificadores (F1, revisão de 27/11: a versão anterior não
/// descia nessas formas, e um parâmetro lido só ali ficava fora da obrigação). Devolve `false` se
/// encontrar uma forma que não sabe percorrer; quem chama trata então todos os parâmetros como lidos
/// (por isso parar no primeiro `false` não perde nada).
fn free_vars(e: &Expr, out: &mut BTreeSet<String>) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) => true,
        ExprKind::Var(x) => {
            out.insert(x.clone());
            true
        }
        ExprKind::Field(r, _)
        | ExprKind::Unary(_, r)
        | ExprKind::Quote(r)
        | ExprKind::Commit(r)
        | ExprKind::Return(r) => free_vars(r, out),
        ExprKind::Call { args, .. } => args.iter().all(|a| free_vars(a, out)),
        ExprKind::Method { recv, args, .. } => free_vars(recv, out) && args.iter().all(|a| free_vars(a, out)),
        ExprKind::Binary(_, a, b) => free_vars(a, out) & free_vars(b, out),
        ExprKind::List(xs) => xs.iter().all(|a| free_vars(a, out)),
        ExprKind::Ctor { fields, .. } => fields.iter().all(|(_, a)| free_vars(a, out)),
        ExprKind::If { cond, then, els } => {
            let mut ok = free_vars(cond, out) & block_vars(then, out);
            if let Some(b) = els {
                ok &= block_vars(b, out);
            }
            ok
        }
        ExprKind::Match { scrut, arms } => free_vars(scrut, out) && arms.iter().all(|a| free_vars(&a.body, out)),
        ExprKind::Block(b) => block_vars(b, out),
        ExprKind::Quant { iter, body, .. } => free_vars(iter, out) & free_vars(body, out),
        _ => false,
    }
}

fn block_vars(b: &Block, out: &mut BTreeSet<String>) -> bool {
    let mut ok = true;
    for s in &b.stmts {
        ok &= match s {
            Stmt::Let { value, .. } => free_vars(value, out),
            Stmt::Expr(e) => free_vars(e, out),
        };
    }
    if let Some(t) = &b.tail {
        ok &= free_vars(t, out);
    }
    ok
}

struct An<'m> {
    tools: BTreeMap<&'m str, &'m ToolDecl>,
    out: Obligations,
}

type Env = Vec<(String, Vec<Src>)>;

/// Por sítio de `refine`: o caminho até o elemento, a variante e as ligações parâmetro ← campo.
type Lift = Option<(Vec<Step>, Vec<(u32, u32)>, String, Vec<(String, String)>)>;

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
                    // `keeping … as nome` liga também o que a política recusou (como no checker e
                    // nos runtimes: só quando o `refine` é o valor desta ligação). Esses elementos
                    // não passaram pela política nem pela obrigação levantada: origem desconhecida,
                    // e a nova ligação sombreia qualquer `nome` de fora (B2).
                    if let ExprKind::Refine { keep: Some(Keep { bind: Some(b), .. }), .. } = &value.kind {
                        env.push((
                            b.name.clone(),
                            vec![Src::Unknown(format!("`{}` é o que a política recusou no `refine`", b.name))],
                        ));
                    }
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
                let r = vec![vec![Src::Refine(e.span, Vec::new(), Vec::new())], self.block(els, env)];
                join(r, e.span)
            }
            ExprKind::Block(b) => self.block(b, env),
            ExprKind::If { cond, then, els } => {
                self.expr(cond, env);
                let mut r = vec![self.block(then, env)];
                match els {
                    Some(b) => r.push(self.block(b, env)),
                    None => r.push(vec![Src::Unknown("`if` sem `else`".into())]),
                }
                join(r, e.span)
            }
            ExprKind::Match { scrut, arms } => {
                let s = self.expr(scrut, env);
                let mut r = Vec::new();
                for a in arms {
                    let n = env.len();
                    for (b, f) in a.binds.iter().zip(&a.fields) {
                        env.push((b.name.clone(), extend(&s, &Step::Field(a.variant.name.clone(), f.name.clone()))));
                    }
                    r.push(self.expr(&a.body, env));
                    env.truncate(n);
                }
                join(r, e.span)
            }
            ExprKind::For { var, iter, body } => {
                let s = self.expr(iter, env);
                env.push((var.name.clone(), extend_at(&s, &Step::Elem, Some((var.span.lo, var.span.hi)))));
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
        // forma que a análise não sabe percorrer: todos os parâmetros contam como lidos (F1)
        let complete = free_vars(req, &mut used);
        let params: Vec<(usize, &str)> = t
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.name.name.as_str()))
            .filter(|(_, n)| !complete || used.contains(*n))
            .collect();
        // `φ` não lê parâmetro nenhum: nenhum valor endossado a carrega, e nada a confere antes do
        // efeito (só a fotografia `s` decide) — o monitor do efeito é quem confere
        if params.is_empty() {
            return Status::Dynamic {
                reason: "o contrato não lê parâmetro algum: nenhum `refine` o confere".into()
            };
        }
        // por sítio de `refine`: (prefixo, variante) → parâmetro ← campo
        let mut per: BTreeMap<(u32, u32), Lift> = BTreeMap::new();
        for (i, name) in &params {
            for s in args.get(*i).into_iter().flatten() {
                match s {
                    Src::Refine(sp, path, ids) => {
                        let Some((Step::Field(v, f), prefix)) = path.split_last() else {
                            return Status::Dynamic {
                                reason: format!("`{name}` é o valor endossado inteiro, não um campo"),
                            };
                        };
                        let e = per.entry((sp.lo, sp.hi)).or_insert(None);
                        match e {
                            None => {
                                *e =
                                    Some((prefix.to_vec(), ids.clone(), v.clone(), vec![(name.to_string(), f.clone())]))
                            }
                            Some((p0, ids0, v0, b)) => {
                                // mesmo caminho não basta: o último ponto de escolha tem de ser o mesmo,
                                // senão são elementos diferentes da mesma lista (B1) ou valores de
                                // propostas diferentes juntados por `if`/`match` (F2)
                                if p0.as_slice() != prefix || ids0.last() != ids.last() || v0 != v {
                                    return Status::Dynamic {
                                        reason: format!("`{name}` vem de outro elemento do valor endossado"),
                                    };
                                }
                                match b.iter().find(|(n, _)| n == name) {
                                    None => b.push((name.to_string(), f.clone())),
                                    Some((_, f0)) if f0 != f => {
                                        return Status::Dynamic {
                                            reason: format!(
                                                "`{name}` vem de campos diferentes (`{f0}`, `{f}`) do valor endossado"
                                            ),
                                        };
                                    }
                                    Some(_) => {}
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
            let Some((_, _, _, b)) = e else { continue };
            if b.len() != params.len() {
                return Status::Dynamic { reason: "parte dos argumentos não vem do mesmo endosso".into() };
            }
        }
        // mais de um `refine` alcança o sítio: na execução, os argumentos só vêm do mesmo valor se
        // todos passaram pelo mesmo ponto de escolha, que decide de qual `refine` o valor vem (F2)
        if per.len() > 1 {
            let lasts: BTreeSet<Option<(u32, u32)>> =
                per.values().flatten().map(|(_, ids, _, _)| ids.last().copied()).collect();
            if lasts.len() != 1 || lasts.contains(&None) {
                return Status::Dynamic {
                    reason: "os argumentos podem vir de propostas diferentes (mais de um `refine` alcança a chamada)"
                        .into(),
                };
            }
        }
        let mut refines = Vec::new();
        for ((lo, hi), e) in per {
            let Some((path, _, variant, mut binds)) = e else { continue };
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
