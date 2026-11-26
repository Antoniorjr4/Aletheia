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

/// O menor intervalo que contém os dois (ramos de `if`/`match`); sem os dois, nenhum.
fn int_hull(a: Option<Refn>, b: Option<Refn>) -> Option<Refn> {
    match (a, b) {
        (Some(Refn::Int { lo: l1, hi: h1 }), Some(Refn::Int { lo: l2, hi: h2 })) => {
            Some(Refn::Int { lo: l1.zip(l2).map(|(x, y)| x.min(y)), hi: h1.zip(h2).map(|(x, y)| x.max(y)) })
        }
        _ => None,
    }
}

/// Aritmética de intervalos fechados. `None` se um limite falta, se a conta pode sair de `i64`
/// ou se o divisor pode ser zero: nesses casos nada é provado.
fn int_arith(op: BinOp, a: Option<Refn>, b: Option<Refn>) -> Option<Refn> {
    let (Some(Refn::Int { lo: Some(al), hi: Some(ah) }), Some(Refn::Int { lo: Some(bl), hi: Some(bh) })) = (a, b)
    else {
        return None;
    };
    let corners = |f: fn(i64, i64) -> Option<i64>| -> Option<(i64, i64)> {
        let c = [f(al, bl)?, f(al, bh)?, f(ah, bl)?, f(ah, bh)?];
        Some((*c.iter().min()?, *c.iter().max()?))
    };
    let (lo, hi) = match op {
        BinOp::Add => (al.checked_add(bl)?, ah.checked_add(bh)?),
        BinOp::Sub => (al.checked_sub(bh)?, ah.checked_sub(bl)?),
        BinOp::Mul => corners(i64::checked_mul)?,
        BinOp::Div => {
            if bl <= 0 && bh >= 0 {
                return None;
            }
            corners(i64::checked_div)?
        }
        _ => return None,
    };
    Some(Refn::Int { lo: Some(lo), hi: Some(hi) })
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
        // `contains("ana bia", nome)`: o mesmo inventário escrito à mão, com outra função.
        // (Literal no segundo argumento é um padrão procurado no valor, não um inventário.)
        ExprKind::Call { callee, args } if callee.name == "contains" => {
            matches!(args.first().map(|a| &a.kind), Some(ExprKind::Str(_)))
        }
        _ => false,
    })
}

/// Os inventários confiáveis em que a política decide: `s.is_managed`/`s.scale_justified`
/// decidem pelo inventário `managed`; `s.member("k", x)` e `s.links_within("k", t)`, pelo inventário `k`.
fn policy_inventories(e: &Expr) -> BTreeSet<String> {
    let found = std::cell::RefCell::new(BTreeSet::new());
    expr_any(e, &|x| {
        match (snapshot_method(x), &x.kind) {
            (Some("is_managed" | "scale_justified"), _) => {
                found.borrow_mut().insert("managed".to_string());
            }
            (Some("member" | "links_within"), ExprKind::Method { args, .. }) => {
                if let Some(Expr { kind: ExprKind::Str(k), .. }) = args.first() {
                    found.borrow_mut().insert(k.clone());
                }
            }
            _ => {}
        }
        false
    });
    found.into_inner()
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
    /// v1.0-RC10: verificações de fluxo de confidencialidade (argumento → sink).
    pub confidentiality_checks: usize,
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
    /// Ciclo 1: campos de `infer` que entregam ao modelo uma variável ligada por `let` a
    /// `s.inventory("k")` ou `s.managed()`: span da variável → a expressão da fotografia. O
    /// compilador substitui a variável pela expressão antes de entregar a árvore aos backends,
    /// para que os dois runtimes rotulem o campo como fotografia confiável, como na forma direta.
    pub anchor_vars: Vec<(Span, Expr)>,
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
const BUILTINS: [&str; 9] =
    ["fit", "sample", "len", "text", "join", "contains", "lower", "ascii_letters", "ascii_digits"];

pub struct Ctx<'m> {
    pub m: &'m Module,
    pub types: HashMap<String, &'m TypeDecl>,
    pub variants: HashMap<String, (&'m TypeDecl, &'m Variant)>,
    /// v1.0-RC8: o `refine` que inicializa o `let` em curso (só ele pode ter `keeping … as`)
    let_init: Option<Span>,
    keep_bind: Option<(Span, Ident, LTy)>,
    pub tools: HashMap<String, &'m ToolDecl>,
    pub models: HashMap<String, &'m ModelDecl>,
    pub policies: HashMap<String, &'m PolicyDecl>,
    pub diags: Vec<Diag>,
    pub facts: Facts,
    scopes: Vec<Vec<(String, LTy)>>,
    /// Ligações `let` cujo valor é a fotografia confiável, pela posição (profundidade, índice) da
    /// ligação em `scopes`. Só o `let` de um bloco cria entrada; o bloco as apaga ao sair.
    let_anchor: HashMap<(usize, usize), Expr>,
    pc: Label,
    /// v1.0-RC10: confidencialidade do contexto de controle (ramo decidido por dado secreto).
    pc_conf: Conf,
    pc_span: Option<Span>,
    pc_conf_span: Option<Span>,
    used: BTreeMap<String, Span>,
    ret: Option<Ty>,
    ret_refn: Option<Refn>,
    in_policy: bool,
    /// D15: como `pc`, mas só rebaixado por `if`/`match`/`for` sobre dado; o `else` de um
    /// `refine` é o veredito de uma política auditada e não o rebaixa.
    pc_data: Label,
    /// dentro de um contrato `requires` (onde `s.authorizes` é permitido)
    in_requires: bool,
    commit_ok: bool,
    /// Pilha de blocos `atomic` abertos; cada um guarda as chamadas compensáveis que contém.
    atomic_stack: Vec<Vec<Span>>,
    /// Dentro de `handle timeout_unknown`.
    in_handle: bool,
    /// Ao checar condições de controle, o resultado de uma ferramenta herda a integridade dos argumentos.
    taint_calls: bool,
    /// `infer`s (por span) e os inventários confiáveis que o seu prompt recebe
    /// (`s.managed()`, `s.inventory("k")`).
    anchored: HashMap<Span, BTreeSet<String>>,
    /// as operações de `s.authorizes("op", …)` nos contratos, com o lugar
    auth_ops: Vec<(String, Span)>,
}

use Stratum::{E0, E1, E2, E3, E5, E6};

impl<'m> Ctx<'m> {
    pub fn new(m: &'m Module) -> Self {
        Ctx {
            m,
            types: HashMap::new(),
            variants: HashMap::new(),
            let_init: None,
            keep_bind: None,
            tools: HashMap::new(),
            models: HashMap::new(),
            policies: HashMap::new(),
            diags: Vec::new(),
            facts: Facts::default(),
            scopes: Vec::new(),
            let_anchor: HashMap::new(),
            pc: Label::Trusted,
            pc_conf: Conf::Public,
            pc_span: None,
            pc_conf_span: None,
            used: BTreeMap::new(),
            ret: None,
            ret_refn: None,
            in_policy: false,
            pc_data: Label::Trusted,
            in_requires: false,
            commit_ok: false,
            atomic_stack: Vec::new(),
            in_handle: false,
            taint_calls: false,
            anchored: HashMap::new(),
            auth_ops: Vec::new(),
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

    /// v1.0-RC10: a confidencialidade declarada num tipo (`^ Secret`), se houver.
    pub fn conf_of(&self, te: &TypeExpr) -> Option<Conf> {
        match te {
            TypeExpr::ConfLabeled(_, c, _) => Some(*c),
            TypeExpr::Labeled(inner, ..) | TypeExpr::Own(inner, _) => self.conf_of(inner),
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

    /// **Ciclo 1 (revisão independente, achado 2).** Numa política, toda conta tem de ter o
    /// resultado limitado pelos tipos: a aritmética é de 64 bits com volta, e uma volta poderia
    /// transformar uma recusa em aceitação. Sem intervalo provado, o programa é recusado aqui.
    fn unbounded_arith(&mut self, span: Span, what: &str) {
        if !self.in_policy {
            return;
        }
        self.push(
            Diag::new(
                E5,
                "E0451",
                "aritmética sem limite provado numa política",
                span,
                format!("o resultado de {what} pode sair de 64 bits ou dividir por zero"),
            )
            .note("a política decide o que é executado: um estouro não pode virar aceitação")
            .help("declare limites nos tipos dos operandos (`Int[a..b]`, `List[T, 0..n]`); divida só por valor que o tipo prova diferente de zero"),
        );
    }

    /// A expressão da fotografia a que `e` equivale, se `e` for `s.inventory("k")`/`s.managed()`
    /// ou uma variável cuja ligação visível é um `let` de uma delas (direto ou por outra variável).
    /// A fotografia é capturada uma vez por execução e é imutável nos dois runtimes, e `let` não
    /// se reatribui: ler a variável é ler a fotografia.
    fn snapshot_expr(&self, e: &Expr) -> Option<Expr> {
        if e.inventory_anchor().is_some() {
            return Some(e.clone());
        }
        let ExprKind::Var(name) = &e.kind else { return None };
        for (d, s) in self.scopes.iter().enumerate().rev() {
            for (i, (n, _)) in s.iter().enumerate().rev() {
                if n == name {
                    // A ligação visível é esta: se não for um `let` da fotografia, não é âncora,
                    // mesmo que uma ligação mais externa com o mesmo nome fosse.
                    return self.let_anchor.get(&(d, i)).cloned();
                }
            }
        }
        None
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
            TypeExpr::ConfLabeled(inner, _, _) => self.resolve(inner),
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
                        if let Some((pd, pv)) = self.variants.get(&v.name.name).copied() {
                            let mut diag = Diag::new(
                                E0,
                                "E0002",
                                format!("variante `{}` definida mais de uma vez", v.name.name),
                                v.name.span,
                                "redefinida aqui",
                            )
                            .secondary(pv.name.span, "primeira definição");
                            // Nomes de variante são do módulo, não do tipo: diga qual nome usar.
                            if pd.name.name != d.name.name {
                                diag = diag
                                    .note(format!(
                                        "`{}` já é variante de `{}`; nomes de variante valem para o módulo inteiro",
                                        v.name.name, pd.name.name
                                    ))
                                    .help(format!(
                                        "dê a esta variante um nome próprio, como `{}{}`; para um total sobre uma lista, \
                                         use `sum x in xs => …` em vez de slots fixos com `{}`",
                                        v.name.name, d.name.name, v.name.name
                                    ));
                            }
                            self.push(diag);
                        } else {
                            self.variants.insert(v.name.name.clone(), (d, v));
                        }
                    }
                }
                Item::Model(d) => {
                    self.models.insert(d.name.name.clone(), d);
                }
                Item::Authority(d) => self.check_key("autoridade", &d.name.name, &d.key, d.key_span),
                Item::Witness(d) => self.check_key("testemunha", &d.name.name, &d.key, d.key_span),
                Item::Bind(_) => {}
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
        self.check_authority_scope();
    }

    /// A forma da chave é conferida aqui; o ponto da curva, na carga (`arx::sign`).
    fn check_key(&mut self, what: &str, name: &str, key: &str, span: Span) {
        let hex = key.strip_prefix("ed25519:");
        if !hex.is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())) {
            self.push(
                Diag::new(
                    E0,
                    "E0426",
                    format!("chave inválida na {what} `{name}`"),
                    span,
                    "esperado `ed25519:` e 64 dígitos hexadecimais",
                )
                .help("gere um par com `athc keygen --out chave` e declare a chave pública impressa"),
            );
        }
    }

    /// **A ligação nativa (RC5).** Cada rota de um `bind` nomeia uma ferramenta declarada
    /// (`E0431`), ligada uma só vez (`E0432`). Um segredo não viaja em claro para outro host
    /// (`E0433`), e uma operação com chave, que muda o mundo, não é um `GET` (`E0434`).
    fn check_binds(&mut self) {
        let binds: Vec<&BindDecl> =
            self.m.items.iter().filter_map(|i| if let Item::Bind(b) = i { Some(b) } else { None }).collect();
        let mut seen: Vec<&Ident> = Vec::new();
        for b in binds {
            if let (BindUrl::Lit(u), Some(secret)) = (&b.url, &b.auth) {
                if u.starts_with("http://") && !ath_syntax::ast::loopback_url(u) {
                    self.push(
                        Diag::new(
                            E2,
                            "E0433",
                            format!("o segredo `{secret}` de `bind {}` viajaria em claro", b.name.name),
                            b.url_span,
                            "http sem TLS para outro host",
                        )
                        .help("use `https://`, ou ligue ao próprio host (127.0.0.1)"),
                    );
                }
            }
            if let (BindUrl::Lit(u), None) = (&b.url, &b.mcp) {
                if ath_syntax::ast::url_authority(u).is_none() {
                    self.push(Diag::new(
                        E0,
                        "E0431",
                        format!("a url de `bind {}` não é http(s), ou carrega credenciais", b.name.name),
                        b.url_span,
                        "esperado `http://…`, `https://…` ou `env(\"NOME\")`",
                    ));
                }
            }
            for (t, route) in &b.routes {
                let Some(decl) = self.tools.get(t.name.as_str()).copied() else {
                    self.push(
                        Diag::new(
                            E0,
                            "E0431",
                            format!("`bind {}` liga `{}`, que não é uma ferramenta do módulo", b.name.name, t.name),
                            t.span,
                            "ferramenta não declarada",
                        )
                        .help("cada rota de um `bind` tem o nome de uma `tool` (fora `url`, `auth` e `status`)"),
                    );
                    continue;
                };
                if seen.iter().any(|s| s.name == t.name) {
                    self.push(Diag::new(
                        E0,
                        "E0432",
                        format!("a ferramenta `{}` é ligada mais de uma vez", t.name),
                        t.span,
                        "segunda ligação",
                    ));
                }
                seen.push(t);
                let keyed = decl.params.iter().any(|p| p.ty.unlabeled().base_name() == "IdemKey");
                // v1.0-RC6: um parâmetro do caminho é um parâmetro de dados da ferramenta
                let data = |n: &str| {
                    decl.params
                        .iter()
                        .any(|p| p.name.name == n && matches!(p.ty.unlabeled().base_name(), "Text" | "Int"))
                };
                let finder = b.finds.iter().find(|(f, _)| f.name == t.name).map(|(_, r)| r);
                for r in std::iter::once(route).chain(finder) {
                    for n in r.path_params() {
                        if !data(n) {
                            self.push(
                                Diag::new(
                                    E0,
                                    "E0435",
                                    format!("a rota de `{}` usa `{{{n}}}`, que não é um parâmetro `Text` ou `Int` da ferramenta", t.name),
                                    t.span,
                                    "parâmetro de caminho sem origem",
                                )
                                .help("o caminho só recebe dados que o programa passa à ferramenta; o host é sempre o da `url`"),
                            );
                        }
                    }
                }
                let marker = b.keys.iter().find(|(k, _)| k.name == t.name).map(|(_, p)| p.as_str());
                let text =
                    |n: &str| decl.params.iter().any(|p| p.name.name == n && p.ty.unlabeled().base_name() == "Text");
                if let Some(p) = marker {
                    if !keyed || !text(p) || route.path_params().contains(&p) {
                        self.push(
                            Diag::new(
                                E0,
                                "E0436",
                                format!("`{}.key = \"{p}\"`: a chave viaja num parâmetro `Text` do corpo de uma ferramenta com `own IdemKey`", t.name),
                                t.span,
                                "marcador de chave sem lugar",
                            )
                            .help("declare a chave na ferramenta e escolha um parâmetro de texto que o provedor guarda e devolve"),
                        );
                    }
                }
                if finder.is_none() && b.settles.iter().any(|(k, _)| k.name == t.name) {
                    self.push(Diag::new(
                        E0,
                        "E0436",
                        format!("`{}.settle` exige `{}.find`", t.name, t.name),
                        t.span,
                        "espera sem consulta",
                    ));
                }
                if let Some(f) = finder {
                    if marker.is_none() || f.method != "GET" {
                        self.push(
                            Diag::new(
                                E0,
                                "E0436",
                                format!("`{}.find` é um `GET` e exige `{}.key`", t.name, t.name),
                                t.span,
                                "consulta por chave sem marcador",
                            )
                            .help("a consulta acha a operação pelo marcador que `.key` põe no parâmetro"),
                        );
                    }
                }
                if route.method == "GET" && (keyed || decl.class != EffClass::Reversible) {
                    self.push(
                        Diag::new(
                            E2,
                            "E0434",
                            format!("`{}` muda o mundo e está ligada a um `GET`", t.name),
                            t.span,
                            "operação com chave, ou não `reversible`",
                        )
                        .help("`GET` é para leituras; ligue a operação a `POST`, `PUT` ou `PATCH`"),
                    );
                }
            }
            // v1.0-RC9: as chamadas de um `bind` MCP
            for (t, call) in &b.calls {
                let Some(decl) = self.tools.get(t.name.as_str()).copied() else {
                    self.push(
                        Diag::new(
                            E0,
                            "E0431",
                            format!("`bind {}` liga `{}`, que não é uma ferramenta do módulo", b.name.name, t.name),
                            t.span,
                            "ferramenta não declarada",
                        )
                        .help("cada chamada de um `bind` tem o nome de uma `tool` (fora `mcp`, `auth` e `arg.…`)"),
                    );
                    continue;
                };
                if seen.iter().any(|s| s.name == t.name) {
                    self.push(Diag::new(
                        E0,
                        "E0432",
                        format!("a ferramenta `{}` é ligada mais de uma vez", t.name),
                        t.span,
                        "segunda ligação",
                    ));
                }
                seen.push(t);
                self.check_mcp_call(b, t, decl, call, false);
                let finder = b.call_finds.iter().find(|(f, _)| f.name == t.name).map(|(_, c)| c);
                if let Some(f) = finder {
                    self.check_mcp_call(b, t, decl, f, true);
                }
                let keyed = decl.params.iter().any(|p| p.ty.unlabeled().base_name() == "IdemKey");
                let marker = b.keys.iter().find(|(k, _)| k.name == t.name).map(|(_, p)| p.as_str());
                let text =
                    |n: &str| decl.params.iter().any(|p| p.name.name == n && p.ty.unlabeled().base_name() == "Text");
                if let Some(p) = marker {
                    let sent_as_text = call.args.iter().any(|(_, a)| a == &McpArg::Param(p.to_string()));
                    if !keyed || !text(p) || !sent_as_text {
                        self.push(
                            Diag::new(
                                E0,
                                "E0436",
                                format!("`{}.key = \"{p}\"`: a chave viaja num parâmetro `Text` que a chamada envia, numa ferramenta com `own IdemKey`", t.name),
                                t.span,
                                "marcador de chave sem lugar",
                            )
                            .help("declare a chave na ferramenta e escolha um parâmetro de texto que o servidor guarda e devolve"),
                        );
                    }
                }
                if finder.is_none() && b.settles.iter().any(|(k, _)| k.name == t.name) {
                    self.push(Diag::new(
                        E0,
                        "E0436",
                        format!("`{}.settle` exige `{}.find`", t.name, t.name),
                        t.span,
                        "espera sem consulta",
                    ));
                }
                if let Some(f) = finder {
                    if marker.is_none() || f.list.is_none() {
                        self.push(
                            Diag::new(
                                E0,
                                "E0436",
                                format!("`{}.find` lê uma lista (`-> lista[]…`) e exige `{}.key`", t.name, t.name),
                                t.span,
                                "consulta por chave sem marcador",
                            )
                            .help("a consulta acha a operação pelo marcador que `.key` põe no parâmetro"),
                        );
                    }
                }
            }
            for (t, _) in &b.call_finds {
                if !b.calls.iter().any(|(c, _)| c.name == t.name) {
                    self.push(Diag::new(
                        E0,
                        "E0436",
                        format!("`{}.find` sem a chamada de `{}`", t.name, t.name),
                        t.span,
                        "consulta de uma ferramenta não ligada",
                    ));
                }
            }
        }
    }

    /// v1.0-RC9: uma chamada MCP. Cada valor é um parâmetro de dados da ferramenta, `int(p)` de
    /// um parâmetro `Text` ou `Int`, um `@arg` do `bind`, ou um literal (`E0435`). A chamada
    /// principal envia todos os parâmetros de dados (nada que o programa passa fica para trás) e
    /// lê um caminho do resultado se a ferramenta devolve um valor (`E0439`). O que o servidor
    /// anuncia é conferido contra isto na ligação, antes de qualquer efeito (`R0646`).
    fn check_mcp_call(&mut self, b: &BindDecl, t: &Ident, decl: &ToolDecl, call: &McpCall, find: bool) {
        let data = |n: &str| {
            decl.params.iter().find(|p| p.name.name == n && matches!(p.ty.unlabeled().base_name(), "Text" | "Int"))
        };
        for (k, a) in &call.args {
            let bad = match a {
                McpArg::Param(p) | McpArg::IntOf(p) => data(p)
                    .is_none()
                    .then(|| format!("`{k}: {p}` usa `{p}`, que não é um parâmetro `Text` ou `Int` da ferramenta")),
                McpArg::Arg(x) => (!b.args.iter().any(|(n, _)| &n.name == x))
                    .then(|| format!("`{k}: @{x}` usa `@{x}`, que não é um `arg.{x}` do `bind`")),
                _ => None,
            };
            if let Some(msg) = bad {
                self.push(
                    Diag::new(E0, "E0435", format!("a chamada de `{}`: {msg}", t.name), t.span, "argumento sem origem")
                        .help("uma chamada só envia dados que o programa passa à ferramenta, configuração do executor (`@arg`) e literais"),
                );
            }
        }
        if find {
            return;
        }
        let sent = call.params();
        for p in &decl.params {
            if matches!(p.ty.unlabeled().base_name(), "Text" | "Int") && !sent.contains(&p.name.name.as_str()) {
                self.push(
                    Diag::new(
                        E0,
                        "E0439",
                        format!("a chamada de `{}` não envia `{}`", t.name, p.name.name),
                        t.span,
                        "parâmetro que não chega ao servidor",
                    )
                    .help("todo dado que o programa passa à ferramenta vai na chamada: nomeie-o num argumento"),
                );
            }
        }
        if decl.ret.unlabeled().base_name() != "Unit" && !call.projected {
            self.push(
                Diag::new(
                    E0,
                    "E0439",
                    format!("`{}` devolve `{}`, e a chamada não diz o que ler do resultado", t.name, decl.ret.unlabeled().sig()),
                    t.span,
                    "resultado sem caminho",
                )
                .help("termine a chamada com `-> campo` (ou `-> a.b`); o tipo do campo é conferido contra o esquema de saída do servidor"),
            );
        }
    }

    /// **A autoridade tipada (RC4).** Cada operação de `ops = [..]` é resolvida contra os
    /// contratos `requires s.authorizes("op", …)` do módulo: inexistente é `E0427`, repetida é
    /// `E0428`. `clock` e `outcomes` são resolvidas contra as testemunhas declaradas (`E0429`);
    /// a testemunha de tempo declara a tolerância de atraso admitida.
    fn check_typed_authority(&mut self, d: &AuthorityDecl, used: &[(String, Span)]) {
        let mut seen: Vec<&Ident> = Vec::new();
        for op in &d.op_idents {
            if let Some(first) = seen.iter().find(|s| s.name == op.name) {
                let first = first.span;
                self.push(
                    Diag::new(
                        E2,
                        "E0428",
                        format!("operação `{}` repetida na autoridade `{}`", op.name, d.name.name),
                        op.span,
                        "repetida",
                    )
                    .secondary(first, "primeira declaração"),
                );
                continue;
            }
            seen.push(op);
            if !used.iter().any(|(u, _)| *u == op.name) {
                let known: Vec<&str> = {
                    let mut k: Vec<&str> = used.iter().map(|(u, _)| u.as_str()).collect();
                    k.sort();
                    k.dedup();
                    k
                };
                self.push(
                    Diag::new(
                        E2,
                        "E0427",
                        format!("a autoridade `{}` cita a operação inexistente `{}`", d.name.name, op.name),
                        op.span,
                        "nenhum contrato `requires s.authorizes(…)` com esta operação",
                    )
                    .note(format!(
                        "operações concedíveis neste módulo: {}",
                        if known.is_empty() { "nenhuma".to_string() } else { known.join(", ") }
                    )),
                );
            }
        }
        if d.op_idents.is_empty() && d.ops.is_some() {
            self.push(Diag::new(
                E2,
                "E0427",
                format!("a autoridade `{}` não concede operação alguma", d.name.name),
                d.ops_span,
                "`ops = []`",
            ));
        }
        let witnesses: Vec<&WitnessDecl> =
            self.m.items.iter().filter_map(|i| if let Item::Witness(w) = i { Some(w) } else { None }).collect();
        for (role, r) in [("clock", &d.clock), ("outcomes", &d.outcomes)] {
            let Some(r) = r else { continue };
            match witnesses.iter().find(|w| w.name.name == r.name) {
                None => self.push(Diag::new(
                    E2,
                    "E0429",
                    format!("`{role} = {}`: a testemunha `{}` não está declarada", r.name, r.name),
                    r.span,
                    "declare `witness nome { key = \"ed25519:…\" }`",
                )),
                Some(w) if role == "clock" && w.tolerance.is_none() => self.push(
                    Diag::new(E2, "E0429", format!("a testemunha de tempo `{}` não declara `tolerance`", w.name.name), r.span, "usada como `clock`")
                        .note("o atraso máximo admitido entre o pedido e a resposta de tempo é parte da garantia: declare-o (segundos)"),
                ),
                Some(_) => {}
            }
        }
    }

    /// **O escopo das autoridades fecha com os contratos (E0427).** Se toda autoridade do
    /// módulo declara `ops`, uma operação que nenhuma delas pode conceder nunca seria
    /// autorizada: o contrato é inatingível. E um `ops` que cita uma operação que nenhum
    /// contrato usa é um erro de digitação que daria a falsa impressão de escopo.
    fn check_authority_scope(&mut self) {
        self.check_binds();
        let auths: Vec<&AuthorityDecl> =
            self.m.items.iter().filter_map(|i| if let Item::Authority(d) = i { Some(d) } else { None }).collect();
        let used = std::mem::take(&mut self.auth_ops);
        for d in &auths {
            if d.typed {
                self.check_typed_authority(d, &used);
                continue;
            }
            for op in d.ops.iter().flatten() {
                if !used.iter().any(|(u, _)| u == op) {
                    self.push(Diag::new(
                        E2,
                        "E0427",
                        format!("a autoridade `{}` concede `{op}`, que nenhum contrato usa", d.name.name),
                        d.ops_span,
                        "nenhum `requires s.authorizes(…)` com esta operação",
                    ));
                }
            }
        }
        if !auths.is_empty() && auths.iter().all(|d| d.ops.is_some()) {
            for (op, span) in &used {
                if !auths.iter().any(|d| d.ops.iter().flatten().any(|o| o == op)) {
                    self.push(
                        Diag::new(
                            E2,
                            "E0427",
                            format!("nenhuma autoridade do módulo pode conceder `{op}`"),
                            *span,
                            "este contrato nunca seria satisfeito",
                        )
                        .help("acrescente a operação ao `ops` de uma autoridade"),
                    );
                }
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
        self.check_requires(t);
    }

    /// `requires φ`: o contrato de autorização. É conferido como o corpo de uma política (puro:
    /// nada de ferramenta, inferência ou `commit`; só os parâmetros e o estado confiável) e
    /// precisa ser `Bool`. O runtime o avalia sobre os argumentos reais antes do efeito.
    fn check_requires(&mut self, t: &'m ToolDecl) {
        let Some(req) = &t.requires else { return };
        self.in_policy = true;
        self.scopes = vec![vec![]];
        self.pc = Label::Trusted;
        self.pc_data = Label::Trusted;
        self.pc_span = None;
        self.used.clear();
        self.ret = None;
        for prm in &t.params {
            let (pt, _) = self.resolve(&prm.ty);
            if !matches!(pt, Ty::Cap(_)) && !pt.is_linear() {
                self.bind(&prm.name.name, LTy::trusted(pt));
            }
        }
        self.in_requires = true;
        let body = self.expr(req);
        self.in_requires = false;
        if !assignable(&body.ty, &Ty::Bool) {
            self.push(Diag::new(
                E0,
                "E0430",
                "o contrato `requires` é uma condição",
                req.span,
                format!("`{}` exige `Bool`, encontrado `{}`", t.name.name, body.ty.show()),
            ));
        }
        self.in_policy = false;
        self.scopes.clear();
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
        self.pc_data = Label::Trusted;
        self.pc_span = None;
        self.used.clear();
        self.ret = None;
        let mut names: HashSet<&str> = HashSet::new();
        for prm in &p.params {
            if !names.insert(prm.name.name.as_str()) {
                self.push(Diag::new(
                    E0,
                    "E0002",
                    format!("parâmetro `{}` repetido", prm.name.name),
                    prm.name.span,
                    "já declarado nesta política",
                ));
            }
            let (t, _) = self.resolve(&prm.ty);
            // o refinamento declarado vale dentro da política: toda chamada o prova (E0450)
            let refn = self.refn_of(&prm.ty);
            self.bind(&prm.name.name, LTy::declared(t, Label::Trusted, refn, Vec::new()));
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
        self.pc_data = Label::Trusted;
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
                let conf = self.conf_of(&p.ty).unwrap_or(Conf::Public);
                self.bind(&p.name.name, LTy::declared(t, lab, refn, origin).with_conf(conf));
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
        let depth = self.scopes.len() - 1;
        self.let_anchor.retain(|(d, _), _| *d < depth);
        let mut diverged = false;
        for s in &b.stmts {
            match s {
                Stmt::Let { name, value, span } => {
                    let saved = self.let_init.replace(value.span);
                    let t = self.expr(value);
                    self.let_init = saved;
                    if matches!(t.ty, Ty::IdemKey) {
                        self.facts.linear_lets.insert(*span);
                    }
                    let anchor = self.snapshot_expr(value);
                    self.bind(&name.name, t);
                    if let Some(a) = anchor {
                        let i = self.scopes[depth].len() - 1;
                        self.let_anchor.insert((depth, i), a);
                    }
                    // v1.0-RC8: `keeping … as nome` liga também a lista do que foi deixado de fora
                    // (só a do `refine` que é o valor desta ligação, não a de um `let` aninhado)
                    if self.keep_bind.as_ref().is_some_and(|(s, ..)| *s == value.span) {
                        let (_, n, lt) = self.keep_bind.take().unwrap();
                        if n.name == name.name {
                            self.push(
                                Diag::new(
                                    E0,
                                    "E0438",
                                    format!("`as {}` repete o nome da ligação", n.name),
                                    n.span,
                                    "o plano e o que foi deixado de fora teriam o mesmo nome",
                                )
                                .help("dê ao que foi deixado de fora um nome próprio, como `as recusados`"),
                            );
                        } else {
                            self.bind(&n.name, lt);
                        }
                    }
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
        self.let_anchor.retain(|(d, _), _| *d < depth);
        self.scopes.pop();
        out
    }

    fn expr(&mut self, e: &Expr) -> LTy {
        match &e.kind {
            ExprKind::Int(n) => {
                let mut t = LTy::trusted(Ty::Int);
                t.refn = Some(Refn::Int { lo: Some(*n), hi: Some(*n) });
                t
            }
            ExprKind::Bool(_) => LTy::trusted(Ty::Bool),
            // (v1.0-RC8: o literal carrega o seu comprimento, para a composição de texto provar limites)
            ExprKind::Str(s) => LTy {
                conf: Conf::Public,
                tok: Some(est_tokens(s)),
                refn: Some(Refn::Text { max: s.chars().count() as u32 }),
                ..LTy::trusted(Ty::Text)
            },
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
                let refn = match (op, t.refn) {
                    (UnOp::Neg, Some(Refn::Int { lo: Some(a), hi: Some(b) })) => {
                        match (b.checked_neg(), a.checked_neg()) {
                            (Some(lo), Some(hi)) => Some(Refn::Int { lo: Some(lo), hi: Some(hi) }),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if *op == UnOp::Neg && refn.is_none() {
                    self.unbounded_arith(e.span, "`-`");
                }
                LTy { ty: want, tok: None, refn, ..t }
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
            ExprKind::Refine { draft, policy, retry, els, keep } => {
                self.refine(e, draft, policy, *retry, els, keep.as_ref())
            }
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
                    self.impure(e.span, "laço `for` (use `all x in xs => …`, `any` ou `sum`)");
                }
                let Some((it, elem)) = self.iteration(e.span, iter, "for") else {
                    self.scopes.push(vec![(var.name.clone(), LTy::never())]);
                    self.block(body);
                    self.scopes.pop();
                    return LTy::trusted(Ty::Unit);
                };
                let saved = (self.pc, self.pc_span, self.pc_conf, self.pc_conf_span);
                // O número de iterações é decidido pela lista: fluxo implícito, como num `match`,
                // para a integridade e para a confidencialidade.
                let saved_data = self.pc_data;
                self.pc_data = self.pc_data.meet(it.lab);
                if it.lab < self.pc {
                    self.pc = it.lab;
                    self.pc_span = Some(iter.span);
                }
                if it.conf > self.pc_conf {
                    self.pc_conf = it.conf;
                    self.pc_conf_span = Some(iter.span);
                }
                self.scopes.push(vec![(var.name.clone(), elem)]);
                self.block(body);
                self.scopes.pop();
                (self.pc, self.pc_span, self.pc_conf, self.pc_conf_span) = saved;
                self.pc_data = saved_data;
                LTy::trusted(Ty::Unit)
            }
            ExprKind::Quant { op, var, iter, body } => {
                let sum = *op == QuantOp::Sum;
                let want = if sum { Ty::Int } else { Ty::Bool };
                let Some((it, elem)) = self.iteration(e.span, iter, op.keyword()) else {
                    self.scopes.push(vec![(var.name.clone(), LTy::never())]);
                    self.expr(body);
                    self.scopes.pop();
                    return LTy::trusted(want);
                };
                let saved = (self.pc, self.pc_span, self.pc_conf, self.pc_conf_span);
                if it.lab < self.pc {
                    self.pc = it.lab;
                    self.pc_span = Some(iter.span);
                }
                if it.conf > self.pc_conf {
                    self.pc_conf = it.conf;
                    self.pc_conf_span = Some(iter.span);
                }
                self.scopes.push(vec![(var.name.clone(), elem)]);
                let b = self.expr(body);
                self.scopes.pop();
                (self.pc, self.pc_span, self.pc_conf, self.pc_conf_span) = saved;
                if !assignable(&b.ty, &want) {
                    self.mismatch(body.span, &want, &b.ty);
                }
                let mut out = LTy::trusted(want);
                // O resultado depende da lista e do corpo: os dois rótulos, nas duas dimensões
                // (Ciclo 1: antes a confidencialidade não era propagada pelo quantificador).
                out.meet_from(&it);
                out.meet_from(&b);
                out.join_conf_from(&it);
                out.join_conf_from(&b);
                if sum {
                    // Soma de k termos em [a, b], k em [n_lo, n_hi]: o intervalo é provado pelo
                    // tipo da lista e pelo refinamento do corpo; sem um deles, `Int` sem limite.
                    let (n_lo, n_hi) = match &it.ty {
                        Ty::List(_, _, lo, hi) => (*lo as i64, *hi as i64),
                        _ => (0, 0),
                    };
                    if let Some(Refn::Int { lo, hi }) = b.refn {
                        let lo = lo.and_then(|a| if a >= 0 { n_lo.checked_mul(a) } else { n_hi.checked_mul(a) });
                        let hi = hi.and_then(|b| if b >= 0 { n_hi.checked_mul(b) } else { n_lo.checked_mul(b) });
                        out.refn = Some(Refn::Int { lo, hi });
                    }
                    if !matches!(out.refn, Some(Refn::Int { lo: Some(_), hi: Some(_) })) {
                        self.unbounded_arith(e.span, "`sum`");
                    }
                }
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
        LTy {
            ty: Ty::List(Box::new(elem), refn.flatten(), n, n),
            lab: out.lab,
            conf: out.conf,
            tok,
            origin: out.origin,
            refn: None,
        }
    }

    /// Operando de `for`/`all`/`any`: precisa ser uma lista delimitada. Registra os limites
    /// de iterações (para o E6) e devolve o tipo do elemento, com a integridade da lista.
    fn iteration(&mut self, span: Span, iter: &Expr, what: &str) -> Option<(LTy, LTy)> {
        let it = self.expr(iter);
        match &it.ty {
            Ty::List(elem, er, lo, hi) => {
                self.facts.loop_bounds.insert(span, (*lo, *hi));
                self.facts.loops += 1;
                // O elemento herda integridade e confidencialidade da lista: iterar um segredo
                // não o torna público (Ciclo 1: antes só a integridade era herdada).
                let mut el = LTy::declared((**elem).clone(), it.lab, *er, it.origin.clone()).with_conf(it.conf);
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
        let recv_conf = r.conf;
        // **v1.0-RC8: projeção.** `xs.campo` numa lista de registros é a lista do campo de cada
        // um, com os mesmos limites e o mesmo rótulo.
        if let Ty::List(elem, _, lo, hi) = &r.ty {
            if let Ty::Adt(n) = &**elem {
                let (lo, hi, n) = (*lo, *hi, n.clone());
                let decl: &'m TypeDecl = self.types[&n];
                let mut found: Option<(Ty, Option<Refn>)> = None;
                let mut ok = true;
                for v in &decl.variants {
                    match v.fields.iter().find(|fd| fd.name.name == f.name) {
                        Some(fd) => {
                            let (t, _) = self.resolve(&fd.ty);
                            let rf = self.refn_of(&fd.ty);
                            match &found {
                                None => found = Some((t, rf)),
                                Some((prev, prf)) if crate::ty::same(prev, &t) && *prf == rf => {}
                                _ => ok = false,
                            }
                        }
                        None => ok = false,
                    }
                }
                if let (true, Some((t, rf))) = (ok, found) {
                    let mut out = LTy::trusted(Ty::List(Box::new(t), rf, lo, hi));
                    out.lab = r.lab;
                    out.conf = recv_conf;
                    out.origin = r.origin;
                    return out;
                }
                self.push(
                    Diag::new(
                        E0,
                        "E0005",
                        format!("campo `{}` não existe, com o mesmo tipo, em todas as variantes de `{n}`", f.name),
                        f.span,
                        "projeção de lista",
                    )
                    .help("`lista.campo` exige o campo em toda variante do elemento"),
                );
                return LTy::never();
            }
        }
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
            (true, Some(t)) => LTy::declared(t, r.lab, refn, r.origin).with_conf(recv_conf),
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
                "inventory" | "member" | "within" | "mentions" | "links_within" | "binds" | "authorizes" => {
                    // `s.links_within("k", texto)` (ciclo 1, D2): toda ocorrência de link na forma G
                    // (`http://`, `https://`, `www.`) dentro de `texto` pertence ao inventário
                    // confiável `k`. Propriedade sintática; não é detecção de phishing.
                    // `s.within("k", x)`: `x` (decimal) ≤ o limite confiável `k`; `s.mentions("k", x)`:
                    // `x` aparece literalmente num texto confiável `k` (Rev12, contratos `requires`);
                    // `s.binds("r", x1, …, xn)`: a tupla é uma linha da relação confiável `r` (Rev13:
                    // a intenção autorizada liga operação, destinatário e valor)
                    // `s.authorizes("op", x1, …, xn)`: há uma concessão livre desta execução para a
                    // tupla inteira, e o efeito a consome (v1.0-RC1). É linear: só existe no
                    // contrato de uma ferramenta, onde há um efeito para consumi-la. Uma política
                    // é pura e pode ser avaliada várias vezes; não decide com concessões.
                    let member = method.name != "inventory";
                    if method.name == "authorizes" {
                        if let Some(Expr { kind: ExprKind::Str(op), .. }) = args.first() {
                            if !self.auth_ops.iter().any(|(_, sp)| *sp == method.span) {
                                self.auth_ops.push((op.clone(), method.span));
                            }
                        }
                    }
                    if method.name == "authorizes" && !self.in_requires {
                        self.push(
                            Diag::new(
                                E2,
                                "E0425",
                                "autorização linear fora de `requires`",
                                method.span,
                                "`s.authorizes` só existe no contrato `requires` de uma ferramenta",
                            )
                            .help("uma concessão é consumida pelo efeito que ela autoriza: declare-a no `requires` da ferramenta"),
                        );
                    }
                    if matches!(method.name.as_str(), "binds" | "authorizes") {
                        if args.len() < 2 {
                            check_args(self, 2);
                        }
                    } else {
                        check_args(self, if member { 2 } else { 1 });
                    }
                    if member && !self.in_policy {
                        self.push(
                            Diag::new(
                                E2,
                                "E0423",
                                "sonda contextual fora de política",
                                method.span,
                                format!("`s.{}` só decide dentro de `policy` ou de `requires`", method.name),
                            )
                            .help("leve a decisão para a política aplicada pelo `refine`"),
                        );
                    }
                    // o nome do inventário é parte do programa: literal, nunca dado
                    if !matches!(args.first().map(|a| &a.kind), Some(ExprKind::Str(_))) {
                        self.push(
                            Diag::new(
                                E3,
                                "E0424",
                                "nome de inventário não literal",
                                args.first().map_or(method.span, |a| a.span),
                                "o nome precisa ser um literal de texto",
                            )
                            .help("escreva `s.inventory(\"nome\")`: o nome do inventário não pode vir de dado"),
                        );
                    }
                    let text_only = method.name == "links_within";
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.expr(arg);
                        let ok = assignable(&t.ty, &Ty::Text) || (!text_only && assignable(&t.ty, &Ty::Int));
                        if i >= 1 && !ok {
                            self.mismatch(arg.span, &Ty::Text, &t.ty);
                        }
                    }
                    return if member {
                        LTy::trusted(Ty::Bool)
                    } else {
                        LTy::declared(Ty::Text, Label::Trusted, Some(Refn::Text { max: 2048 }), Vec::new())
                    };
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
                return LTy { ty: Ty::Text, lab: t.lab, conf: t.conf, tok: Some(n), origin: t.origin, refn: None };
            }
            // **v1.0-RC8: composição de texto.** `text(n)`: o inteiro em decimal, com o comprimento
            // provado pelos limites de `n`. `join(xs, sep)`: os textos da lista, separados. O
            // rótulo é o mais fraco dos argumentos (como em `++`): compor não endossa nada.
            "text" => {
                if args.len() != 1 {
                    self.push(Diag::new(E0, "E0007", "`text(n)` recebe 1 argumento", e.span, "aridade"));
                    return LTy::never();
                }
                let t = self.expr(&args[0]);
                if !assignable(&t.ty, &Ty::Int) {
                    self.mismatch(args[0].span, &Ty::Int, &t.ty);
                    return LTy::never();
                }
                let digits = |n: i64| n.to_string().chars().count() as u32;
                let refn = match t.refn {
                    Some(Refn::Int { lo: Some(lo), hi: Some(hi) }) => {
                        Some(Refn::Text { max: digits(lo).max(digits(hi)) })
                    }
                    _ => Some(Refn::Text { max: 20 }),
                };
                // A representação decimal de um segredo é o segredo (Ciclo 1: antes a
                // confidencialidade se perdia aqui).
                let mut out = LTy::declared(Ty::Text, t.lab, refn, t.origin.clone()).with_conf(t.conf);
                out.tok = refn.map(|r| match r {
                    Refn::Text { max } => max as u64,
                    _ => 20,
                });
                return out;
            }
            "join" => {
                if args.len() != 2 {
                    self.push(Diag::new(
                        E0,
                        "E0007",
                        "`join(lista, separador)` recebe 2 argumentos",
                        e.span,
                        "aridade",
                    ));
                    return LTy::never();
                }
                let xs = self.expr(&args[0]);
                let sep = self.expr(&args[1]);
                if !assignable(&sep.ty, &Ty::Text) {
                    self.mismatch(args[1].span, &Ty::Text, &sep.ty);
                }
                let Ty::List(elem, er, _lo, hi) = &xs.ty else {
                    if !xs.ty.is_never() {
                        self.push(Diag::new(
                            E0,
                            "E0008",
                            "`join` exige uma lista de textos",
                            args[0].span,
                            format!("encontrado `{}`", xs.ty.show()),
                        ));
                    }
                    return LTy::never();
                };
                if !assignable(elem, &Ty::Text) {
                    self.push(Diag::new(
                        E0,
                        "E0008",
                        "`join` exige uma lista de textos",
                        args[0].span,
                        format!("encontrado `{}`", xs.ty.show()),
                    ));
                    return LTy::never();
                }
                let hi = *hi;
                let refn = match (er, sep.refn) {
                    (Some(Refn::Text { max: a }), Some(Refn::Text { max: s })) => Some(Refn::Text {
                        max: hi.saturating_mul(*a).saturating_add(hi.saturating_sub(1).saturating_mul(s)),
                    }),
                    _ => None,
                };
                let mut out = LTy::declared(Ty::Text, xs.lab.meet(sep.lab), refn, xs.origin.clone())
                    .with_conf(xs.conf.join(sep.conf));
                if refn.is_none() {
                    out.tok = None;
                }
                return out;
            }
            // **Ciclo 1 (P3): propriedades de texto.** Funções puras e totais sobre `Text`, para a
            // política decidir sobre conteúdo sem passar a regra ao host. O resultado herda os
            // rótulos dos argumentos: inspecionar um texto não o endossa nem o torna público.
            //   contains(t, p)    Bool  `p` ocorre em `t` (pontos de código, exato; "" ocorre sempre)
            //   lower(t)          Text  só A–Z viram a–z; todo o resto fica como está
            //   ascii_letters(t)  Int   quantos caracteres em A–Z ou a–z
            //   ascii_digits(t)   Int   quantos caracteres em 0–9
            "contains" => {
                if args.len() != 2 {
                    self.push(Diag::new(
                        E0,
                        "E0007",
                        "`contains(texto, trecho)` recebe 2 argumentos",
                        e.span,
                        "aridade",
                    ));
                    return LTy::never();
                }
                let t = self.expr(&args[0]);
                let p = self.expr(&args[1]);
                for (a, x) in [(&args[0], &t), (&args[1], &p)] {
                    if !assignable(&x.ty, &Ty::Text) {
                        self.mismatch(a.span, &Ty::Text, &x.ty);
                    }
                }
                let mut out = LTy::trusted(Ty::Bool);
                out.meet_from(&t);
                out.meet_from(&p);
                out.join_conf_from(&t);
                out.join_conf_from(&p);
                return out;
            }
            "lower" | "ascii_letters" | "ascii_digits" => {
                let name = callee.name.as_str();
                if args.len() != 1 {
                    self.push(Diag::new(E0, "E0007", format!("`{name}(texto)` recebe 1 argumento"), e.span, "aridade"));
                    return LTy::never();
                }
                let t = self.expr(&args[0]);
                if !assignable(&t.ty, &Ty::Text) {
                    self.mismatch(args[0].span, &Ty::Text, &t.ty);
                    return LTy::never();
                }
                if name == "lower" {
                    // mesmo número de caracteres e de tokens: os limites do argumento valem
                    return LTy { ty: Ty::Text, lab: t.lab, conf: t.conf, tok: t.tok, origin: t.origin, refn: t.refn };
                }
                return LTy {
                    ty: Ty::Int,
                    lab: t.lab,
                    conf: t.conf,
                    tok: None,
                    origin: t.origin.clone(),
                    refn: Some(Refn::Int {
                        lo: Some(0),
                        hi: match t.refn {
                            Some(Refn::Text { max }) => Some(max as i64),
                            _ => None,
                        },
                    }),
                };
            }
            "len" => {
                if args.len() != 1 {
                    self.push(Diag::new(E0, "E0007", "`len(lista ou texto)` recebe 1 argumento", e.span, "aridade"));
                    return LTy::never();
                }
                let t = self.expr(&args[0]);
                return match &t.ty {
                    // O tamanho herda os limites do tipo: `len(xs)` é um `Int[a..b]` provado.
                    Ty::List(_, _, lo, hi) => LTy {
                        conf: t.conf,
                        ty: Ty::Int,
                        lab: t.lab,
                        tok: None,
                        origin: t.origin.clone(),
                        refn: Some(Refn::Int { lo: Some(*lo as i64), hi: Some(*hi as i64) }),
                    },
                    // Em texto, `len` conta caracteres; o teto do tipo vira o teto do inteiro.
                    Ty::Text => LTy {
                        conf: t.conf,
                        ty: Ty::Int,
                        lab: t.lab,
                        tok: None,
                        origin: t.origin.clone(),
                        refn: Some(Refn::Int {
                            lo: Some(0),
                            hi: match t.refn {
                                Some(Refn::Text { max }) => Some(max as i64),
                                _ => None,
                            },
                        }),
                    },
                    Ty::Never => LTy::never(),
                    other => {
                        self.push(Diag::new(
                            E0,
                            "E0008",
                            "`len` exige uma lista ou um texto",
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
                let LTy { ty, lab, conf, origin, refn, .. } = self.expr(&args[0]);
                return match ty {
                    Ty::Draft(inner, ispan) => {
                        self.facts.draft_src.insert(e.span, ispan);
                        self.use_effect("Infer", e.span);
                        LTy::declared(*inner, lab, refn, origin).with_conf(conf).with_step("sample(…)")
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
                // Revisão, achado 4: o veredito sobre um segredo é segredo, como na forma inline.
                out.join_conf_from(&at);
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
                taint.join_conf_from(&at);
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
            // v1.0-RC10: a confidencialidade do argumento não pode passar do que o parâmetro
            // tolera (um sink público declara `^ Public`). `pc_conf` conta como um canal: um
            // efeito num ramo decidido por dado secreto também expõe.
            let declared_conf = self.conf_of(&p.ty);
            let allow = declared_conf.unwrap_or(Conf::Secret);
            if declared_conf.is_some() {
                self.facts.confidentiality_checks += 1;
            }
            let eff_conf = at.conf.join(self.pc_conf);
            if eff_conf > allow {
                let implicit = at.conf <= allow;
                let mut d = Diag::new(
                    E3,
                    "E0440",
                    "fluxo de confidencialidade violado",
                    a.span,
                    format!("confidencialidade {}", eff_conf.name()),
                );
                if implicit {
                    d = d.note(format!(
                        "o valor é {}, mas a chamada ocorre num ramo decidido por dado {}",
                        at.conf.name(),
                        self.pc_conf.name()
                    ));
                    if let Some(ps) = self.pc_conf_span {
                        d = d.secondary(ps, format!("ramo decidido por dado {}", self.pc_conf.name()));
                    }
                } else if !at.origin.is_empty() {
                    d = d.note(format!("origem: {}", at.origin.join(" → ")));
                }
                d = d.note(format!("exigido: {}({}: {} ^ {})", callee.name, p.name.name, pt.show(), allow.name()));
                d = d.help(
                    "um canal que aceita só `^ Public` não recebe dado `Secret` ou `Internal`: nenhum dado confidencial sai por um sink público, qualquer que seja a decisão do modelo",
                );
                self.push(d);
            }
        }
        // **D15 (Ciclo 1).** Um efeito irrevogável não pode ser decidido por dado não confiável,
        // tenha ou não a ferramenta um parâmetro `^ Endorsed`: antes, o `pc` só era conferido
        // junto com um parâmetro rotulado, e `if web == "x" { commit wipe(c) }` compilava.
        if tool.class == EffClass::Irrevocable
            && self.pc_data < Label::Endorsed
            && !tool.params.iter().any(|p| self.resolve(&p.ty).1.is_some())
        {
            let mut d = Diag::new(
                E3,
                "E0412",
                "fluxo de integridade violado",
                callee.span,
                format!("efeito irrevogável decidido por dado {}", self.pc_data.name()),
            )
            .note(format!(
                "`{}` é irrevocable e a chamada ocorre num ramo decidido por dado {}",
                callee.name,
                self.pc.name()
            ))
            .help("decida o ramo com dado confiável ou endossado (`refine … by <policy>`)");
            if let Some(ps) = self.pc_span {
                d = d.secondary(ps, format!("ramo decidido por dado {}", self.pc.name()));
            }
            self.push(d);
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
        let mut out = LTy::declared(rt, lab, refn, origin).with_conf(self.conf_of(&tool.ret).unwrap_or(Conf::Public));
        if self.taint_calls {
            // Numa condição de controle, o que a ferramenta observa é escolhido pelos argumentos.
            if taint.lab < out.lab {
                out.lab = taint.lab;
                out.origin = taint.origin;
                out.origin.push(format!("{}(…)", callee.name));
            }
            out.conf = out.conf.join(taint.conf);
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
                    out.join_conf_from(&at);
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
        out.join_conf_from(&lt);
        out.join_conf_from(&rt);
        if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
            out.refn = int_arith(op, lt.refn, rt.refn);
            if out.refn.is_none() {
                self.unbounded_arith(e.span, &format!("`{}`", op.symbol()));
            }
        }
        if op == BinOp::Concat {
            out.tok = match (lt.tok, rt.tok) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            };
            // v1.0-RC8: Text[..a] ++ Text[..b] é Text[..a+b]
            out.refn = match (lt.refn, rt.refn) {
                (Some(Refn::Text { max: a }), Some(Refn::Text { max: b })) => {
                    Some(Refn::Text { max: a.saturating_add(b) })
                }
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
        let saved = (self.pc, self.pc_span, self.pc_conf, self.pc_conf_span);
        let saved_data = self.pc_data;
        self.pc_data = self.pc_data.meet(c.lab);
        if c.lab < self.pc {
            self.pc = c.lab;
            self.pc_span = Some(cond.span);
        }
        if c.conf > self.pc_conf {
            self.pc_conf = c.conf;
            self.pc_conf_span = Some(cond.span);
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
                        // D16: o valor de um ramo secreto continua secreto fora do `if`
                        o.join_conf_from(&t);
                        o.join_conf_from(&f);
                        o.refn = int_hull(t.refn, f.refn);
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
        (self.pc, self.pc_span, self.pc_conf, self.pc_conf_span) = saved;
        self.pc_data = saved_data;
        out.meet_from(&c);
        out.join_conf_from(&c);
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
        let saved = (self.pc, self.pc_span, self.pc_conf, self.pc_conf_span);
        let saved_data = self.pc_data;
        if arms.len() > 1 {
            self.pc_data = self.pc_data.meet(s.lab);
        }
        if arms.len() > 1 && s.lab < self.pc {
            self.pc = s.lab;
            self.pc_span = Some(scrut.span);
        }
        if arms.len() > 1 && s.conf > self.pc_conf {
            self.pc_conf = s.conf;
            self.pc_conf_span = Some(scrut.span);
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
                        self.bind(&b.name, LTy::declared(ft, s.lab, refn, s.origin.clone()).with_conf(s.conf));
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
                        // D16: idem para os braços de um `match`
                        o.join_conf_from(&prev);
                        o.join_conf_from(&bt);
                        o.refn = int_hull(prev.refn, bt.refn);
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
        (self.pc, self.pc_span, self.pc_conf, self.pc_conf_span) = saved;
        self.pc_data = saved_data;
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
        out.join_conf_from(&s);
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
        let mut names: BTreeSet<String> = BTreeSet::new();
        for (_, v) in fields {
            if let Some(k) = v.inventory_anchor() {
                names.insert(k.to_string());
            } else if let Some(a) = self.snapshot_expr(v) {
                names.insert(a.inventory_anchor().expect("snapshot_expr só devolve âncoras").to_string());
                self.facts.anchor_vars.push((v.span, a));
            }
        }
        if !names.is_empty() {
            self.anchored.insert(e.span, names);
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
        let mut acc = LTy { ty: Ty::Unit, lab: self.pc, conf: self.pc_conf, tok: None, origin: Vec::new(), refn: None };
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
        LTy { ty: Ty::Draft(Box::new(oty), e.span), lab: acc.lab, conf: acc.conf, tok: None, origin: acc.origin, refn }
            .with_step(format!("infer {}", model.name))
    }

    fn refine(&mut self, e: &Expr, draft: &Expr, policy: &Ident, _retry: u32, els: &Block, keep: Option<&Keep>) -> LTy {
        if self.in_policy {
            self.impure(e.span, "`refine`");
        }
        let d = self.expr(draft);
        if let Some(k) = keep {
            let dropped = self.keeping(&d.ty, k);
            if let Some(b) = &k.bind {
                if self.let_init != Some(e.span) {
                    self.push(
                        Diag::new(
                            E0,
                            "E0438",
                            format!("`keeping … as {}` só liga o nome num `let`", b.name),
                            b.span,
                            "fora de `let x = refine …`",
                        )
                        .help("escreva `let plano = refine … keeping campo by Q as recusados else { … }`"),
                    );
                } else {
                    self.keep_bind = Some((e.span, b.clone(), dropped.unwrap_or_else(LTy::never)));
                }
            }
        }
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
                    self.anchor_check(p, policy, ispan);
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

    /// O inventário que a política lê precisa estar no prompt do `infer` (E0421), e a política
    /// decide pela fotografia, não por literais (E0422). Vale para a política do `refine` e para
    /// a de `keeping`.
    fn anchor_check(&mut self, p: &'m PolicyDecl, policy: &Ident, ispan: Span) {
        let given = self.anchored.get(&ispan).cloned().unwrap_or_default();
        let anchored = !given.is_empty();
        // Ciclo 1, D1: a âncora segue as chamadas entre políticas. `SafePlan` que delega a
        // `SafeWrite` decide pelos inventários que `SafeWrite` lê, e pelos literais que ela compara.
        let bodies = self.policy_closure(p);
        let read: BTreeSet<String> = bodies.iter().flat_map(|b| policy_inventories(b)).collect();
        let reads = bodies.iter().any(|b| reads_snapshot(b));
        let literal = bodies.iter().any(|b| compares_text_literal(b));
        let missing: Vec<String> = read.difference(&given).cloned().collect();
        if reads && (!anchored || !missing.is_empty()) {
            let want = missing
                .iter()
                .map(|k| if k == "managed" { "s.managed()".to_string() } else { format!("s.inventory(\"{k}\")") })
                .collect::<Vec<_>>()
                .join(", ");
            let want = if want.is_empty() { "s.managed()".to_string() } else { want };
            self.push(
                    Diag::new(
                        E3,
                        "E0421",
                        "a política decide pela fotografia confiável, mas o prompt não recebe o inventário",
                        ispan,
                        format!("`{}` lê `s.…`; o `infer` não tem um campo `= {want}`", policy.name),
                    )
                    .note(format!("decidido por `refine … by {}`", policy.name))
                    .help(format!("passe o inventário ao modelo como `campo = {want}`: nunca copiado de literal, log ou resposta de modelo")),
                );
        }
        if anchored && literal {
            self.push(
                Diag::new(
                    E3,
                    "E0422",
                    "descompasso entre o inventário confiável e a política",
                    policy.span,
                    format!("o prompt recebe `s.managed()`, mas `{}` compara com literais de texto", policy.name),
                )
                .help("decida pela mesma fotografia: `s.is_managed(x)` no lugar de `x == \"…\"`"),
            );
        }
    }

    /// O corpo da política e os de todas as políticas que ela chama, transitivamente (sem
    /// repetição: uma recursão entre políticas não trava a análise).
    fn policy_closure(&self, p: &'m PolicyDecl) -> Vec<&'m Expr> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut todo = vec![p];
        let mut out = Vec::new();
        while let Some(q) = todo.pop() {
            if !seen.insert(q.name.name.as_str()) {
                continue;
            }
            out.push(&q.body);
            let called = std::cell::RefCell::new(Vec::new());
            expr_any(&q.body, &|x| {
                if let ExprKind::Call { callee, .. } = &x.kind {
                    called.borrow_mut().push(callee.name.clone());
                }
                false
            });
            for name in called.into_inner() {
                if let Some(r) = self.policies.get(&name).copied() {
                    todo.push(r);
                }
            }
        }
        out
    }

    /// **`keeping campo by Política` (v1.0-RC7).** O campo existe no tipo do rascunho e é uma
    /// lista; a política recebe um elemento dela (E0437, E0450); e o inventário que ela lê está
    /// no prompt (E0421, E0422).
    fn keeping(&mut self, d: &Ty, k: &Keep) -> Option<LTy> {
        let (adt, infer_span) = match d {
            Ty::Draft(inner, ispan) => match &**inner {
                Ty::Adt(n) => (Some(n.clone()), Some(*ispan)),
                _ => (None, Some(*ispan)),
            },
            _ => return None,
        };
        let field = k.field.name.as_str();
        let mut elem: Option<TypeExpr> = None;
        let mut hi = 0u32;
        let mut not_list = false;
        if let Some(decl) = adt.as_ref().and_then(|n| self.types.get(n.as_str()).copied()) {
            for v in &decl.variants {
                for f in v.fields.iter().filter(|f| f.name.name == field) {
                    match f.ty.unlabeled() {
                        TypeExpr::List { elem: e, hi: h, .. } => {
                            // a mesma política julga o campo em todas as variantes: o tipo do
                            // elemento tem de ser o mesmo em todas (revisão, achado 7)
                            if elem.as_ref().is_some_and(|prev| prev.sig() != e.sig()) {
                                not_list = true;
                            }
                            elem = Some((**e).clone());
                            hi = hi.max(*h);
                        }
                        _ => not_list = true,
                    }
                }
            }
        }
        let Some(elem) = elem.filter(|_| !not_list) else {
            self.push(
                Diag::new(
                    E0,
                    "E0437",
                    format!("`keeping {field}`: o rascunho não tem um campo de lista `{field}`"),
                    k.field.span,
                    format!("o tipo é `{}`", adt.unwrap_or_else(|| "?".into())),
                )
                .help("`keeping` deixa de fora elementos de uma lista do plano; nomeie um campo `List[…]` do tipo do `infer`"),
            );
            return None;
        };
        let Some(p) = self.policies.get(&k.policy.name).copied() else {
            self.push(Diag::new(
                E0,
                "E0024",
                format!("política desconhecida `{}`", k.policy.name),
                k.policy.span,
                "não declarada",
            ));
            return None;
        };
        if let Some(ispan) = infer_span {
            self.anchor_check(p, &k.policy, ispan);
        }
        if p.params.len() != 1 && p.params.len() != 2 {
            self.push(
                Diag::new(
                    E0,
                    "E0437",
                    "a política de `keeping` recebe o elemento, ou o elemento e a lista dos já aceitos",
                    k.policy.span,
                    "aridade",
                )
                .help("`policy Q(x: T) -> Bool` ou `policy Q(x: T, aceitos: List[T, 0..n]) -> Bool`"),
            );
            return None;
        }
        // **Ciclo 1 (P3, C3): avaliação ordenada.** Com um segundo parâmetro, `Q` decide cada
        // elemento, na ordem proposta, vendo a lista dos que ela mesma já aceitou neste plano.
        // O estado é essa lista: limitada pelo tipo, sem efeito, refeita a cada `refine`.
        if let Some(acc) = p.params.get(1) {
            let (at, _) = self.resolve(&acc.ty);
            let (et0, _) = self.resolve(&elem);
            // o tipo do elemento da lista de aceitos é o do próprio elemento julgado (mesmos
            // limites: o que foi aceito cabe nela) e não tem rótulo próprio
            let same_elem = match acc.ty.unlabeled() {
                TypeExpr::List { elem: ae, .. } => {
                    ae.unlabeled().sig() == elem.unlabeled().sig()
                        && ae.sig() == ae.unlabeled().sig()
                        && self.conf_of(&acc.ty).is_none()
                }
                _ => false,
            };
            let ok = same_elem
                && match &at {
                    Ty::List(inner, _, lo, h) => *lo == 0 && *h >= hi && assignable(&et0, inner),
                    _ => false,
                };
            if !ok {
                self.push(
                    Diag::new(
                        E0,
                        "E0437",
                        "o segundo parâmetro da política de `keeping` é a lista dos elementos já aceitos",
                        acc.name.span,
                        format!("encontrado `{}`", at.show()),
                    )
                    .help(format!(
                        "declare `{}: List[{}, 0..{hi}]`: começa vazia e pode chegar ao tamanho máximo de `{field}`",
                        acc.name.name,
                        et0.show()
                    )),
                );
                return None;
            }
        }
        let (et, _) = self.resolve(&elem);
        let (pt, _) = self.resolve(&p.params[0].ty);
        if !assignable(&et, &pt) {
            self.push(Diag::new(
                E0,
                "E0437",
                "política de `keeping` incompatível com o elemento",
                k.policy.span,
                format!("`{}` decide sobre `{}`, a lista `{field}` tem `{}`", k.policy.name, pt.show(), et.show()),
            ));
        } else if !self.ty_within(&et, &p.params[0].ty) {
            self.facts.refinement_checks += 1;
            self.push(Diag::new(
                E5,
                "E0450",
                "refinamento não provado",
                k.policy.span,
                format!(
                    "`{}` decide sobre `{}`, a lista `{field}` tem `{}`",
                    k.policy.name,
                    p.params[0].ty.unlabeled().sig(),
                    et.show()
                ),
            ));
        }
        // o que foi deixado de fora: dados do modelo que a política recusou
        let erefn = self.refn_of(&elem);
        let mut out = LTy::trusted(Ty::List(Box::new(et), erefn, 0, hi));
        out.lab = Label::Untrusted;
        Some(out)
    }
}
