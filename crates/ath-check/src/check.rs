//! Passada principal: E0 (estrutura), E2 (efeitos e capacidades) e E3 (integridade).
//! Também coleta os fatos de que E1 (linearidade) e E6 (orçamento) dependem.

use crate::diag::{Diag, Stratum};
use crate::fmt;
use crate::ty::{assignable, unify, LTy, Ty};
use ath_syntax::ast::*;
use ath_syntax::{est_tokens, Span};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Custo de uma amostra de um nó `infer`, sob o contrato Δ do modelo.
#[derive(Clone, Debug)]
pub struct InferCost {
    pub model: String,
    pub tok_in: u64,
    pub tok_out: u64,
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
    pub tool_calls: usize,
    pub effects: BTreeSet<String>,
}

pub const MODEL_PROPS: [&str; 6] = ["price_in", "price_out", "ctx", "max_out", "ttft", "per_tok"];
const BUILTINS: [&str; 2] = ["fit", "sample"];

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
    in_policy: bool,
    commit_ok: bool,
}

use Stratum::{E0, E2, E3, E6};

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
            in_policy: false,
            commit_ok: false,
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
                    n if self.types.contains_key(n) => Ty::Adt(n.to_string()),
                    n => {
                        self.push(Diag::new(E0, "E0004", format!("tipo desconhecido `{n}`"), id.span, "não declarado"));
                        Ty::Never
                    }
                };
                (t, None)
            }
            TypeExpr::Cap(effs, _) => (Ty::Cap(effs.iter().map(|e| e.name.clone()).collect()), None),
            TypeExpr::Labeled(inner, l, _) => {
                let (t, _) = self.resolve(inner);
                (t, Some(*l))
            }
        }
    }

    // ================================================================ declarações

    pub fn check_all(&mut self) {
        let m = self.m;
        let mut seen: HashMap<String, Span> = HashMap::new();
        for item in &m.items {
            let id = item.ident();
            if BUILTINS.contains(&id.name.as_str()) {
                self.push(Diag::new(E0, "E0002", format!("`{}` é uma primitiva reservada", id.name), id.span, "nome reservado"));
                continue;
            }
            if let Some(prev) = seen.get(&id.name).copied() {
                self.push(
                    Diag::new(E0, "E0002", format!("`{}` definido mais de uma vez", id.name), id.span, "redefinido aqui")
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
                                Diag::new(E0, "E0002", format!("variante `{}` definida mais de uma vez", v.name.name), v.name.span, "redefinida aqui")
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
                    self.push(Diag::new(E0, "E0004", "rótulo de integridade em campo de dado", f.ty.span(), "rótulos pertencem a valores, não a campos"));
                }
                if matches!(t, Ty::Cap(_) | Ty::IdemKey) {
                    self.push(Diag::new(E0, "E0004", "capacidades e chaves não podem ser armazenadas em dados", f.ty.span(), "não armazenável"));
                }
            }
        }
    }

    fn check_model(&mut self, d: &'m ModelDecl) {
        for key in MODEL_PROPS {
            if d.prop(key).is_none() {
                self.push(
                    Diag::new(E0, "E0030", format!("modelo `{}` sem `{}`", d.name.name, key), d.name.span, "contrato Δ incompleto")
                        .help(format!("declare {}", MODEL_PROPS.join(", "))),
                );
            }
        }
        for p in &d.props {
            if !MODEL_PROPS.contains(&p.key.name.as_str()) {
                self.push(Diag::new(E0, "E0030", format!("propriedade de modelo desconhecida `{}`", p.key.name), p.key.span, "desconhecida"));
            } else if p.value < 0.0 {
                self.push(Diag::new(E0, "E0030", "propriedade de modelo negativa", p.key.span, "deve ser ≥ 0"));
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
        self.resolve(&t.ret);
        if t.latency < 0.0 || t.usd < 0.0 {
            self.push(Diag::new(E0, "E0030", "latência e custo devem ser ≥ 0", t.name.span, "contrato Δ inválido"));
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
            self.push(Diag::new(E0, "E0008", "uma política devolve `Bool`", p.ret.span(), format!("encontrado `{}`", rt.show())));
        }
        let body = self.expr(&p.body);
        if !assignable(&body.ty, &Ty::Bool) {
            self.mismatch(p.body.span, &Ty::Bool, &body.ty);
        }
        self.in_policy = false;
        self.scopes.clear();
    }

    fn check_agent(&mut self, a: &'m AgentDecl) {
        for b in &a.budget {
            if !["tok", "usd", "wall"].contains(&b.key.name.as_str()) {
                self.push(
                    Diag::new(E0, "E0031", format!("dimensão de orçamento desconhecida `{}`", b.key.name), b.key.span, "desconhecida")
                        .help("use tok, usd ou wall"),
                );
            } else if b.value < 0.0 {
                self.push(Diag::new(E0, "E0031", "orçamento negativo", b.key.span, "deve ser ≥ 0"));
            }
        }
        let mut events = HashSet::new();
        for h in &a.handlers {
            if !events.insert(h.event.name.clone()) {
                self.push(Diag::new(E0, "E0002", format!("handler `on {}` definido mais de uma vez", h.event.name), h.event.span, "duplicado"));
            }
            self.scopes = vec![vec![]];
            self.pc = Label::Trusted;
            self.pc_span = None;
            self.used.clear();
            self.in_policy = false;
            for p in &a.params {
                let (t, _) = self.resolve(&p.ty);
                if !matches!(t, Ty::Cap(_) | Ty::Never) {
                    self.push(Diag::new(E0, "E0040", "parâmetros de agente são capacidades", p.ty.span(), format!("encontrado `{}`", t.show())));
                }
                self.bind(&p.name.name, LTy::trusted(t));
            }
            for p in &h.params {
                let (t, l) = self.resolve(&p.ty);
                if matches!(t, Ty::Cap(_) | Ty::IdemKey) {
                    self.push(Diag::new(E0, "E0041", "eventos não carregam capacidades nem chaves", p.ty.span(), "capacidades só vêm do agente"));
                }
                let lab = l.unwrap_or(Label::Trusted);
                let mut lt = LTy { ty: t, lab, tok: None, origin: Vec::new() };
                if lab < Label::Trusted {
                    lt.origin.push(format!("evento `{}({})` ^ {}", h.event.name, p.name.name, lab.name()));
                }
                self.bind(&p.name.name, lt);
            }
            let (rt, _) = self.resolve(&h.ret);
            self.ret = Some(rt.clone());
            let body = self.block(&h.body);
            if !assignable(&body.ty, &rt) {
                let sp = h.body.tail.as_ref().map(|t| t.span).unwrap_or(h.body.span);
                self.mismatch(sp, &rt, &body.ty);
            }
            let declared: BTreeSet<String> = h.effects.iter().map(|e| e.name.clone()).collect();
            let used = std::mem::take(&mut self.used);
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
                Some(t) => t,
                None => {
                    self.push(Diag::new(E0, "E0001", format!("variável desconhecida `{n}`"), e.span, "não encontrada neste escopo"));
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
                LTy { ty: want, tok: None, ..t }
            }
            ExprKind::If { cond, then, els } => self.if_expr(cond, then, els.as_ref()),
            ExprKind::Match { scrut, arms } => self.match_expr(e, scrut, arms),
            ExprKind::Block(b) => self.block(b),
            ExprKind::Return(v) => {
                if self.in_policy {
                    self.impure(e.span, "`return`");
                }
                let t = self.expr(v);
                if let Some(r) = self.ret.clone() {
                    if !assignable(&t.ty, &r) {
                        self.mismatch(v.span, &r, &t.ty);
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
                self.push(Diag::new(E2, "T0304", "`commit` exige uma chamada de ferramenta irrevocable", x.span, "não é chamada de ferramenta"));
                self.expr(x)
            }
        }
    }

    fn field(&mut self, recv: &Expr, f: &Ident) -> LTy {
        let r = self.expr(recv);
        let name = match &r.ty {
            Ty::Adt(n) => n.clone(),
            Ty::Never => return LTy::never(),
            other => {
                self.push(Diag::new(E0, "E0005", format!("`{}` não tem campos", other.show()), f.span, "campo inexistente"));
                return LTy::never();
            }
        };
        let decl: &'m TypeDecl = self.types[&name];
        let mut found: Option<Ty> = None;
        let mut ok = true;
        for v in &decl.variants {
            match v.fields.iter().find(|fd| fd.name.name == f.name) {
                Some(fd) => {
                    let (t, _) = self.resolve(&fd.ty);
                    match &found {
                        None => found = Some(t),
                        Some(prev) if crate::ty::same(prev, &t) => {}
                        _ => ok = false,
                    }
                }
                None => ok = false,
            }
        }
        match (ok, found) {
            (true, Some(t)) => LTy { ty: t, lab: r.lab, tok: None, origin: r.origin },
            _ => {
                self.push(
                    Diag::new(E0, "E0005", format!("campo `{}` não existe em todas as variantes de `{}`", f.name, name), f.span, "campo ausente em alguma variante")
                        .help("use `match` para acessar campos de variantes específicas"),
                );
                LTy::never()
            }
        }
    }

    fn method(&mut self, recv: &Expr, method: &Ident, args: &[Expr]) -> LTy {
        let r = self.expr(recv);
        if method.name != "mint" {
            self.push(Diag::new(E0, "E0003", format!("método desconhecido `{}`", method.name), method.span, "o Marco Zero só conhece `mint()`"));
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
                Diag::new(E2, "E0502", "capacidade insuficiente", recv.span, format!("`{}` não inclui IdemKey.Mint", other.show()))
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
                        self.push(Diag::new(E0, "E0006", "`fit` exige um inteiro literal positivo", args[1].span, "limite de tokens"));
                        0
                    }
                };
                return LTy { ty: Ty::Text, lab: t.lab, tok: Some(n), origin: t.origin };
            }
            "sample" => {
                if args.len() != 1 {
                    self.push(Diag::new(E0, "E0007", "`sample(rascunho)` recebe 1 argumento", e.span, "aridade"));
                    return LTy::never();
                }
                if self.in_policy {
                    self.impure(e.span, "`sample`");
                }
                let LTy { ty, lab, origin, .. } = self.expr(&args[0]);
                return match ty {
                    Ty::Draft(inner, ispan) => {
                        self.facts.draft_src.insert(e.span, ispan);
                        self.use_effect("Infer", e.span);
                        LTy { ty: *inner, lab, tok: None, origin }.with_step("sample(…)")
                    }
                    Ty::Never => LTy::never(),
                    other => {
                        self.push(Diag::new(E0, "E0023", "`sample` exige um rascunho de `infer`", args[0].span, format!("encontrado `{}`", other.show())));
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
                self.push(Diag::new(E0, "E0007", format!("`{}` recebe {} argumento(s)", callee.name, p.params.len()), e.span, "aridade"));
            }
            let mut out = LTy::trusted(Ty::Bool);
            for (prm, a) in p.params.iter().zip(args) {
                let at = self.expr(a);
                let (pt, _) = self.resolve(&prm.ty);
                if !assignable(&at.ty, &pt) {
                    self.mismatch(a.span, &pt, &at.ty);
                }
                out.meet_from(&at);
            }
            return out;
        }
        self.push(Diag::new(E0, "E0001", format!("função desconhecida `{}`", callee.name), callee.span, "não é ferramenta, política nem primitiva"));
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
        match (tool.class, committed) {
            (EffClass::Irrevocable, false) => self.push(
                Diag::new(E2, "T0301", "efeito irrevogável fora de `commit`", callee.span, format!("`{}` é irrevocable", callee.name))
                    .help(format!("escreva `commit {}(…)`: irrevogáveis só depois de toda validação", callee.name)),
            ),
            (c, true) if c != EffClass::Irrevocable => self.push(
                Diag::new(E2, "T0303", "`commit` sobre efeito revogável", callee.span, format!("`{}` é {}", callee.name, c.name()))
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
        for (p, a) in tool.params.iter().zip(args) {
            let at = self.expr(a);
            let (pt, req) = self.resolve(&p.ty);
            if !assignable(&at.ty, &pt) {
                if let (Ty::Cap(have), Ty::Cap(need)) = (&at.ty, &pt) {
                    let missing: Vec<String> = need.difference(have).cloned().collect();
                    self.push(
                        Diag::new(E2, "E0502", "capacidade insuficiente", a.span, format!("falta {}", missing.join(", ")))
                            .note(format!("tem: {}", at.ty.show()))
                            .note(format!("exigido: {}({}: {})", callee.name, p.name.name, pt.show()))
                            .help("capacidades só vêm dos parâmetros do agente; passe uma que inclua o efeito"),
                    );
                } else {
                    self.mismatch(a.span, &pt, &at.ty);
                }
                continue;
            }
            if let Some(req) = req {
                self.facts.integrity_checks += 1;
                let eff = at.lab.meet(self.pc);
                if eff < req {
                    let implicit = at.lab >= req;
                    let mut d = Diag::new(E3, "E0412", "fluxo de integridade violado", a.span, format!("integridade {}", eff.name()));
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
        let mut out = LTy { ty: rt, lab, tok: None, origin: Vec::new() };
        if lab < Label::Trusted {
            out.origin.push(format!("{}(…) devolve ^ {}", callee.name, lab.name()));
        }
        out
    }

    fn ctor(&mut self, e: &Expr, name: &Ident, fields: &[(Ident, Expr)]) -> LTy {
        let Some((td, v)) = self.variants.get(&name.name).copied() else {
            self.push(Diag::new(E0, "E0009", format!("construtor desconhecido `{}`", name.name), name.span, "nenhum tipo declara esta variante"));
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
                    }
                    out.meet_from(&at);
                }
                None => self.push(Diag::new(E0, "E0010", format!("`{}` não tem campo `{}`", name.name, fname.name), fname.span, "campo inexistente")),
            }
        }
        for fd in &v.fields {
            if !fields.iter().any(|(n, _)| n.name == fd.name.name) {
                self.push(Diag::new(E0, "E0010", format!("campo `{}` ausente em `{}`", fd.name.name, name.name), e.span, "campo obrigatório"));
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
                    self.push(Diag::new(E0, "E0008", "tipo não comparável", e.span, format!("`{}` não admite `{}`", lt.ty.show(), op.symbol())));
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
                        self.push(Diag::new(E0, "E0011", "ramos com tipos diferentes", b.span, format!("`{}` vs `{}`", t.ty.show(), f.ty.show())));
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
                self.push(Diag::new(E0, "E0008", "`match` exige um tipo algébrico", scrut.span, format!("encontrado `{}`", other.show())));
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
                self.push(Diag::new(E0, "E0009", format!("`{}` não é variante de `{}`", arm.variant.name, tname), arm.variant.span, "variante inexistente"));
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
                        self.bind(&b.name, LTy { ty: ft, lab: s.lab, tok: None, origin: s.origin.clone() });
                    }
                    None => {
                        self.push(Diag::new(E0, "E0010", format!("`{}` não tem campo `{}`", v.name.name, b.name), b.span, "campo inexistente"));
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
                        self.push(Diag::new(E0, "E0011", "braços com tipos diferentes", arm.body.span, format!("`{}` vs `{}`", prev.ty.show(), bt.ty.show())));
                        prev
                    }
                },
            });
        }
        (self.pc, self.pc_span) = saved;
        let missing: Vec<String> = decl.variants.iter().filter(|v| !seen.contains(&v.name.name)).map(|v| v.name.name.clone()).collect();
        if !missing.is_empty() {
            self.push(Diag::new(E0, "E0012", "`match` não exaustivo", e.span, format!("faltam: {}", missing.join(", "))));
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
            self.push(Diag::new(E0, "E0020", format!("modelo desconhecido `{}`", model.name), model.span, "não declarado"));
            for (_, v) in fields {
                self.expr(v);
            }
            return LTy::never();
        };
        let (oty, _) = self.resolve(out);
        if !matches!(oty, Ty::Adt(_) | Ty::Text | Ty::Never) {
            self.push(Diag::new(E0, "E0021", "a saída de `infer` deve ser um tipo algébrico ou `Text`", out.span(), format!("encontrado `{}`", oty.show())));
        }
        let mut acc = LTy { ty: Ty::Unit, lab: self.pc, tok: None, origin: Vec::new() };
        if self.pc < Label::Trusted {
            acc.origin.push("contexto de controle (pc)".into());
        }
        let mut tok_in: u64 = 8;
        let mut bounded = true;
        for (k, v) in fields {
            let quoted = matches!(v.kind, ExprKind::Quote(_));
            let t = self.expr(v);
            if t.lab == Label::Untrusted && !quoted {
                let mut d = Diag::new(E3, "E0415", "dado Untrusted em posição de instrução", v.span, "entraria no prompt como instrução");
                if !t.origin.is_empty() {
                    d = d.note(format!("origem: {}", t.origin.join(" → ")));
                }
                self.push(d.help(format!("envolva com `quote(…)`: `{} = quote(…)` marca o conteúdo como dado", k.name)));
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
                Ty::Never => 0,
                other => {
                    self.push(Diag::new(E0, "E0022", format!("`{}` não pode entrar num prompt", other.show()), v.span, "tipo não serializável"));
                    0
                }
            };
            tok_in += 4 + ft;
            acc.meet_from(&t);
        }
        self.use_effect("Infer", e.span);
        let p = |k: &str| md.prop(k).unwrap_or(0.0);
        let tok_out = p("max_out") as u64;
        let ctx = p("ctx") as u64;
        if bounded && tok_in + tok_out > ctx {
            self.push(Diag::new(
                E6,
                "B0301",
                "janela de contexto excedida",
                e.span,
                format!("{} + {} > {}", fmt::tok(tok_in), fmt::tok(tok_out), fmt::tok(ctx)),
            ));
        }
        let usd = tok_in as f64 * p("price_in") / 1e6 + tok_out as f64 * p("price_out") / 1e6;
        let wall = p("ttft") + tok_out as f64 * p("per_tok");
        self.facts.infer_costs.insert(e.span, InferCost { model: model.name.clone(), tok_in, tok_out, usd, wall });
        LTy { ty: Ty::Draft(Box::new(oty), e.span), lab: acc.lab, tok: None, origin: acc.origin }.with_step(format!("infer {}", model.name))
    }

    fn refine(&mut self, e: &Expr, draft: &Expr, policy: &Ident, _retry: u32, els: &Block) -> LTy {
        if self.in_policy {
            self.impure(e.span, "`refine`");
        }
        let d = self.expr(draft);
        let inner = match &d.ty {
            Ty::Draft(inner, ispan) => {
                self.facts.draft_src.insert(e.span, *ispan);
                (**inner).clone()
            }
            Ty::Never => Ty::Never,
            other => {
                self.push(Diag::new(E0, "E0023", "`refine` exige um rascunho de `infer`", draft.span, format!("encontrado `{}`", other.show())));
                Ty::Never
            }
        };
        match self.policies.get(&policy.name).copied() {
            None => self.push(Diag::new(E0, "E0024", format!("política desconhecida `{}`", policy.name), policy.span, "não declarada")),
            Some(p) => {
                if p.params.len() != 1 {
                    self.push(Diag::new(E0, "E0024", "a política de `refine` recebe exatamente um argumento", policy.span, "aridade"));
                } else {
                    let (pt, _) = self.resolve(&p.params[0].ty);
                    if !assignable(&inner, &pt) {
                        self.push(Diag::new(
                            E0,
                            "E0008",
                            "política incompatível com o rascunho",
                            policy.span,
                            format!("`{}` decide sobre `{}`, o rascunho produz `{}`", policy.name, pt.show(), inner.show()),
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
        let mut out = LTy { ty: inner, lab: base, tok: None, origin: Vec::new() };
        if !eb.ty.is_never() {
            out.meet_from(&eb);
        }
        out
    }
}
