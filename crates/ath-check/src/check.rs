//! Passada principal: E0 (estrutura), E2 (efeitos e capacidades) e E3 (integridade).
//! Também coleta os fatos de que E1 (linearidade) e E6 (orçamento) dependem.

use crate::diag::{Diag, Stratum};
use crate::fmt;
use crate::ty::{assignable, ty_tokens, unify, LTy, Refn, Ty};
use ath_syntax::ast::*;
use ath_syntax::{est_tokens, Span};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Verdadeiro se algum nó de `e` (inclusive `e`) satisfaz `f`. Percorre todas as formas de
/// expressão, então as regras de ancoragem não dependem de onde o termo aparece.
fn expr_any(e: &Expr, f: &dyn Fn(&Expr) -> bool) -> bool {
    if f(e) {
        return true;
    }
    let block = |b: &Block| {
        b.stmts.iter().any(|st| match st {
            Stmt::Let { value, .. } => expr_any(value, f),
            Stmt::Expr(x) => expr_any(x, f),
        }) || b.tail.as_ref().is_some_and(|t| expr_any(t, f))
    };
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Var(_) => false,
        ExprKind::Transfer(_) => false,
        ExprKind::Field(x, _) | ExprKind::Unary(_, x) | ExprKind::Return(x) => expr_any(x, f),
        ExprKind::Quote(x) | ExprKind::Commit(x) => expr_any(x, f),
        ExprKind::Call { args, .. } => args.iter().any(|a| expr_any(a, f)),
        ExprKind::Method { recv, args, .. } => expr_any(recv, f) || args.iter().any(|a| expr_any(a, f)),
        ExprKind::Ctor { fields, .. } | ExprKind::Infer { fields, .. } => fields.iter().any(|(_, v)| expr_any(v, f)),
        ExprKind::Binary(_, l, r) => expr_any(l, f) || expr_any(r, f),
        ExprKind::If { cond, then, els } => expr_any(cond, f) || block(then) || els.as_ref().is_some_and(block),
        ExprKind::Match { scrut, arms } => expr_any(scrut, f) || arms.iter().any(|a| expr_any(&a.body, f)),
        ExprKind::Block(b) => block(b),
        ExprKind::Refine { draft, els, .. } => expr_any(draft, f) || block(els),
        ExprKind::Atomic { recover, body, on_failure, on_cf } => {
            expr_any(recover, f)
                || block(body)
                || on_failure.as_ref().is_some_and(|(_, b)| block(b))
                || on_cf.as_ref().is_some_and(|(_, b)| block(b))
        }
        ExprKind::Await { cond, .. } => expr_any(cond, f),
        ExprKind::Reconcile { els, .. } => block(els),
        ExprKind::List(xs) => xs.iter().any(|x| expr_any(x, f)),
        ExprKind::For { iter, body, .. } => expr_any(iter, f) || block(body),
        ExprKind::Quant { iter, body, .. } => expr_any(iter, f) || expr_any(body, f),
    }
}

/// `s.<método>(…)`: leitura da fotografia confiável do host. `s` é o receptor intrínseco: uma
/// variável local chamada `s` não intercepta esses métodos.
fn snapshot_method(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Method { recv, method, .. } if matches!(&recv.kind, ExprKind::Var(n) if n == "s") => {
            Some(method.name.as_str())
        }
        _ => None,
    }
}

/// A política decide pela fotografia confiável (`s.is_managed`, `s.scale_justified`…).
fn reads_snapshot(e: &Expr) -> bool {
    expr_any(e, &|x| snapshot_method(x).is_some())
}

/// A política compara texto com um literal (`deploy == "api"`): um inventário escrito à mão.
fn compares_text_literal(e: &Expr) -> bool {
    expr_any(e, &|x| match &x.kind {
        ExprKind::Binary(BinOp::Eq | BinOp::Ne, l, r) => {
            matches!(l.kind, ExprKind::Str(_)) || matches!(r.kind, ExprKind::Str(_))
        }
        _ => false,
    })
}

/// O campo do prompt é exatamente `s.managed()`: o inventário vem da fotografia confiável.
fn is_inventory_anchor(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Method { args, .. } if args.is_empty()) && snapshot_method(e) == Some("managed")
}

/// Custo de uma amostra de um nó `infer`, sob o contrato Δ do modelo.
#[derive(Clone, Debug)]
pub struct InferCost {
    pub model: String,
    pub out_ty: String,
    pub tok_in: u64,
    /// Teto de tokens de saída por amostra: conteúdo + raciocínio.
    pub tok_out: u64,
    /// Parte do teto reservada a raciocínio pelo contrato do modelo.
    pub tok_reasoning: u64,
    /// O teto do conteúdo veio do tipo (bytes da codificação), não do `max_out` do modelo.
    pub out_by_type: bool,
    /// Overhead declarado no contrato (template de chat + esquema), incluído em `tok_in`.
    pub overhead: u64,
    pub usd: f64,
    pub wall: f64,
}

#[derive(Default, Debug)]
pub struct Facts {
    /// `let` que vinculam valores lineares (`IdemKey`).
    pub linear_lets: HashSet<Span>,
    /// Custo por amostra de cada nó `infer`, indexado pelo span do nó.
    pub infer_costs: HashMap<Span, InferCost>,
    /// Ponto de consumo (`sample`, `refine`) → nó `infer` de origem.
    pub draft_src: HashMap<Span, Span>,
    pub integrity_checks: usize,
    pub refinement_checks: usize,
    pub tool_calls: usize,
    /// Blocos `atomic` e, para cada ferramenta compensável usada neles, o compensador.
    pub atomic_blocks: usize,
    pub compensations: std::collections::BTreeSet<(String, String)>,
    /// Chamadas com chave de idempotência (podem terminar em `TimeoutUnknown`).
    pub keyed_calls: usize,
    /// Pior tempo de reconciliação por chave, fora do contrato Δ (segundos).
    pub reconcile_bound: Option<f64>,
    pub effects: BTreeSet<String>,
    /// Laços (`for`) e quantificadores (`all`/`any`), indexados pelo span: limites de iterações.
    pub loop_bounds: HashMap<Span, (u32, u32)>,
    pub loops: usize,
}

/// Recuo entre tentativas de reconciliação no ARX, em segundos.
pub const RECONCILE_BACKOFF: f64 = 2.0;

/// Assinatura textual de um tipo declarado (para comparar parâmetros).
pub fn type_sig(t: &TypeExpr) -> String {
    t.sig()
}

/// Maior `fuel` de `reconcile` num tratador (0 se não reconcilia).
fn max_fuel(b: &Block) -> u32 {
    fn ex(e: &Expr) -> u32 {
        match &e.kind {
            ExprKind::Reconcile { fuel, els, .. } => (*fuel).max(max_fuel(els)),
            ExprKind::Block(b) => max_fuel(b),
            ExprKind::If { then, els, .. } => max_fuel(then).max(els.as_ref().map_or(0, max_fuel)),
            _ => 0,
        }
    }
    let mut m = 0;
    for s in &b.stmts {
        let (Stmt::Expr(e) | Stmt::Let { value: e, .. }) = s;
        m = m.max(ex(e));
    }
    b.tail.as_deref().map_or(m, |t| m.max(ex(t)))
}

pub const MODEL_PROPS: [&str; 6] = ["price_in", "price_out", "ctx", "max_out", "ttft", "per_tok"];
/// Propriedades opcionais do contrato Δ: `reasoning` = tokens de raciocínio admitidos por amostra.
/// `overhead` = tokens de entrada que o provedor acrescenta (template de chat, esquema de decodificação).
pub const MODEL_OPTIONAL: [&str; 2] = ["reasoning", "overhead"];
/// Identidade versionada do contrato Δ.
pub const MODEL_META: [&str; 3] = ["id", "provider", "version"];
const BUILTINS: [&str; 3] = ["fit", "sample", "len"];

pub struct Ctx<'m> {
    pub m: &'m Module,
    pub types: HashMap<String, &'m TypeDecl>,
    pub variants: HashMap<String, (&'m TypeDecl, &'m Variant)>,
    pub tools: HashMap<String, &'m ToolDecl>,
    pub models: HashMap<String, &'m ModelDecl>,
    pub policies: HashMap<String, &'m PolicyDecl>,
    pub diags: Vec<Diag>,
    pub facts: Facts,
    scopes: Vec<Vec<(String, LTy)>>,
    pc: Label,
    pc_span: Option<Span>,
    used: BTreeMap<String, Span>,
    ret: Option<Ty>,
    ret_refn: Option<Refn>,
    in_policy: bool,
    commit_ok: bool,
    /// Pilha de blocos `atomic` abertos; cada um guarda as chamadas compensáveis que contém.
    atomic_stack: Vec<Vec<Span>>,
    /// Dentro de `handle timeout_unknown`.
    in_handle: bool,
    /// Ao checar condições de controle, o resultado de uma ferramenta herda a integridade dos argumentos.
    taint_calls: bool,
    /// `infer`s (por span) cujo prompt recebe o inventário confiável `s.managed()`.
    anchored: BTreeSet<Span>,
}

use Stratum::{E0, E1, E2, E3, E5, E6};

impl<'m> Ctx<'m> {
    pub fn new(m: &'m Module) -> Self {
        Ctx {
            m,
            types: HashMap::new(),
            variants: HashMap::new(),
            tools: HashMap::new(),
            models: HashMap::new(),
            policies: HashMap::new(),
            diags: Vec::new(),
            facts: Facts::default(),
            scopes: Vec::new(),
            pc: Label::Trusted,
            pc_span: None,
            used: BTreeMap::new(),
            ret: None,
            ret_refn: None,
            in_policy: false,
            commit_ok: false,
            atomic_stack: Vec::new(),
            in_handle: false,
            taint_calls: false,
            anchored: BTreeSet::new(),
        }
    }

    /// Condições que decidem o desfecho de uma saga precisam de integridade ⊒ Endorsed.
    fn check_control(&mut self, span: Span, t: &LTy, what: &str) {
        let eff = t.lab.meet(self.pc);
        self.facts.integrity_checks += 1;
        if eff >= Label::Endorsed {
            return;
        }
        let mut d = Diag::new(
            E3,
            "E0417",
            format!("{what} com integridade {}", eff.name()),
            span,
            format!("integridade {}", eff.name()),
        );
        if !t.origin.is_empty() {
            d = d.note(format!("origem: {}", t.origin.join(" → ")));
        }
        self.push(
            d.note(format!("exigido: {what} ⊒ Endorsed"))
                .help("observe o que o plano endossado afetou: use os campos do plano, não o dado do evento"),
        );
    }

    /// Refinamento declarado num tipo, se houver.
    pub fn refn_of(&self, te: &TypeExpr) -> Option<Refn> {
        match te.unlabeled() {
            TypeExpr::Refined { base, lo, hi, .. } => match base.name.as_str() {
                "Int" => Some(Refn::Int { lo: *lo, hi: *hi }),
                "Text" => hi.map(|h| Refn::Text { max: h as u32 }),
                _ => None,
            },
            _ => None,
        }
    }

    /// E5: o valor de `arg` precisa satisfazer `want`. Provas aceitas no Marco Zero:
    /// literal dentro do intervalo, ou valor cujo refinamento conhecido está contido em `want`
    /// (campos e parâmetros declarados, saídas decodificadas de `infer`).
    fn expect_refn(&mut self, arg: &Expr, at: &LTy, want: Option<Refn>, what: &str) {
        let Some(w) = want else { return };
        self.facts.refinement_checks += 1;
        if at.ty.is_never() || at.refn.is_some_and(|r| r.within(w)) {
            return;
        }
        let literal = match (&arg.kind, w) {
            (ExprKind::Int(n), Refn::Int { .. }) => Some(w.admits_int(*n)),
            (ExprKind::Unary(UnOp::Neg, inner), Refn::Int { .. }) => match inner.kind {
                ExprKind::Int(n) => Some(w.admits_int(-n)),
                _ => None,
            },
            (ExprKind::Str(s), Refn::Text { .. }) => Some(w.admits_str(s)),
            _ => None,
        };
        match literal {
            Some(true) => {}
            Some(false) => self.push(
                Diag::new(E5, "E0451", "literal fora do refinamento", arg.span, format!("não pertence a {}", w.show()))
                    .note(format!("exigido por {what}")),
            ),
            None => {
                let known = at.refn.map(Refn::show).unwrap_or_else(|| at.ty.show());
                self.push(
                    Diag::new(E5, "E0450", "refinamento não provado", arg.span, format!("conhecido como `{known}`, exigido {}", w.show()))
                        .note(format!("exigido por {what}"))
                        .help("prove com um literal, um campo de refinamento igual ou mais estreito, ou a saída decodificada de `infer`; obrigações aritméticas gerais esperam o E5 com SMT"),
                );
            }
        }
    }

    fn push(&mut self, d: Diag) {
        self.diags.push(d);
    }

    fn mismatch(&mut self, span: Span, want: &Ty, got: &Ty) {
        self.push(Diag::new(
            E0,
            "E0008",
            "tipo incompatível",
            span,
            format!("esperado `{}`, encontrado `{}`", want.show(), got.show()),
        ));
    }

    fn impure(&mut self, span: Span, what: &str) {
        self.push(
            Diag::new(E2, "E0420", "política impura", span, format!("{what} dentro de `policy`"))
                .help("políticas são puras, totais e decidíveis: só comparações, lógica e `match`"),
        );
    }

    fn bind(&mut self, name: &str, t: LTy) {
        if let Some(s) = self.scopes.last_mut() {
            s.push((name.to_string(), t));
        }
    }

    fn lookup(&self, name: &str) -> Option<LTy> {
        for s in self.scopes.iter().rev() {
            for (n, t) in s.iter().rev() {
                if n == name {
                    return Some(t.clone());
                }
            }
        }
        None
    }

    fn use_effect(&mut self, eff: &str, span: Span) {
        self.used.entry(eff.to_string()).or_insert(span);
    }

    pub fn resolve(&mut self, te: &TypeExpr) -> (Ty, Option<Label>) {
        match te {
            TypeExpr::Named(id) => {
                let t = match id.name.as_str() {
                    "Int" => Ty::Int,
                    "Bool" => Ty::Bool,
                    "Text" => Ty::Text,
                    "Unit" => Ty::Unit,
                    "IdemKey" => Ty::IdemKey,
                    "Pending" => Ty::Pending,
                    n if self.types.contains_key(n) => Ty::Adt(n.to_string()),
                    n => {
                        self.push(Diag::new(E0, "E0004", format!("tipo desconhecido `{n}`"), id.span, "não declarado"));
                        Ty::Never
                    }
                };
                (t, None)
            }
            TypeExpr::Refined { base, .. } => match base.name.as_str() {
                "Int" => (Ty::Int, None),
                "Text" => (Ty::Text, None),
                n => {
                    self.push(Diag::new(
                        E0,
                        "E0004",
                        format!("`{n}` não admite refinamento"),
                        base.span,
                        "só Int e Text",
                    ));
                    (Ty::Never, None)
                }
            },
            TypeExpr::Cap(effs, _) => (Ty::Cap(effs.iter().map(|e| e.name.clone()).collect()), None),
            TypeExpr::Labeled(inner, l, _) => {
                let (t, _) = self.resolve(inner);
                (t, Some(*l))
            }
            TypeExpr::Own(inner, _) => self.resolve(inner),
            TypeExpr::List { elem, lo, hi, .. } => {
                let (et, el) = self.resolve(elem);
                if el.is_some() {
                    self.push(
                        Diag::new(
                            E0,
                            "E0004",
                            "rótulo de integridade dentro de `List`",
                            elem.span(),
                            "rótulo no elemento",
                        )
                        .help("rotule a lista inteira: `List[T, a..b] ^ Untrusted`"),
                    );
                }
                if matches!(et, Ty::Cap(_)) || et.is_linear() {
                    self.push(Diag::new(
                        E0,
                        "E0004",
                        "capacidades, chaves e obrigações não entram em listas",
                        elem.span(),
                        "não armazenável",
                    ));
                }
                let refn = self.refn_of(elem);
                (Ty::List(Box::new(et), refn, *lo, *hi), None)
            }
        }
    }

    /// O tipo `have` está contido no tipo declarado `want`? Limites de listas (em qualquer
    /// nível de aninhamento) e refinamentos dos elementos.
    fn ty_within(&self, have: &Ty, want: &TypeExpr) -> bool {
        match (have, want.unlabeled()) {
            (Ty::List(elem, er, a, b), TypeExpr::List { elem: we, lo, hi, .. }) => {
                let elem_ok = match self.refn_of(we) {
                    Some(w) => er.is_some_and(|r| r.within(w)),
                    None => true,
                };
                a >= lo && b <= hi && elem_ok && self.ty_within(elem, we)
            }
            _ => true,
        }
    }

    /// E5 para um argumento declarado com o tipo `te`: refinamento escalar ou, para listas,
    /// limites de tamanho e refinamento dos elementos.
    fn expect_param(&mut self, arg: &Expr, at: &LTy, te: &TypeExpr, what: &str) {
        if !matches!(te.unlabeled(), TypeExpr::List { .. }) {
            let want = self.refn_of(te);
            self.expect_refn(arg, at, want, what);
            return;
        }
        if !matches!(at.ty, Ty::List(..)) {
            return;
        }
        self.facts.refinement_checks += 1;
        if self.ty_within(&at.ty, te) {
            return;
        }
        let TypeExpr::List { elem, lo, hi, .. } = te.unlabeled() else { return };
        let Ty::List(et, _, a, b) = &at.ty else { return };
        if let ExprKind::List(items) = &arg.kind {
            let n = items.len() as u32;
            if n < *lo || n > *hi {
                self.push(
                    Diag::new(
                        E5,
                        "E0452",
                        "lista fora dos limites",
                        arg.span,
                        format!("{n} elemento(s); `{}` admite de {lo} a {hi}", te.unlabeled().sig()),
                    )
                    .note(format!("exigido por {what}")),
                );
                return;
            }
            let want = self.refn_of(elem);
            for it in items {
                let lt = LTy::trusted((**et).clone());
                match &it.kind {
                    ExprKind::List(_) => self.expect_param(it, &LTy::trusted((**et).clone()), elem, what),
                    _ => self.expect_refn(it, &lt, want, what),
                }
            }
            return;
        }
        let known = format!("List[_, {a}..{b}]");
        self.push(
            Diag::new(
                E5,
                "E0450",
                "refinamento não provado",
                arg.span,
                format!("conhecida como `{}`, exigido `{}`", at.ty.show(), te.unlabeled().sig()),
            )
            .note(format!("exigido por {what}; limites conhecidos {known}"))
            .help("passe uma lista cujo tipo declarado tenha limites contidos nos exigidos, ou uma lista literal"),
        );
    }

    /// Parâmetros lineares exigem `own`; `own` só se aplica a tipos lineares.
    fn check_ownership(&mut self, p: &Param, t: &Ty) {
        if t.is_linear() && !p.ty.is_own() {
            self.push(
                Diag::new(
                    E1,
                    "E0042",
                    format!("`{}` é linear e precisa de posse explícita", t.show()),
                    p.ty.span(),
                    "declare `own`",
                )
                .help(format!("escreva `{}: own {}`", p.name.name, p.ty.base_name())),
            );
        } else if p.ty.is_own() && !t.is_linear() && !t.is_never() {
            self.push(Diag::new(
                E1,
                "E0043",
                format!("`own` só se aplica a tipos lineares, não a `{}`", t.show()),
                p.ty.span(),
                "posse linear desnecessária",
            ));
        }
    }

    // ================================================================ declarações

    pub fn check_all(&mut self) {
        let m = self.m;
        let mut seen: HashMap<String, Span> = HashMap::new();
        for item in &m.items {
            let id = item.ident();
            if BUILTINS.contains(&id.name.as_str()) {
                self.push(Diag::new(
                    E0,
                    "E0002",
                    format!("`{}` é uma primitiva reservada", id.name),
                    id.span,
                    "nome reservado",
                ));
                continue;
            }
            if let Some(prev) = seen.get(&id.name).copied() {
                self.push(
                    Diag::new(
                        E0,
                        "E0002",
                        format!("`{}` definido mais de uma vez", id.name),
                        id.span,
                        "redefinido aqui",
                    )
                    .secondary(prev, "primeira definição"),
                );
                continue;
            }
            seen.insert(id.name.clone(), id.span);
            match item {
                Item::Type(d) => {
                    self.types.insert(d.name.name.clone(), d);
                    for v in &d.variants {
                        if let Some((_, pv)) = self.variants.get(&v.name.name).copied() {
                            self.push(
                                Diag::new(
                                    E0,
                                    "E0002",
                                    format!("variante `{}` definida mais de uma vez", v.name.name),
                                    v.name.span,
                                    "redefinida aqui",
                                )
                                .secondary(pv.name.span, "primeira definição"),
                            );
                        } else {
                            self.variants.insert(v.name.name.clone(), (d, v));
                        }
                    }
                }
                Item::Model(d) => {
                    self.models.insert(d.name.name.clone(), d);
                }
                Item::Tool(d) => {
                    self.tools.insert(d.name.name.clone(), d);
                }
                Item::Policy(d) => {
                    self.policies.insert(d.name.name.clone(), d);
                }
                Item::Agent(_) => {}
            }
        }

        for item in &m.items {
            match item {
                Item::Type(d) => self.check_type(d),
                Item::Model(d) => self.check_model(d),
                Item::Tool(d) => self.check_tool(d),
                _ => {}
            }
        }
        for item in &m.items {
            if let Item::Policy(p) = item {
                self.check_policy(p);
            }
        }
        for item in &m.items {
            if let Item::Agent(a) = item {
                self.check_agent(a);
            }
        }
    }

    fn check_type(&mut self, d: &'m TypeDecl) {
        for v in &d.variants {
            for f in &v.fields {
                let (t, l) = self.resolve(&f.ty);
                if l.is_some() {
                    self.push(Diag::new(
                        E0,
                        "E0004",
                        "rótulo de integridade em campo de dado",
                        f.ty.span(),
                        "rótulos pertencem a valores, não a campos",
                    ));
                }
                if matches!(t, Ty::Cap(_) | Ty::IdemKey) {
                    self.push(Diag::new(
                        E0,
                        "E0004",
                        "capacidades e chaves não podem ser armazenadas em dados",
                        f.ty.span(),
                        "não armazenável",
                    ));
                }
            }
        }
    }

    fn check_model(&mut self, d: &'m ModelDecl) {
        for key in MODEL_PROPS {
            if d.prop(key).is_none() {
                self.push(
                    Diag::new(
                        E0,
                        "E0030",
                        format!("modelo `{}` sem `{}`", d.name.name, key),
                        d.name.span,
                        "contrato Δ incompleto",
                    )
                    .help(format!("declare {}", MODEL_PROPS.join(", "))),
                );
            }
        }
        for p in &d.props {
            if !MODEL_PROPS.contains(&p.key.name.as_str()) && !MODEL_OPTIONAL.contains(&p.key.name.as_str()) {
                self.push(Diag::new(
                    E0,
                    "E0030",
                    format!("propriedade de modelo desconhecida `{}`", p.key.name),
                    p.key.span,
                    "desconhecida",
                ));
            } else if p.value < 0.0 {
                self.push(Diag::new(E0, "E0030", "propriedade de modelo negativa", p.key.span, "deve ser ≥ 0"));
            }
        }
        for (k, v) in &d.meta {
            if !MODEL_META.contains(&k.name.as_str()) {
                self.push(
                    Diag::new(
                        E0,
                        "E0030",
                        format!("campo de contrato desconhecido `{}`", k.name),
                        k.span,
                        "desconhecido",
                    )
                    .help("a identidade do contrato Δ é `id`, `provider` e `version`"),
                );
            } else if v.trim().is_empty() {
                self.push(Diag::new(E0, "E0030", "campo de contrato vazio", k.span, "informe um valor"));
            }
        }
        if d.meta("id").is_none() {
            if let Some((k, _)) = d.meta.iter().find(|(k, _)| k.name == "provider" || k.name == "version") {
                self.push(
                    Diag::new(E0, "E0030", "contrato versionado sem `id`", k.span, "versão de quê?")
                        .help("declare o modelo servido: `id = \"openai/gpt-oss-20b\"`"),
                );
            }
        }
    }

    fn check_tool(&mut self, t: &'m ToolDecl) {
        let mut caps = BTreeSet::new();
        for p in &t.params {
            if let (Ty::Cap(c), _) = self.resolve(&p.ty) {
                caps.extend(c);
            }
        }
        for eff in &t.effects {
            if !caps.contains(&eff.name) {
                self.push(
                    Diag::new(
                        E2,
                        "E0500",
                        "efeito sem capacidade",
                        eff.span,
                        format!("`{}` declara `{}`, mas nenhum parâmetro `Cap[…]` o cobre", t.name.name, eff.name),
                    )
                    .help(format!("acrescente um parâmetro `c: Cap[{}]`", eff.name)),
                );
            }
        }
        let (rt, _) = self.resolve(&t.ret);
        if rt.is_linear() {
            self.push(Diag::new(E0, "E0040", "ferramentas não devolvem valores lineares", t.ret.span(), "tipo linear"));
        }
        for p in &t.params {
            let (pt, _) = self.resolve(&p.ty);
            if matches!(pt, Ty::Pending) {
                self.push(Diag::new(
                    E0,
                    "E0044",
                    "`Pending` não é argumento de ferramenta",
                    p.ty.span(),
                    "só reconcile ou transfer consomem Pending",
                ));
            }
            self.check_ownership(p, &pt);
        }
        if t.latency < 0.0 || t.usd < 0.0 {
            self.push(Diag::new(E0, "E0030", "latência e custo devem ser ≥ 0", t.name.span, "contrato Δ inválido"));
        }
        self.check_compensator(t);
    }

    /// `compensable by c`: `c` existe, não é a própria ferramenta, recebe os mesmos parâmetros
    /// (o ARX a chama com os argumentos e a chave da operação original) e não é irrevogável.
    fn check_compensator(&mut self, t: &'m ToolDecl) {
        if t.class != EffClass::Compensable {
            if let Some(c) = &t.compensator {
                self.push(Diag::new(E2, "E0505", "compensador em efeito não compensável", c.span, "remova `by …`"));
            }
            return;
        }
        let Some(c) = &t.compensator else {
            self.push(
                Diag::new(E2, "E0505", "`compensable` sem compensador", t.class_span, "falta `by <ferramenta>`")
                    .help("declare a transição de volta: `compensable by <ferramenta com os mesmos parâmetros>`"),
            );
            return;
        };
        let Some(comp) = self.tools.get(&c.name).copied() else {
            self.push(Diag::new(
                E2,
                "E0505",
                format!("compensador desconhecido `{}`", c.name),
                c.span,
                "não é ferramenta declarada",
            ));
            return;
        };
        if comp.name.name == t.name.name {
            self.push(Diag::new(E2, "E0505", "uma ferramenta não compensa a si mesma", c.span, "compensador circular"));
            return;
        }
        if comp.class == EffClass::Irrevocable {
            self.push(
                Diag::new(
                    E2,
                    "E0505",
                    format!("compensador `{}` é irrevocable", c.name),
                    c.span,
                    "compensação irrevogável",
                )
                .note("uma compensação é uma transição de volta; precisa ser reversible ou compensable"),
            );
        }
        let sig = |d: &ToolDecl| d.params.iter().map(|p| type_sig(&p.ty)).collect::<Vec<_>>();
        if sig(t) != sig(comp) {
            self.push(
                Diag::new(
                    E2,
                    "E0505",
                    format!("`{}` não tem a assinatura de `{}`", c.name, t.name.name),
                    c.span,
                    "parâmetros diferentes",
                )
                .note(format!("{}({})", t.name.name, sig(t).join(", ")))
                .note(format!("{}({})", c.name, sig(comp).join(", ")))
                .help("o ARX chama o compensador com os mesmos argumentos e a mesma chave da operação original"),
            );
        }
    }

    fn check_policy(&mut self, p: &'m PolicyDecl) {
        self.in_policy = true;
        self.scopes = vec![vec![]];
        self.pc = Label::Trusted;
        self.pc_span = None;
        self.used.clear();
        self.ret = None;
        for prm in &p.params {
            let (t, _) = self.resolve(&prm.ty);
            self.bind(&prm.name.name, LTy::trusted(t));
        }
        let (rt, _) = self.resolve(&p.ret);
        if !assignable(&rt, &Ty::Bool) {
            self.push(Diag::new(
                E0,
                "E0008",
                "uma política devolve `Bool`",
                p.ret.span(),
                format!("encontrado `{}`", rt.show()),
            ));
        }
        let body = self.expr(&p.body);
        if !assignable(&body.ty, &Ty::Bool) {
            self.mismatch(p.body.span, &Ty::Bool, &body.ty);
        }
        self.in_policy = false;
        self.scopes.clear();
    }

    /// Escopo inicial de um handler: as capacidades do agente.
    fn agent_scope(&mut self, a: &'m AgentDecl, report: bool) {
        self.scopes = vec![vec![]];
        self.pc = Label::Trusted;
        self.pc_span = None;
        self.used.clear();
        self.in_policy = false;
        for p in &a.params {
            let (t, _) = self.resolve(&p.ty);
            if report && !matches!(t, Ty::Cap(_) | Ty::Never) {
                self.push(Diag::new(
                    E0,
                    "E0040",
                    "parâmetros de agente são capacidades",
                    p.ty.span(),
                    format!("encontrado `{}`", t.show()),
                ));
            }
            self.bind(&p.name.name, LTy::trusted(t));
        }
    }

    /// `handle timeout_unknown(p: own Pending[_]) -> T { … }`. Devolve o tipo de retorno do
    /// tratador e os efeitos que ele usa (somados à linha de quem executa `TimeoutUnknown`).
    fn check_handles(&mut self, a: &'m AgentDecl) -> Option<(Ty, Span, BTreeMap<String, Span>)> {
        let mut out = None;
        for h in &a.handles {
            if h.effect.name != "timeout_unknown" {
                self.push(
                    Diag::new(
                        E2,
                        "E0045",
                        format!("efeito desconhecido `{}`", h.effect.name),
                        h.effect.span,
                        "sem tratador",
                    )
                    .help("o Marco Zero trata `timeout_unknown`; `CompensationFailed` é tratado no próprio `atomic`"),
                );
                continue;
            }
            if out.is_some() {
                self.push(Diag::new(
                    E0,
                    "E0002",
                    "`handle timeout_unknown` definido mais de uma vez",
                    h.effect.span,
                    "duplicado",
                ));
                continue;
            }
            self.agent_scope(a, false);
            match h.params.as_slice() {
                [p] => {
                    let (t, _) = self.resolve(&p.ty);
                    if !matches!(t, Ty::Pending | Ty::Never) {
                        self.push(Diag::new(
                            E0,
                            "E0046",
                            "o tratador recebe `p: own Pending[_]`",
                            p.ty.span(),
                            format!("encontrado `{}`", t.show()),
                        ));
                    }
                    self.check_ownership(p, &t);
                    self.bind(&p.name.name, LTy::trusted(Ty::Pending));
                }
                _ => self.push(Diag::new(
                    E0,
                    "E0046",
                    "o tratador recebe exatamente `p: own Pending[_]`",
                    h.effect.span,
                    "aridade",
                )),
            }
            let (rt, _) = self.resolve(&h.ret);
            self.ret = Some(rt.clone());
            self.in_handle = true;
            let body = self.block(&h.body);
            self.in_handle = false;
            if !assignable(&body.ty, &rt) {
                let sp = h.body.tail.as_ref().map(|t| t.span).unwrap_or(h.body.span);
                self.mismatch(sp, &rt, &body.ty);
            }
            let used = std::mem::take(&mut self.used);
            self.ret = None;
            self.scopes.clear();
            // pior caso por chave: `fuel` tentativas, cada uma com recuo e a maior latência entre as ferramentas com chave
            let fuel = max_fuel(&h.body);
            let lat = self.tools.values().filter(|t| t.keyed()).map(|t| t.latency).fold(0.0, f64::max);
            self.facts.reconcile_bound = Some(fuel as f64 * (RECONCILE_BACKOFF + lat));
            out = Some((rt, h.ret.span(), used));
        }
        out
    }

    fn check_agent(&mut self, a: &'m AgentDecl) {
        for b in &a.budget {
            if !["tok", "usd", "wall"].contains(&b.key.name.as_str()) {
                self.push(
                    Diag::new(
                        E0,
                        "E0031",
                        format!("dimensão de orçamento desconhecida `{}`", b.key.name),
                        b.key.span,
                        "desconhecida",
                    )
                    .help("use tok, usd ou wall"),
                );
            } else if b.value < 0.0 {
                self.push(Diag::new(E0, "E0031", "orçamento negativo", b.key.span, "deve ser ≥ 0"));
            }
        }
        let handle = self.check_handles(a);
        let mut events = HashSet::new();
        for (i, h) in a.handlers.iter().enumerate() {
            if !events.insert(h.event.name.clone()) {
                self.push(Diag::new(
                    E0,
                    "E0002",
                    format!("handler `on {}` definido mais de uma vez", h.event.name),
                    h.event.span,
                    "duplicado",
                ));
            }
            self.agent_scope(a, i == 0);
            for p in &h.params {
                let (t, l) = self.resolve(&p.ty);
                if matches!(t, Ty::Cap(_)) || t.is_linear() {
                    self.push(Diag::new(
                        E0,
                        "E0041",
                        "eventos não carregam capacidades, chaves nem obrigações",
                        p.ty.span(),
                        "capacidades só vêm do agente",
                    ));
                }
                let lab = l.unwrap_or(Label::Trusted);
                let mut origin = Vec::new();
                if lab < Label::Trusted {
                    origin.push(format!("evento `{}({})` ^ {}", h.event.name, p.name.name, lab.name()));
                }
                let refn = self.refn_of(&p.ty);
                self.bind(&p.name.name, LTy::declared(t, lab, refn, origin));
            }
            let (rt, _) = self.resolve(&h.ret);
            self.ret = Some(rt.clone());
            self.ret_refn = self.refn_of(&h.ret);
            let body = self.block(&h.body);
            if !assignable(&body.ty, &rt) {
                let sp = h.body.tail.as_ref().map(|t| t.span).unwrap_or(h.body.span);
                self.mismatch(sp, &rt, &body.ty);
            } else if let Some(t) = &h.body.tail {
                let want = self.ret_refn;
                self.expect_refn(t, &body, want, &format!("o retorno de `on {}`", h.event.name));
            }
            self.ret_refn = None;
            let mut used = std::mem::take(&mut self.used);
            if let Some(&sp) = used.get("TimeoutUnknown") {
                match &handle {
                    None => self.push(
                        Diag::new(E2, "E0503", "efeito `TimeoutUnknown` sem tratador", sp, "chamada com chave pode terminar em resultado desconhecido")
                            .note("um timeout diz que o agente parou de esperar, não que o efeito não aconteceu")
                            .help("declare no agente: `handle timeout_unknown(p: own Pending[_]) -> T { reconcile p fuel 3 else { transfer p; … } }`"),
                    ),
                    Some((ht, hspan, heffects)) => {
                        if !assignable(ht, &rt) {
                            self.push(
                                Diag::new(E0, "E0008", "o tratador de `TimeoutUnknown` encerra o evento com outro tipo", *hspan, format!("devolve `{}`", ht.show()))
                                    .note(format!("`on {}` devolve `{}`", h.event.name, rt.show())),
                            );
                        }
                        for (eff, esp) in heffects {
                            used.entry(eff.clone()).or_insert(*esp);
                        }
                    }
                }
            }
            let declared: BTreeSet<String> = h.effects.iter().map(|e| e.name.clone()).collect();
            for (eff, sp) in &used {
                if !declared.contains(eff) {
                    self.push(
                        Diag::new(E2, "E0501", "efeito não declarado", *sp, format!("produz `{eff}`"))
                            .note(format!(
                                "linha de efeitos de `on {}`: {{{}}}",
                                h.event.name,
                                declared.iter().cloned().collect::<Vec<_>>().join(", ")
                            ))
                            .help(format!("acrescente `{eff}` à linha de efeitos do handler")),
                    );
                }
            }
            self.facts.effects.extend(used.into_keys());
            self.ret = None;
            self.scopes.clear();
        }
    }

    // ================================================================ expressões

    fn block(&mut self, b: &Block) -> LTy {
        self.scopes.push(Vec::new());
        let mut diverged = false;
        for s in &b.stmts {
            match s {
                Stmt::Let { name, value, span } => {
                    let t = self.expr(value);
                    if matches!(t.ty, Ty::IdemKey) {
                        self.facts.linear_lets.insert(*span);
                    }
                    self.bind(&name.name, t);
                }
                Stmt::Expr(x) => {
                    let t = self.expr(x);
                    if t.ty.is_never() {
                        diverged = true;
                    }
                }
            }
        }
        let out = match &b.tail {
            Some(t) => self.expr(t),
            None if diverged => LTy::never(),
            None => LTy::trusted(Ty::Unit),
        };
        self.scopes.pop();
        out
    }

    fn expr(&mut self, e: &Expr) -> LTy {
        match &e.kind {
            ExprKind::Int(_) => LTy::trusted(Ty::Int),
            ExprKind::Bool(_) => LTy::trusted(Ty::Bool),
            ExprKind::Str(s) => LTy { tok: Some(est_tokens(s)), ..LTy::trusted(Ty::Text) },
            ExprKind::Var(n) => match self.lookup(n) {
                Some(t) if matches!(t.ty, Ty::Pending) => {
                    self.push(
                        Diag::new(
                            E0,
                            "E0044",
                            "`Pending` só é consumido por `reconcile` ou `transfer`",
                            e.span,
                            "uso indevido de obrigação",
                        )
                        .help("uma operação em aberto não é um valor comum: reconcilie-a ou transfira-a"),
                    );
                    LTy::never()
                }
                Some(t) => t,
                None => {
                    self.push(Diag::new(
                        E0,
                        "E0001",
                        format!("variável desconhecida `{n}`"),
                        e.span,
                        "não encontrada neste escopo",
                    ));
                    LTy::never()
                }
            },
            ExprKind::Field(recv, f) => self.field(recv, f),
            ExprKind::Call { callee, args } => self.call(e, callee, args),
            ExprKind::Method { recv, method, args } => self.method(recv, method, args),
            ExprKind::Ctor { name, fields } => self.ctor(e, name, fields),
            ExprKind::Binary(op, l, r) => self.binary(e, *op, l, r),
            ExprKind::Unary(op, x) => {
                let t = self.expr(x);
                let want = match op {
                    UnOp::Not => Ty::Bool,
                    UnOp::Neg => Ty::Int,
                };
                if !assignable(&t.ty, &want) {
                    self.mismatch(x.span, &want, &t.ty);
                }
                LTy { ty: want, tok: None, refn: None, ..t }
            }
            ExprKind::If { cond, then, els } => self.if_expr(cond, then, els.as_ref()),
            ExprKind::Match { scrut, arms } => self.match_expr(e, scrut, arms),
            ExprKind::Block(b) => self.block(b),
            ExprKind::Return(v) => {
                if self.in_policy {
                    self.impure(e.span, "`return`");
                }
                if !self.atomic_stack.is_empty() {
                    self.push(
                        Diag::new(E2, "E0602", "`return` dentro de `atomic`", e.span, "a saga precisa confirmar ou falhar")
                            .help("devolva o valor no fim do bloco, ou saia pelos tratadores `on failure` / `on compensation_failed`"),
                    );
                }
                let t = self.expr(v);
                if let Some(r) = self.ret.clone() {
                    if !assignable(&t.ty, &r) {
                        self.mismatch(v.span, &r, &t.ty);
                    } else {
                        let want = self.ret_refn;
                        self.expect_refn(v, &t, want, "o valor de `return`");
                    }
                }
                LTy::never()
            }
            ExprKind::Infer { model, out, fields } => self.infer(e, model, out, fields),
            ExprKind::Refine { draft, policy, retry, els } => self.refine(e, draft, policy, *retry, els),
            ExprKind::Quote(x) => self.expr(x),
            ExprKind::Commit(x) => {
                if let ExprKind::Call { callee, .. } = &x.kind {
                    if self.tools.contains_key(&callee.name) {
                        self.commit_ok = true;
                        return self.expr(x);
                    }
                }
                self.push(Diag::new(
                    E2,
                    "T0304",
                    "`commit` exige uma chamada de ferramenta irrevocable",
                    x.span,
                    "não é chamada de ferramenta",
                ));
                self.expr(x)
            }
            ExprKind::Atomic { recover, body, on_failure, on_cf } => {
                self.atomic(e, recover, body, on_failure.as_ref(), on_cf.as_ref())
            }
            ExprKind::Await { cond, within } => {
                if self.atomic_stack.is_empty() {
                    self.push(
                        Diag::new(E2, "E0601", "`await` fora de `atomic`", e.span, "pós-condição sem saga")
                            .help("uma pós-condição observada só faz sentido onde a falha dispara compensações"),
                    );
                }
                if *within < 0.0 {
                    self.push(Diag::new(E0, "E0030", "`within` negativo", e.span, "deve ser ≥ 0"));
                }
                let saved = std::mem::replace(&mut self.taint_calls, true);
                let c = self.expr(cond);
                self.taint_calls = saved;
                if !assignable(&c.ty, &Ty::Bool) {
                    self.mismatch(cond.span, &Ty::Bool, &c.ty);
                }
                self.check_control(cond.span, &c, "a pós-condição de `atomic`");
                LTy::trusted(Ty::Unit)
            }
            ExprKind::Reconcile { pending, els, .. } => {
                self.pending_operand(e.span, pending, "reconcile");
                self.block(els)
            }
            ExprKind::Transfer(p) => {
                self.pending_operand(e.span, p, "transfer");
                LTy::trusted(Ty::Unit)
            }
            ExprKind::List(items) => self.list_lit(items),
            ExprKind::For { var, iter, body } => {
                if self.in_policy {
                    self.impure(e.span, "laço `for` (use `all x in xs => …` ou `any`)");
                }
                let Some((it, elem)) = self.iteration(e.span, iter, "for") else {
                    self.scopes.push(vec![(var.name.clone(), LTy::never())]);
                    self.block(body);
                    self.scopes.pop();
                    return LTy::trusted(Ty::Unit);
                };
                let saved = (self.pc, self.pc_span);
                // O número de iterações é decidido pela lista: fluxo implícito, como num `match`.
                if it.lab < self.pc {
                    self.pc = it.lab;
                    self.pc_span = Some(iter.span);
                }
                self.scopes.push(vec![(var.name.clone(), elem)]);
                self.block(body);
                self.scopes.pop();
                (self.pc, self.pc_span) = saved;
                LTy::trusted(Ty::Unit)
            }
            ExprKind::Quant { var, iter, body, .. } => {
                let Some((it, elem)) = self.iteration(e.span, iter, "quantificador") else {
                    self.scopes.push(vec![(var.name.clone(), LTy::never())]);
                    self.expr(body);
                    self.scopes.pop();
                    return LTy::trusted(Ty::Bool);
                };
                let saved = (self.pc, self.pc_span);
                if it.lab < self.pc {
                    self.pc = it.lab;
                    self.pc_span = Some(iter.span);
                }
                self.scopes.push(vec![(var.name.clone(), elem)]);
                let b = self.expr(body);
                self.scopes.pop();
                (self.pc, self.pc_span) = saved;
                if !assignable(&b.ty, &Ty::Bool) {
                    self.mismatch(body.span, &Ty::Bool, &b.ty);
                }
                let mut out = LTy::trusted(Ty::Bool);
                out.meet_from(&it);
                out.meet_from(&b);
                out
            }
        }
    }

    /// Lista literal: tipo dos elementos unificado, limites exatos `n..n` e, para os elementos,
    /// a envoltória dos refinamentos (literais contam como intervalos exatos).
    fn list_lit(&mut self, items: &[Expr]) -> LTy {
        let mut elem = Ty::Never;
        let mut out = LTy::trusted(Ty::Unit);
        let mut refn: Option<Option<Refn>> = None;
        let mut tok: Option<u64> = Some(2);
        for x in items {
            let t = self.expr(x);
            match unify(&elem, &t.ty) {
                Some(u) => elem = u,
                None => self.push(Diag::new(
                    E0,
                    "E0011",
                    "elementos de lista com tipos diferentes",
                    x.span,
                    format!("`{}` vs `{}`", elem.show(), t.ty.show()),
                )),
            }
            let r = match &x.kind {
                ExprKind::Int(n) => Some(Refn::Int { lo: Some(*n), hi: Some(*n) }),
                ExprKind::Unary(UnOp::Neg, inner) => match inner.kind {
                    ExprKind::Int(n) => Some(Refn::Int { lo: Some(-n), hi: Some(-n) }),
                    _ => t.refn,
                },
                ExprKind::Str(s) => Some(Refn::Text { max: s.chars().count() as u32 }),
                _ => t.refn,
            };
            refn = Some(match refn {
                None => r,
                Some(prev) => match (prev, r) {
                    (Some(p), Some(q)) => p.hull(q),
                    _ => None,
                },
            });
            tok = match (tok, t.tok.or_else(|| ty_tokens(&t.ty, t.refn))) {
                (Some(a), Some(b)) => Some(a + b + 1),
                _ => None,
            };
            out.meet_from(&t);
        }
        let n = items.len() as u32;
        LTy { ty: Ty::List(Box::new(elem), refn.flatten(), n, n), lab: out.lab, tok, origin: out.origin, refn: None }
    }

    /// Operando de `for`/`all`/`any`: precisa ser uma lista delimitada. Registra os limites
    /// de iterações (para o E6) e devolve o tipo do elemento, com a integridade da lista.
    fn iteration(&mut self, span: Span, iter: &Expr, what: &str) -> Option<(LTy, LTy)> {
        let it = self.expr(iter);
        match &it.ty {
            Ty::List(elem, er, lo, hi) => {
                self.facts.loop_bounds.insert(span, (*lo, *hi));
                self.facts.loops += 1;
                let mut el = LTy::declared((**elem).clone(), it.lab, *er, it.origin.clone());
                if el.tok.is_none() {
                    el.tok = ty_tokens(elem, *er).filter(|_| matches!(**elem, Ty::List(..)));
                }
                Some((it.clone(), el))
            }
            Ty::Never => None,
            other => {
                self.push(
                    Diag::new(
                        E0,
                        "E0008",
                        format!("`{what}` exige uma lista delimitada"),
                        iter.span,
                        format!("encontrado `{}`", other.show()),
                    )
                    .help("itere uma `List[T, a..b]`: o teto `b` limita o custo do laço no E6"),
                );
                None
            }
        }
    }

    /// `reconcile p` e `transfer p`: só no tratador de `TimeoutUnknown`, só sobre `Pending`.
    fn pending_operand(&mut self, span: Span, p: &Ident, op: &str) {
        if !self.in_handle {
            self.push(
                Diag::new(
                    E2,
                    "E0603",
                    format!("`{op}` fora de `handle timeout_unknown`"),
                    span,
                    "sem obrigação em aberto aqui",
                )
                .help("obrigações chegam ao programa só pelo tratador de `TimeoutUnknown`"),
            );
        }
        match self.lookup(&p.name) {
            Some(t) if matches!(t.ty, Ty::Pending | Ty::Never) => {}
            Some(t) => self.mismatch(p.span, &Ty::Pending, &t.ty),
            None => self.push(Diag::new(
                E0,
                "E0001",
                format!("variável desconhecida `{}`", p.name),
                p.span,
                "não encontrada",
            )),
        }
    }

    /// `atomic recover <sonda> { … } on failure(e) { … } on compensation_failed(cf) { … }`
    fn atomic(
        &mut self,
        e: &Expr,
        recover: &Expr,
        body: &Block,
        on_failure: Option<&(Ident, Block)>,
        on_cf: Option<&(Ident, Block)>,
    ) -> LTy {
        if self.in_policy {
            self.impure(e.span, "`atomic`");
        }
        if self.in_handle {
            self.push(Diag::new(
                E2,
                "E0603",
                "`atomic` dentro do tratador de `TimeoutUnknown`",
                e.span,
                "saga dentro de reconciliação",
            ));
        }
        self.facts.atomic_blocks += 1;
        let saved = std::mem::replace(&mut self.taint_calls, true);
        let r = self.expr(recover);
        self.taint_calls = saved;
        if !assignable(&r.ty, &Ty::Bool) {
            self.mismatch(recover.span, &Ty::Bool, &r.ty);
        }
        self.check_control(recover.span, &r, "a sonda de recuperação de `atomic`");
        self.atomic_stack.push(Vec::new());
        let b = self.block(body);
        let compensable = self.atomic_stack.pop().unwrap_or_default();
        let mut out = b;
        for (bind, blk) in on_failure.into_iter().chain(on_cf) {
            self.scopes.push(vec![(bind.name.clone(), LTy::trusted(Ty::Text))]);
            let h = self.block(blk);
            self.scopes.pop();
            match unify(&out.ty, &h.ty) {
                Some(ty) => {
                    let mut o = LTy::trusted(ty);
                    o.meet_from(&out);
                    o.meet_from(&h);
                    out = o;
                }
                None => self.push(Diag::new(
                    E0,
                    "E0011",
                    "tratador de `atomic` com tipo diferente do bloco",
                    blk.span,
                    format!("`{}` vs `{}`", out.ty.show(), h.ty.show()),
                )),
            }
        }
        if let Some(&first) = compensable.first() {
            self.use_effect("CompensationFailed", first);
            if on_cf.is_none() {
                let mut d = Diag::new(
                    E2,
                    "E0503",
                    "efeito `CompensationFailed` sem tratador",
                    first,
                    "efeito compensável: a compensação também pode falhar",
                )
                .note("compensar é executar uma nova transição no mundo; ela pode não restaurar o invariante");
                for sp in compensable.iter().skip(1) {
                    d = d.secondary(*sp, "outro efeito compensável");
                }
                self.push(d.help(
                    "acrescente ao `atomic`: `on compensation_failed(cf) { … }` (escalar a um humano, por exemplo)",
                ));
            }
        }
        out
    }

    fn field(&mut self, recv: &Expr, f: &Ident) -> LTy {
        let r = self.expr(recv);
        let name = match &r.ty {
            Ty::Adt(n) => n.clone(),
            Ty::Never => return LTy::never(),
            other => {
                self.push(Diag::new(
                    E0,
                    "E0005",
                    format!("`{}` não tem campos", other.show()),
                    f.span,
                    "campo inexistente",
                ));
                return LTy::never();
            }
        };
        let decl: &'m TypeDecl = self.types[&name];
        let mut found: Option<Ty> = None;
        let mut refns: Vec<Option<Refn>> = Vec::new();
        let mut ok = true;
        for v in &decl.variants {
            match v.fields.iter().find(|fd| fd.name.name == f.name) {
                Some(fd) => {
                    let (t, _) = self.resolve(&fd.ty);
                    refns.push(self.refn_of(&fd.ty));
                    match &found {
                        None => found = Some(t),
                        Some(prev) if crate::ty::same(prev, &t) => {}
                        _ => ok = false,
                    }
                }
                None => ok = false,
            }
        }
        // O refinamento do campo vale se for o mesmo em todas as variantes.
        let refn = match refns.first() {
            Some(first) if refns.iter().all(|r| r == first) => *first,
            _ => None,
        };
        match (ok, found) {
            (true, Some(t)) => LTy::declared(t, r.lab, refn, r.origin),
            _ => {
                self.push(
                    Diag::new(
                        E0,
                        "E0005",
                        format!("campo `{}` não existe em todas as variantes de `{}`", f.name, name),
                        f.span,
                        "campo ausente em alguma variante",
                    )
                    .help("use `match` para acessar campos de variantes específicas"),
                );
                LTy::never()
            }
        }
    }

    fn method(&mut self, recv: &Expr, method: &Ident, args: &[Expr]) -> LTy {
        // `s` é uma referência intrínseca à fotografia confiável do host.
        // Não é variável de usuário, não vem de inferência e não pode ser forjada
        // por `quote` ou por uma ferramenta que devolva Trusted.
        if matches!(&recv.kind, ExprKind::Var(name) if name == "s") {
            let check_args = |this: &mut Self, expected: usize| {
                if args.len() != expected {
                    this.push(Diag::new(
                        E0,
                        "E0007",
                        format!("s.{} exige {expected} argumento(s)", method.name),
                        method.span,
                        "aridade",
                    ));
                }
            };
            match method.name.as_str() {
                "managed" => {
                    check_args(self, 0);
                    for a in args {
                        self.expr(a);
                    }
                    return LTy::declared(Ty::Text, Label::Trusted, Some(Refn::Text { max: 256 }), Vec::new());
                }
                "telemetry" => {
                    check_args(self, 0);
                    for a in args {
                        self.expr(a);
                    }
                    return LTy::declared(Ty::Text, Label::Trusted, Some(Refn::Text { max: 512 }), Vec::new());
                }
                "is_managed" | "scale_justified" => {
                    check_args(self, if method.name == "is_managed" { 1 } else { 2 });
                    if !self.in_policy {
                        // As decisões contextuais ficam nas políticas auditadas, onde o `refine` as aplica.
                        self.push(
                            Diag::new(
                                E2,
                                "E0423",
                                "sonda contextual fora de política",
                                method.span,
                                format!("`s.{}` só decide dentro de `policy`", method.name),
                            )
                            .help("leve a decisão para a política aplicada pelo `refine`"),
                        );
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.expr(arg);
                        let expected = if i == 0 { &Ty::Text } else { &Ty::Int };
                        if !assignable(&t.ty, expected) {
                            self.mismatch(arg.span, expected, &t.ty);
                        }
                    }
                    return LTy::trusted(Ty::Bool);
                }
                _ => {
                    self.push(Diag::new(
                        E0,
                        "E0003",
                        format!("método de estado desconhecido `s.{}`", method.name),
                        method.span,
                        "método não reconhecido",
                    ));
                    for a in args {
                        self.expr(a);
                    }
                    return LTy::never();
                }
            }
        }
        let r = self.expr(recv);
        if method.name != "mint" {
            self.push(Diag::new(
                E0,
                "E0003",
                format!("método desconhecido `{}`", method.name),
                method.span,
                "o Marco Zero só conhece `mint()`",
            ));
            return LTy::never();
        }
        if !args.is_empty() {
            self.push(Diag::new(E0, "E0007", "`mint()` não recebe argumentos", method.span, "aridade"));
        }
        if self.in_policy {
            self.impure(method.span, "`mint()`");
        }
        match &r.ty {
            Ty::Cap(c) if c.contains("IdemKey.Mint") => {}
            Ty::Never => {}
            other => self.push(
                Diag::new(
                    E2,
                    "E0502",
                    "capacidade insuficiente",
                    recv.span,
                    format!("`{}` não inclui IdemKey.Mint", other.show()),
                )
                .help("cunhar chaves exige um parâmetro `Cap[IdemKey.Mint]` do agente"),
            ),
        }
        LTy::trusted(Ty::IdemKey)
    }

    fn call(&mut self, e: &Expr, callee: &Ident, args: &[Expr]) -> LTy {
        let committed = std::mem::replace(&mut self.commit_ok, false);
        match callee.name.as_str() {
            "fit" => {
                if args.len() != 2 {
                    self.push(Diag::new(E0, "E0007", "`fit(texto, n)` recebe 2 argumentos", e.span, "aridade"));
                    return LTy::never();
                }
                let t = self.expr(&args[0]);
                if !assignable(&t.ty, &Ty::Text) {
                    self.mismatch(args[0].span, &Ty::Text, &t.ty);
                }
                let n = match &args[1].kind {
                    ExprKind::Int(n) if *n > 0 => *n as u64,
                    _ => {
                        self.push(Diag::new(
                            E0,
                            "E0006",
                            "`fit` exige um inteiro literal positivo",
                            args[1].span,
                            "limite de tokens",
                        ));
                        0
                    }
                };
                return LTy { ty: Ty::Text, lab: t.lab, tok: Some(n), origin: t.origin, refn: None };
            }
            "len" => {
                if args.len() != 1 {
                    self.push(Diag::new(E0, "E0007", "`len(lista)` recebe 1 argumento", e.span, "aridade"));
                    return LTy::never();
                }
                let t = self.expr(&args[0]);
                return match &t.ty {
                    // O tamanho herda os limites do tipo: `len(xs)` é um `Int[a..b]` provado.
                    Ty::List(_, _, lo, hi) => LTy {
                        ty: Ty::Int,
                        lab: t.lab,
                        tok: None,
                        origin: t.origin.clone(),
                        refn: Some(Refn::Int { lo: Some(*lo as i64), hi: Some(*hi as i64) }),
                    },
                    Ty::Never => LTy::never(),
                    other => {
                        self.push(Diag::new(
                            E0,
                            "E0008",
                            "`len` exige uma lista",
                            args[0].span,
                            format!("encontrado `{}`", other.show()),
                        ));
                        LTy::never()
                    }
                };
            }
            "sample" => {
                if args.len() != 1 {
                    self.push(Diag::new(E0, "E0007", "`sample(rascunho)` recebe 1 argumento", e.span, "aridade"));
                    return LTy::never();
                }
                if self.in_policy {
                    self.impure(e.span, "`sample`");
                }
                let LTy { ty, lab, origin, refn, .. } = self.expr(&args[0]);
                return match ty {
                    Ty::Draft(inner, ispan) => {
                        self.facts.draft_src.insert(e.span, ispan);
                        self.use_effect("Infer", e.span);
                        LTy::declared(*inner, lab, refn, origin).with_step("sample(…)")
                    }
                    Ty::Never => LTy::never(),
                    other => {
                        self.push(Diag::new(
                            E0,
                            "E0023",
                            "`sample` exige um rascunho de `infer`",
                            args[0].span,
                            format!("encontrado `{}`", other.show()),
                        ));
                        LTy::never()
                    }
                };
            }
            _ => {}
        }
        if let Some(tool) = self.tools.get(&callee.name).copied() {
            return self.tool_call(e, tool, callee, args, committed);
        }
        if let Some(p) = self.policies.get(&callee.name).copied() {
            if args.len() != p.params.len() {
                self.push(Diag::new(
                    E0,
                    "E0007",
                    format!("`{}` recebe {} argumento(s)", callee.name, p.params.len()),
                    e.span,
                    "aridade",
                ));
            }
            let mut out = LTy::trusted(Ty::Bool);
            for (prm, a) in p.params.iter().zip(args) {
                let at = self.expr(a);
                let (pt, _) = self.resolve(&prm.ty);
                if !assignable(&at.ty, &pt) {
                    self.mismatch(a.span, &pt, &at.ty);
                } else {
                    self.expect_param(a, &at, &prm.ty, &format!("{}({})", callee.name, prm.name.name));
                }
                out.meet_from(&at);
            }
            return out;
        }
        self.push(Diag::new(
            E0,
            "E0001",
            format!("função desconhecida `{}`", callee.name),
            callee.span,
            "não é ferramenta, política nem primitiva",
        ));
        for a in args {
            self.expr(a);
        }
        LTy::never()
    }

    fn tool_call(&mut self, e: &Expr, tool: &'m ToolDecl, callee: &Ident, args: &[Expr], committed: bool) -> LTy {
        self.facts.tool_calls += 1;
        if self.in_policy {
            self.impure(e.span, &format!("chamada de ferramenta `{}`", callee.name));
        }
        let in_atomic = !self.atomic_stack.is_empty();
        if in_atomic && tool.class == EffClass::Irrevocable {
            self.push(
                Diag::new(
                    E2,
                    "T0302",
                    "efeito irrevogável dentro de `atomic`",
                    callee.span,
                    format!("`{}` é irrevocable", callee.name),
                )
                .note("um efeito irrevogável não tem compensação: a saga não poderia recuperar o invariante")
                .help(format!(
                    "mova `commit {}(…)` para depois do bloco `atomic`, quando a fase compensável já confirmou",
                    callee.name
                )),
            );
        } else if in_atomic && tool.keyed() && tool.provider == ProviderClass::Opaque {
            self.push(
                Diag::new(
                    E2,
                    "T0305",
                    "provedor opaco dentro de `atomic`",
                    callee.span,
                    format!("`{}` não tem protocolo de reconciliação", callee.name),
                )
                .note("um resultado desconhecido de provedor opaco não pode ser reconciliado nem compensado com segurança")
                .help("declare `provider idempotent` ou `provider queryable`, ou chame fora da saga"),
            );
        }
        if in_atomic && tool.class == EffClass::Compensable {
            if let Some(frame) = self.atomic_stack.last_mut() {
                frame.push(callee.span);
            }
            if let Some(comp) = tool.compensator.as_ref().and_then(|c| self.tools.get(&c.name).copied()) {
                self.facts.compensations.insert((tool.name.name.clone(), comp.name.name.clone()));
                for eff in &comp.effects {
                    self.use_effect(&eff.name, e.span);
                }
            }
        }
        if tool.keyed() {
            self.facts.keyed_calls += 1;
            self.use_effect("TimeoutUnknown", e.span);
        }
        match (tool.class, committed) {
            (EffClass::Irrevocable, _) if in_atomic => {}
            (EffClass::Irrevocable, false) => self.push(
                Diag::new(
                    E2,
                    "T0301",
                    "efeito irrevogável fora de `commit`",
                    callee.span,
                    format!("`{}` é irrevocable", callee.name),
                )
                .help(format!("escreva `commit {}(…)`: irrevogáveis só depois de toda validação", callee.name)),
            ),
            (c, true) if c != EffClass::Irrevocable => self.push(
                Diag::new(
                    E2,
                    "T0303",
                    "`commit` sobre efeito revogável",
                    callee.span,
                    format!("`{}` é {}", callee.name, c.name()),
                )
                .help("`commit` marca apenas efeitos irrevocable"),
            ),
            _ => {}
        }
        if args.len() != tool.params.len() {
            self.push(Diag::new(
                E0,
                "E0007",
                format!("`{}` recebe {} argumento(s), {} fornecido(s)", callee.name, tool.params.len(), args.len()),
                e.span,
                "aridade",
            ));
            for a in args {
                self.expr(a);
            }
            return LTy::never();
        }
        let mut taint = LTy::trusted(Ty::Unit);
        for (p, a) in tool.params.iter().zip(args) {
            let at = self.expr(a);
            if !matches!(at.ty, Ty::Cap(_) | Ty::IdemKey) {
                taint.meet_from(&at);
            }
            let (pt, req) = self.resolve(&p.ty);
            if !assignable(&at.ty, &pt) {
                if let (Ty::Cap(have), Ty::Cap(need)) = (&at.ty, &pt) {
                    let missing: Vec<String> = need.difference(have).cloned().collect();
                    self.push(
                        Diag::new(
                            E2,
                            "E0502",
                            "capacidade insuficiente",
                            a.span,
                            format!("falta {}", missing.join(", ")),
                        )
                        .note(format!("tem: {}", at.ty.show()))
                        .note(format!("exigido: {}({}: {})", callee.name, p.name.name, pt.show()))
                        .help("capacidades só vêm dos parâmetros do agente; passe uma que inclua o efeito"),
                    );
                } else {
                    self.mismatch(a.span, &pt, &at.ty);
                }
                continue;
            }
            self.expect_param(a, &at, &p.ty, &format!("{}({})", callee.name, p.name.name));
            if let Some(req) = req {
                self.facts.integrity_checks += 1;
                let eff = at.lab.meet(self.pc);
                if eff < req {
                    let implicit = at.lab >= req;
                    let mut d = Diag::new(
                        E3,
                        "E0412",
                        "fluxo de integridade violado",
                        a.span,
                        format!("integridade {}", eff.name()),
                    );
                    if implicit {
                        d = d.note(format!(
                            "o valor é {}, mas a chamada ocorre num ramo decidido por dado {}",
                            at.lab.name(),
                            self.pc.name()
                        ));
                        if let Some(ps) = self.pc_span {
                            d = d.secondary(ps, format!("ramo decidido por dado {}", self.pc.name()));
                        }
                    } else if !at.origin.is_empty() {
                        d = d.note(format!("origem: {}", at.origin.join(" → ")));
                    }
                    d = d.note(format!("exigido: {}({}: {} ^ {})", callee.name, p.name.name, pt.show(), req.name()));
                    d = d.help(if implicit {
                        "decida o ramo com dado confiável ou endossado: ramificar sobre Untrusted rebaixa o pc"
                    } else {
                        "passe o valor por `refine … by <policy>`: só a política concede Endorsed"
                    });
                    self.push(d);
                }
            }
        }
        for eff in &tool.effects {
            self.use_effect(&eff.name, e.span);
        }
        let (rt, rl) = self.resolve(&tool.ret);
        let lab = rl.unwrap_or(Label::Trusted);
        let mut origin = Vec::new();
        if lab < Label::Trusted {
            origin.push(format!("{}(…) devolve ^ {}", callee.name, lab.name()));
        }
        let refn = self.refn_of(&tool.ret);
        let mut out = LTy::declared(rt, lab, refn, origin);
        if self.taint_calls {
            // Numa condição de controle, o que a ferramenta observa é escolhido pelos argumentos.
            if taint.lab < out.lab {
                out.lab = taint.lab;
                out.origin = taint.origin;
                out.origin.push(format!("{}(…)", callee.name));
            }
        }
        out
    }

    fn ctor(&mut self, e: &Expr, name: &Ident, fields: &[(Ident, Expr)]) -> LTy {
        let Some((td, v)) = self.variants.get(&name.name).copied() else {
            self.push(Diag::new(
                E0,
                "E0009",
                format!("construtor desconhecido `{}`", name.name),
                name.span,
                "nenhum tipo declara esta variante",
            ));
            for (_, x) in fields {
                self.expr(x);
            }
            return LTy::never();
        };
        let mut out = LTy::trusted(Ty::Adt(td.name.name.clone()));
        for (fname, fe) in fields {
            let at = self.expr(fe);
            match v.fields.iter().find(|f| f.name.name == fname.name) {
                Some(fd) => {
                    let (ft, _) = self.resolve(&fd.ty);
                    if !assignable(&at.ty, &ft) {
                        self.mismatch(fe.span, &ft, &at.ty);
                    } else {
                        self.expect_param(fe, &at, &fd.ty, &format!("o campo `{}.{}`", name.name, fname.name));
                    }
                    out.meet_from(&at);
                }
                None => self.push(Diag::new(
                    E0,
                    "E0010",
                    format!("`{}` não tem campo `{}`", name.name, fname.name),
                    fname.span,
                    "campo inexistente",
                )),
            }
        }
        for fd in &v.fields {
            if !fields.iter().any(|(n, _)| n.name == fd.name.name) {
                self.push(Diag::new(
                    E0,
                    "E0010",
                    format!("campo `{}` ausente em `{}`", fd.name.name, name.name),
                    e.span,
                    "campo obrigatório",
                ));
            }
        }
        out
    }

    fn binary(&mut self, e: &Expr, op: BinOp, l: &Expr, r: &Expr) -> LTy {
        let lt = self.expr(l);
        let rt = self.expr(r);
        let (want, result) = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => (Some(Ty::Int), Ty::Int),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => (Some(Ty::Int), Ty::Bool),
            BinOp::And | BinOp::Or => (Some(Ty::Bool), Ty::Bool),
            BinOp::Concat => (Some(Ty::Text), Ty::Text),
            BinOp::Eq | BinOp::Ne => (None, Ty::Bool),
        };
        match want {
            Some(w) => {
                if !assignable(&lt.ty, &w) {
                    self.mismatch(l.span, &w, &lt.ty);
                }
                if !assignable(&rt.ty, &w) {
                    self.mismatch(r.span, &w, &rt.ty);
                }
            }
            None => {
                if unify(&lt.ty, &rt.ty).is_none() {
                    self.push(Diag::new(
                        E0,
                        "E0008",
                        "comparação entre tipos diferentes",
                        e.span,
                        format!("`{}` {} `{}`", lt.ty.show(), op.symbol(), rt.ty.show()),
                    ));
                } else if matches!(lt.ty, Ty::Cap(_) | Ty::IdemKey | Ty::Draft(..)) {
                    self.push(Diag::new(
                        E0,
                        "E0008",
                        "tipo não comparável",
                        e.span,
                        format!("`{}` não admite `{}`", lt.ty.show(), op.symbol()),
                    ));
                }
            }
        }
        let mut out = LTy::trusted(result);
        out.meet_from(&lt);
        out.meet_from(&rt);
        if op == BinOp::Concat {
            out.tok = match (lt.tok, rt.tok) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            };
        }
        out
    }

    fn if_expr(&mut self, cond: &Expr, then: &Block, els: Option<&Block>) -> LTy {
        let c = self.expr(cond);
        if !assignable(&c.ty, &Ty::Bool) {
            self.mismatch(cond.span, &Ty::Bool, &c.ty);
        }
        let saved = (self.pc, self.pc_span);
        if c.lab < self.pc {
            self.pc = c.lab;
            self.pc_span = Some(cond.span);
        }
        let t = self.block(then);
        let mut out = match els {
            Some(b) => {
                let f = self.block(b);
                match unify(&t.ty, &f.ty) {
                    Some(ty) => {
                        let mut o = LTy::trusted(ty);
                        o.meet_from(&t);
                        o.meet_from(&f);
                        o
                    }
                    None => {
                        self.push(Diag::new(
                            E0,
                            "E0011",
                            "ramos com tipos diferentes",
                            b.span,
                            format!("`{}` vs `{}`", t.ty.show(), f.ty.show()),
                        ));
                        LTy::never()
                    }
                }
            }
            None => LTy::trusted(Ty::Unit),
        };
        (self.pc, self.pc_span) = saved;
        out.meet_from(&c);
        out
    }

    fn match_expr(&mut self, e: &Expr, scrut: &Expr, arms: &[Arm]) -> LTy {
        let s = self.expr(scrut);
        let tname = match &s.ty {
            Ty::Adt(n) => n.clone(),
            Ty::Never => return LTy::never(),
            other => {
                self.push(Diag::new(
                    E0,
                    "E0008",
                    "`match` exige um tipo algébrico",
                    scrut.span,
                    format!("encontrado `{}`", other.show()),
                ));
                return LTy::never();
            }
        };
        let decl: &'m TypeDecl = self.types[&tname];
        let saved = (self.pc, self.pc_span);
        if arms.len() > 1 && s.lab < self.pc {
            self.pc = s.lab;
            self.pc_span = Some(scrut.span);
        }
        let mut result: Option<LTy> = None;
        let mut seen: HashSet<String> = HashSet::new();
        for arm in arms {
            let Some(v) = decl.variants.iter().find(|v| v.name.name == arm.variant.name) else {
                self.push(Diag::new(
                    E0,
                    "E0009",
                    format!("`{}` não é variante de `{}`", arm.variant.name, tname),
                    arm.variant.span,
                    "variante inexistente",
                ));
                continue;
            };
            if !seen.insert(v.name.name.clone()) {
                self.push(Diag::new(E0, "E0013", "braço duplicado", arm.variant.span, "variante já tratada"));
            }
            self.scopes.push(Vec::new());
            for b in &arm.binds {
                match v.fields.iter().find(|f| f.name.name == b.name) {
                    Some(fd) => {
                        let (ft, _) = self.resolve(&fd.ty);
                        let refn = self.refn_of(&fd.ty);
                        self.bind(&b.name, LTy::declared(ft, s.lab, refn, s.origin.clone()));
                    }
                    None => {
                        self.push(Diag::new(
                            E0,
                            "E0010",
                            format!("`{}` não tem campo `{}`", v.name.name, b.name),
                            b.span,
                            "campo inexistente",
                        ));
                        self.bind(&b.name, LTy::never());
                    }
                }
            }
            let bt = self.expr(&arm.body);
            self.scopes.pop();
            result = Some(match result {
                None => bt,
                Some(prev) => match unify(&prev.ty, &bt.ty) {
                    Some(ty) => {
                        let mut o = LTy::trusted(ty);
                        o.meet_from(&prev);
                        o.meet_from(&bt);
                        o
                    }
                    None => {
                        self.push(Diag::new(
                            E0,
                            "E0011",
                            "braços com tipos diferentes",
                            arm.body.span,
                            format!("`{}` vs `{}`", prev.ty.show(), bt.ty.show()),
                        ));
                        prev
                    }
                },
            });
        }
        (self.pc, self.pc_span) = saved;
        let missing: Vec<String> =
            decl.variants.iter().filter(|v| !seen.contains(&v.name.name)).map(|v| v.name.name.clone()).collect();
        if !missing.is_empty() {
            self.push(Diag::new(
                E0,
                "E0012",
                "`match` não exaustivo",
                e.span,
                format!("faltam: {}", missing.join(", ")),
            ));
        }
        let mut out = result.unwrap_or_else(LTy::never);
        out.meet_from(&s);
        out
    }

    fn infer(&mut self, e: &Expr, model: &Ident, out: &TypeExpr, fields: &[(Ident, Expr)]) -> LTy {
        if self.in_policy {
            self.impure(e.span, "`infer`");
        }
        let Some(md) = self.models.get(&model.name).copied() else {
            self.push(Diag::new(
                E0,
                "E0020",
                format!("modelo desconhecido `{}`", model.name),
                model.span,
                "não declarado",
            ));
            for (_, v) in fields {
                self.expr(v);
            }
            return LTy::never();
        };
        let (oty, _) = self.resolve(out);
        // Ancoragem: o prompt recebe o inventário da fotografia confiável. O `refine` que decidir
        // sobre este rascunho confere que a política usa a mesma fonte (E0421/E0422).
        if fields.iter().any(|(_, v)| is_inventory_anchor(v)) {
            self.anchored.insert(e.span);
        }
        if !matches!(oty, Ty::Adt(_) | Ty::Text | Ty::List(..) | Ty::Int | Ty::Bool | Ty::Never) {
            self.push(Diag::new(
                E0,
                "E0021",
                "a saída de `infer` deve ser um tipo algébrico, `Int`, `Bool`, `Text` ou uma lista delimitada",
                out.span(),
                format!("encontrado `{}`", oty.show()),
            ));
        }
        let mut acc = LTy { ty: Ty::Unit, lab: self.pc, tok: None, origin: Vec::new(), refn: None };
        if self.pc < Label::Trusted {
            acc.origin.push("contexto de controle (pc)".into());
        }
        let mut tok_in: u64 = 8;
        let mut bounded = true;
        for (k, v) in fields {
            let quoted = matches!(v.kind, ExprKind::Quote(_));
            let t = self.expr(v);
            if t.lab == Label::Untrusted && !quoted {
                let mut d = Diag::new(
                    E3,
                    "E0415",
                    "dado Untrusted em posição de instrução",
                    v.span,
                    "entraria no prompt como instrução",
                );
                if !t.origin.is_empty() {
                    d = d.note(format!("origem: {}", t.origin.join(" → ")));
                }
                self.push(
                    d.help(format!("envolva com `quote(…)`: `{} = quote(…)` marca o conteúdo como dado", k.name)),
                );
            }
            let ft = match &t.ty {
                Ty::Text => match t.tok {
                    Some(n) => n,
                    None => {
                        bounded = false;
                        self.push(
                            Diag::new(E6, "B0110", "contagem de tokens ilimitada no prompt", v.span, "`Text` sem limite superior")
                                .help("limite com `fit(x, N)`: estouro de janela é erro de compilação, nunca truncamento silencioso"),
                        );
                        0
                    }
                },
                Ty::Int | Ty::Bool => 4,
                Ty::Adt(_) => 64,
                Ty::List(..) => {
                    match t.tok.or_else(|| ty_tokens(&t.ty, t.refn)) {
                        Some(n) => n,
                        None => {
                            bounded = false;
                            self.push(
                            Diag::new(E6, "B0110", "contagem de tokens ilimitada no prompt", v.span, "lista de `Text` sem limite superior")
                                .help("use elementos `Text[..n]`: o teto da lista vezes o teto do elemento limita o prompt"),
                        );
                            0
                        }
                    }
                }
                Ty::Never => 0,
                other => {
                    self.push(Diag::new(
                        E0,
                        "E0022",
                        format!("`{}` não pode entrar num prompt", other.show()),
                        v.span,
                        "tipo não serializável",
                    ));
                    0
                }
            };
            tok_in += 4 + ft;
            acc.meet_from(&t);
        }
        self.use_effect("Infer", e.span);
        let p = |k: &str| md.prop(k).unwrap_or(0.0);
        // Teto de saída derivado do tipo: bytes da codificação canônica (todo token cobre ≥ 1 byte).
        let max_out = p("max_out") as u64;
        let by_type = ath_grammar::shape_of(self.m, out).ok().and_then(|s| ath_grammar::max_bytes(&s));
        let (content_out, out_by_type) = match by_type {
            Some(b) if b < max_out => (b, true),
            _ => (max_out, false),
        };
        // Modelos de raciocínio gastam tokens antes do conteúdo: o contrato declara quantos.
        let tok_reasoning = p("reasoning") as u64;
        let tok_out = content_out + tok_reasoning;
        // Contrato Δ estendido: o provedor acrescenta template de chat e esquema de decodificação
        // ao prompt. O overhead declarado entra na janela, no orçamento e na reserva do ARX.
        let overhead = p("overhead") as u64;
        let prompt = tok_in;
        tok_in += overhead;
        if md.prop("overhead").is_some() {
            if let Ok(shape) = ath_grammar::shape_of(self.m, out) {
                let schema = ath_grammar::to_json_schema_with(&shape, ath_grammar::Dialect::Core).to_string();
                let need = est_tokens(&schema);
                if overhead < need {
                    self.push(
                        Diag::new(
                            E6,
                            "B0302",
                            "overhead do contrato não cobre o esquema de decodificação",
                            out.span(),
                            format!(
                                "esquema de `{}` ≈ {}; overhead declarado {}",
                                out.display(),
                                fmt::tok(need),
                                fmt::tok(overhead)
                            ),
                        )
                        .note(format!("o provedor injeta o esquema no prompt de cada amostra de `{}`", model.name))
                        .help(format!(
                            "eleve `overhead` no contrato de `{}` para ≥ {} mais o template de chat",
                            model.name,
                            fmt::int(need)
                        )),
                    );
                }
            }
        }
        let ctx = p("ctx") as u64;
        if bounded && tok_in + tok_out > ctx {
            let label = if overhead > 0 {
                format!(
                    "{} + {} de overhead + {} > {}",
                    fmt::tok(prompt),
                    fmt::tok(overhead),
                    fmt::tok(tok_out),
                    fmt::tok(ctx)
                )
            } else {
                format!("{} + {} > {}", fmt::tok(tok_in), fmt::tok(tok_out), fmt::tok(ctx))
            };
            self.push(Diag::new(E6, "B0301", "janela de contexto excedida", e.span, label));
        }
        let usd = tok_in as f64 * p("price_in") / 1e6 + tok_out as f64 * p("price_out") / 1e6;
        let wall = p("ttft") + tok_out as f64 * p("per_tok");
        let out_ty = match out.unlabeled() {
            r @ (TypeExpr::Refined { .. } | TypeExpr::List { .. }) => r.sig(),
            other => other.base_name().to_string(),
        };
        self.facts.infer_costs.insert(
            e.span,
            InferCost {
                model: model.name.clone(),
                out_ty,
                tok_in,
                tok_out,
                tok_reasoning,
                out_by_type,
                overhead,
                usd,
                wall,
            },
        );
        let refn = self.refn_of(out);
        LTy { ty: Ty::Draft(Box::new(oty), e.span), lab: acc.lab, tok: None, origin: acc.origin, refn }
            .with_step(format!("infer {}", model.name))
    }

    fn refine(&mut self, e: &Expr, draft: &Expr, policy: &Ident, _retry: u32, els: &Block) -> LTy {
        if self.in_policy {
            self.impure(e.span, "`refine`");
        }
        let d = self.expr(draft);
        let infer_span = match &d.ty {
            Ty::Draft(_, ispan) => Some(*ispan),
            _ => None,
        };
        let inner = match &d.ty {
            Ty::Draft(inner, ispan) => {
                self.facts.draft_src.insert(e.span, *ispan);
                (**inner).clone()
            }
            Ty::Never => Ty::Never,
            other => {
                self.push(Diag::new(
                    E0,
                    "E0023",
                    "`refine` exige um rascunho de `infer`",
                    draft.span,
                    format!("encontrado `{}`", other.show()),
                ));
                Ty::Never
            }
        };
        match self.policies.get(&policy.name).copied() {
            None => self.push(Diag::new(
                E0,
                "E0024",
                format!("política desconhecida `{}`", policy.name),
                policy.span,
                "não declarada",
            )),
            Some(p) => {
                // Inventário e política precisam vir da mesma fotografia confiável do host.
                if let Some(ispan) = infer_span {
                    let anchored = self.anchored.contains(&ispan);
                    if reads_snapshot(&p.body) && !anchored {
                        self.push(
                            Diag::new(
                                E3,
                                "E0421",
                                "a política decide pela fotografia confiável, mas o prompt não recebe o inventário",
                                ispan,
                                format!("`{}` lê `s.…`; o `infer` não tem um campo `= s.managed()`", policy.name),
                            )
                            .note(format!("decidido por `refine … by {}`", policy.name))
                            .help("passe o inventário ao modelo como `campo = s.managed()`: nunca copiado de literal, log ou resposta de modelo"),
                        );
                    }
                    if anchored && compares_text_literal(&p.body) {
                        self.push(
                            Diag::new(
                                E3,
                                "E0422",
                                "descompasso entre o inventário confiável e a política",
                                policy.span,
                                format!(
                                    "o prompt recebe `s.managed()`, mas `{}` compara com literais de texto",
                                    policy.name
                                ),
                            )
                            .help("decida pela mesma fotografia: `s.is_managed(x)` no lugar de `x == \"…\"`"),
                        );
                    }
                }
                if p.params.len() != 1 {
                    self.push(Diag::new(
                        E0,
                        "E0024",
                        "a política de `refine` recebe exatamente um argumento",
                        policy.span,
                        "aridade",
                    ));
                } else {
                    let (pt, _) = self.resolve(&p.params[0].ty);
                    if assignable(&inner, &pt) && !self.ty_within(&inner, &p.params[0].ty) {
                        // A política decide sobre o valor decodificado: o tipo do rascunho (com os
                        // limites das listas) precisa estar contido no domínio dela.
                        self.facts.refinement_checks += 1;
                        self.push(
                            Diag::new(
                                E5,
                                "E0450",
                                "refinamento não provado",
                                policy.span,
                                format!(
                                    "`{}` decide sobre `{}`, o rascunho produz `{}`",
                                    policy.name,
                                    p.params[0].ty.unlabeled().sig(),
                                    inner.show()
                                ),
                            )
                            .help("alinhe os limites: o domínio da política precisa conter a saída do `infer`"),
                        );
                    }
                    if !assignable(&inner, &pt) {
                        self.push(Diag::new(
                            E0,
                            "E0008",
                            "política incompatível com o rascunho",
                            policy.span,
                            format!(
                                "`{}` decide sobre `{}`, o rascunho produz `{}`",
                                policy.name,
                                pt.show(),
                                inner.show()
                            ),
                        ));
                    }
                }
            }
        }
        self.use_effect("Infer", e.span);
        let saved = (self.pc, self.pc_span);
        if d.lab < self.pc {
            self.pc = d.lab;
            self.pc_span = Some(draft.span);
        }
        let eb = self.block(els);
        (self.pc, self.pc_span) = saved;
        if unify(&eb.ty, &inner).is_none() {
            self.push(Diag::new(
                E0,
                "E0011",
                "o `else` de `refine` deve divergir ou produzir o tipo do rascunho",
                els.span,
                format!("esperado `{}` ou `return`, encontrado `{}`", inner.show(), eb.ty.show()),
            ));
        }
        let base = if d.lab >= Label::Endorsed { d.lab } else { Label::Endorsed };
        // A saída decodificada carrega o refinamento do tipo; um `else` com valor próprio não.
        let refn = if eb.ty.is_never() { d.refn } else { None };
        let mut out = LTy::declared(inner, base, refn, Vec::new());
        if !eb.ty.is_never() {
            out.meet_from(&eb);
        }
        out
    }
}
