//! **Origem das operações endossadas: o motor da E0453** (protótipo em 27/11/2026; verificação
//! desde 28/11, em `ath-check`).
//!
//! Pergunta: numa chamada de ferramenta, os argumentos que vêm de valores endossados por `refine`
//! dependem de uma só operação aprovada? Uma operação é um elemento de lista endossada, escolhido
//! por um laço, ou o valor endossado inteiro. Ela é identificada pelo último ponto de escolha (o
//! laço ou a junção de `if`/`match`/`else`/`atomic` que escolheu o valor em execução); sem ponto
//! de escolha, pelo `refine`. A dependência de uma lista inteira (`join`, `len`, quantificador) é
//! a origem `[∀]`, distinta de cada elemento.
//!
//! Diferenças para a análise de obrigações:
//!
//! * vale para **toda** chamada de ferramenta, com ou sem `requires`;
//! * um valor calculado (`a ++ b`, `fit(x, n)`, aritmética, reconstrução) **não** perde a origem:
//!   herda a união das origens de quem o formou e é marcado como transformado. Assim,
//!   `fit(a.subject, 9) ++ b.amount` ainda conta como mistura de duas operações.
//!
//! Achados por sítio:
//!
//! * `mistura` (E0453): duas ou mais origens aprovadas distintas. É o caso X2;
//! * `agregação`: só a origem `[∀]` de uma lista com mais de um elemento possível. Aceita e fora da
//!   garantia;
//! * `transformada`: uma só operação, mas algum argumento passou por um cálculo. Aceita; não há
//!   prova de que o argumento seja o aprovado.
//!
//! Fora do alcance: dependência de controle (a condição de um `if` vinda de outra operação) e o
//! valor que passa por uma ferramenta, que perde a origem aprovada: `let r = echo(b.memo)` e depois
//! `pay(a.to, r)` é aceito, inclusive com `memo: Text ^ Endorsed`, porque o retorno de ferramenta
//! sem rótulo não é Untrusted.

use crate::ast::*;
use crate::span::Span;
use std::collections::{BTreeSet, HashMap};

/// Uma operação aprovada: (`refine`, caminho até o elemento, último ponto de escolha).
pub type Op = ((u32, u32), Vec<String>, Option<(u32, u32)>);

#[derive(Clone, Debug, PartialEq)]
struct Src {
    refine: (u32, u32),
    path: Vec<String>,
    /// pontos de escolha (laço, junção de `if`/`match`/`else`), cada um com o comprimento do
    /// caminho quando foi registrado: o que vem depois dele vale para todos os ramos
    ids: Vec<((u32, u32), usize)>,
    transformed: bool,
    /// o valor É uma lista endossada (ou a projeção de um campo dela), com até `n` elementos
    coll: Option<u32>,
    /// o valor foi calculado a partir de uma lista inteira (`join`, `len`, quantificador…):
    /// depende de todos os seus elementos, até `n`
    all: Option<u32>,
    /// uma entrada por lista literal que contém a fonte, da mais interna para a mais externa no
    /// fim: `true` se o item dela já combinava duas ou mais operações (não pode ser unificado
    /// pelo laço que escolhe o item)
    lit: Vec<bool>,
}

impl Src {
    /// Depende de todos os elementos de uma lista com possivelmente mais de um elemento?
    fn aggregate(&self) -> bool {
        self.coll.or(self.all).is_some_and(|n| n >= 2)
    }

    fn origem(&self) -> Origem {
        let op = self.op();
        let at = match self.ids.last() {
            Some(((lo, hi), _)) => Span { lo: *lo, hi: *hi },
            None => Span { lo: self.refine.0, hi: self.refine.1 },
        };
        let shown = |p: &[String]| p.iter().filter(|x| !x.starts_with('[')).cloned().collect::<Vec<_>>().join("/");
        let what = if self.coll.or(self.all).is_some() {
            let p: Vec<String> = self.path.iter().take_while(|x| !x.starts_with("?.")).cloned().collect();
            format!("todos os elementos de `{}`", shown(&p))
        } else {
            match self.ids.last() {
                Some((_, n)) if *n > 0 && self.path.get(n - 1).is_some_and(|x| x == "[*]") => {
                    format!("um elemento de `{}`", shown(&self.path[..*n]))
                }
                Some(_) => "o valor escolhido neste ponto".to_string(),
                None => "o valor endossado inteiro".to_string(),
            }
        };
        Origem { op, at, what }
    }

    fn op(&self) -> Op {
        // o último ponto de escolha identifica um só valor em execução (o elemento desta iteração,
        // o ramo tomado): tudo o que tem o mesmo último ponto vem desse valor, qualquer que seja o
        // caminho até ele (`refine A else { refine B }`, `refine … else { o }`). Os passos depois
        // dele são os mesmos em todos os ramos; só a agregação sobre uma lista os distingue.
        let (refine, from, id) = match self.ids.last() {
            Some((id, n)) => ((0, 0), (*n).min(self.path.len()), Some(*id)),
            None => (self.refine, 0, None),
        };
        if self.coll.or(self.all).is_some() {
            // todos os elementos da lista: uma origem marcada `[∀]`, que conta como várias
            let mut p = self.path[from..]
                .iter()
                .take_while(|x| !x.starts_with("?."))
                .map(|x| x.split_once('.').map(|(_, f)| f.to_string()).unwrap_or_else(|| x.clone()))
                .collect::<Vec<_>>();
            p.push("[∀]".into());
            return (refine, p, id);
        }
        // o elemento escolhido no último ponto, ou o valor endossado inteiro (sem ponto de escolha,
        // não há `[*]`: todo `[*]` vem de um laço, que registra o seu ponto)
        (refine, Vec::new(), id)
    }
}

/// Uma origem num achado: a operação, onde ela foi escolhida e a descrição para o diagnóstico.
#[derive(Clone, Debug)]
pub struct Origem {
    pub op: Op,
    /// o ponto de escolha (laço, `if`, `match`, `else`) ou, sem ele, o `refine`
    pub at: Span,
    pub what: String,
}

#[derive(Clone, Debug)]
pub enum Finding {
    /// uma operação só, intacta (ou nenhuma origem endossada)
    Uma,
    /// sem argumento vindo de valor endossado
    SemOrigem,
    /// argumentos de duas ou mais operações aprovadas; entre elas pode haver `[∀]`, a agregação
    /// sobre todos os elementos de uma lista endossada
    Mistura(Vec<Origem>),
    /// só uma agregação sobre todos os elementos de uma lista endossada com mais de um elemento
    /// possível (`join(notas.issue, …)`): depende de várias operações aprovadas
    Agregacao(Vec<Origem>),
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
    /// (variante, campo) → limite superior, quando o campo é uma lista
    lists: HashMap<(String, String), u32>,
    /// campo → maior limite superior entre as variantes em que ele é lista (para `x.campo`)
    list_fields: HashMap<String, u32>,
    out: Vec<Site>,
}

fn list_hi(t: &TypeExpr) -> Option<u32> {
    match t {
        TypeExpr::List { hi, .. } => Some(*hi),
        TypeExpr::Labeled(t, ..) | TypeExpr::ConfLabeled(t, ..) => list_hi(t),
        _ => None,
    }
}

impl An<'_> {
    fn step(&self, srcs: &[Src], s: &str, elem: Option<(u32, u32)>) -> Vec<Src> {
        srcs.iter()
            .map(|x| {
                let mut y = x.clone();
                y.path.push(s.to_string());
                // escolher um item de uma lista literal cujo item já misturava não unifica as
                // operações do item: elas seguem distintas
                let mixed = if s == "[*]" { y.lit.pop().unwrap_or(false) } else { false };
                if let Some(id) = elem {
                    if !mixed {
                        y.ids.push((id, y.path.len()));
                    }
                }
                if s == "[*]" {
                    y.coll = None; // um elemento escolhido
                } else if let Some((v, f)) = s.split_once('.') {
                    if v != "?" {
                        // campo de variante: lista ou não, pelo tipo declarado
                        y.coll = self.lists.get(&(v.to_string(), f.to_string())).copied();
                    } else if y.coll.is_none() {
                        // `p.ops` num registro: seleciona a lista `ops`, não projeta os elementos;
                        // entra no caminho como o campo de variante (`Plan.ops`)
                        if let Some(n) = self.list_fields.get(f) {
                            y.coll = Some(*n);
                            *y.path.last_mut().unwrap() = format!("*.{f}");
                        }
                    }
                    // `?.campo` mantém `coll`: `notas.issue` é a lista dos campos de todos os elementos
                }
                y
            })
            .collect()
    }
}

fn calc(parts: Vec<Vec<Src>>) -> Vec<Src> {
    let mut out: Vec<Src> = Vec::new();
    for p in parts.into_iter().flatten() {
        // calcular sobre uma lista inteira faz o resultado depender de todos os elementos
        let all = p.all.max(p.coll);
        let q = Src { transformed: true, coll: None, all, ..p };
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
    // os ramos escolhem entre operações diferentes? Compara as operações, não o caminho até o
    // campo nem a transformação: `if c { a.to } else { a.memo ++ "" }` é o mesmo elemento `a`
    let ops = |b: &Vec<Src>| b.iter().map(Src::op).collect::<BTreeSet<Op>>();
    let same = sets.windows(2).all(|w| ops(&w[0]) == ops(&w[1]));
    // um ramo que já combina operações não pode ser unificado pelo ponto de escolha
    let single = sets.iter().all(|b| ops(b).len() <= 1);
    let mut srcs = dedup(sets.into_iter().flatten().collect());
    if !same && single {
        for s in &mut srcs {
            let n = s.path.len();
            s.ids.push(((at.lo, at.hi), n));
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
                let me = vec![Src {
                    refine: (e.span.lo, e.span.hi),
                    path: Vec::new(),
                    ids: Vec::new(),
                    transformed: false,
                    coll: None,
                    all: None,
                    lit: Vec::new(),
                }];
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
                        env.push((b.name.clone(), self.step(&s, &format!("{}.{}", a.variant.name, f.name), None)));
                    }
                    r.push(self.expr(&a.body, env));
                    env.truncate(n);
                }
                join(r, e.span)
            }
            ExprKind::For { var, iter, body } => {
                let s = self.expr(iter, env);
                env.push((var.name.clone(), self.step(&s, "[*]", Some((var.span.lo, var.span.hi)))));
                self.block(body, env);
                env.pop();
                Vec::new()
            }
            ExprKind::List(items) => {
                // `for x in [a, b]` escolhe um item: os itens levam o caminho de lista literal
                let parts: Vec<Vec<Src>> = items.iter().map(|x| self.expr(x, env)).collect();
                parts
                    .into_iter()
                    .flat_map(|item| {
                        let mixed = item.iter().map(Src::op).collect::<BTreeSet<Op>>().len() > 1;
                        item.into_iter().map(move |s| {
                            let mut s = s;
                            s.path.push("[lit]".into());
                            s.lit.push(mixed);
                            s
                        })
                    })
                    .collect()
            }
            ExprKind::Ctor { fields, .. } => {
                // reconstruir um valor a partir de campos: a origem dos campos continua, transformada.
                // Não é cálculo sobre a lista: um campo que é a lista continua sendo a lista
                // (`G { xs: ops }` e depois `for o in xs` escolhe um elemento), e a agregação de um
                // campo continua agregação
                let mut out: Vec<Src> = Vec::new();
                for x in fields.iter().flat_map(|(_, x)| self.expr(x, env)) {
                    let q = Src { transformed: true, ..x };
                    if !out.contains(&q) {
                        out.push(q);
                    }
                }
                out
            }
            ExprKind::Commit(x) => self.expr(x, env),
            ExprKind::Return(x) => {
                self.expr(x, env);
                Vec::new()
            }
            ExprKind::Field(r, f) => {
                let x = self.expr(r, env);
                self.step(&x, &format!("?.{}", f.name), None)
            }
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
                // o valor do `atomic` é o do corpo ou o de um tratador: uma junção
                let mut r = vec![self.block(body, env)];
                for (x, b) in on_failure.iter().chain(on_cf.iter()) {
                    env.push((x.name.clone(), Vec::new()));
                    r.push(self.block(b, env));
                    env.pop();
                }
                join(r, e.span)
            }
            ExprKind::Reconcile { els, .. } => self.block(els, env),
            ExprKind::Quant { iter, body, var, .. } => {
                // `sum w in xs => …` depende de todos os elementos de `xs`: o que o corpo tira de
                // `w` vira a agregação sobre a lista; o resto do corpo entra como está
                let s = self.expr(iter, env);
                let id = (var.span.lo, var.span.hi);
                env.push((var.name.clone(), self.step(&s, "[*]", Some(id))));
                let b: Vec<Src> =
                    self.expr(body, env).into_iter().filter(|x| !x.ids.iter().any(|(i, _)| *i == id)).collect();
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
    // uma origem por operação (a primeira fonte dela descreve)
    let mut ops: Vec<Origem> = Vec::new();
    for s in &all {
        let o = s.origem();
        if !ops.iter().any(|x| x.op == o.op) {
            ops.push(o);
        }
    }
    ops.sort_by(|a, b| a.op.cmp(&b.op));
    if ops.len() > 1 {
        return Finding::Mistura(ops);
    }
    if all.iter().any(|s| s.aggregate()) {
        return Finding::Agregacao(ops);
    }
    if all.iter().any(|s| s.transformed) {
        return Finding::Transformada;
    }
    Finding::Uma
}

/// Os sítios de chamada de ferramenta do módulo, com o achado de cada um.
pub fn analyze(m: &Module) -> Vec<Site> {
    let mut an = An { tools: BTreeSet::new(), lists: HashMap::new(), list_fields: HashMap::new(), out: Vec::new() };
    for it in &m.items {
        match it {
            Item::Tool(t) => {
                an.tools.insert(t.name.name.as_str());
            }
            Item::Type(d) => {
                for v in &d.variants {
                    for f in &v.fields {
                        if let Some(hi) = list_hi(&f.ty) {
                            an.lists.insert((v.name.name.clone(), f.name.name.clone()), hi);
                            let e = an.list_fields.entry(f.name.name.clone()).or_insert(hi);
                            *e = (*e).max(hi);
                        }
                    }
                }
            }
            _ => {}
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
