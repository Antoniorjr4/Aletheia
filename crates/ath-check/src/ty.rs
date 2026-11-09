use ath_syntax::ast::Label;
use ath_syntax::Span;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub enum Ty {
    Int,
    Bool,
    Text,
    Unit,
    Adt(String),
    Cap(BTreeSet<String>),
    /// Chave de idempotência: linear (multiplicidade 1).
    IdemKey,
    /// Rascunho produzido por `infer`; o `Span` identifica o nó de inferência de origem.
    Draft(Box<Ty>, Span),
    /// Divergência (`return`) ou erro já relatado; unifica com qualquer tipo.
    Never,
}

impl Ty {
    pub fn show(&self) -> String {
        match self {
            Ty::Int => "Int".into(),
            Ty::Bool => "Bool".into(),
            Ty::Text => "Text".into(),
            Ty::Unit => "Unit".into(),
            Ty::Adt(n) => n.clone(),
            Ty::Cap(s) => format!("Cap[{}]", s.iter().cloned().collect::<Vec<_>>().join(" + ")),
            Ty::IdemKey => "IdemKey".into(),
            Ty::Draft(t, _) => format!("Draft[{}]", t.show()),
            Ty::Never => "!".into(),
        }
    }

    pub fn is_never(&self) -> bool {
        matches!(self, Ty::Never)
    }
}

/// Igualdade estrutural (ignora o span de origem dos rascunhos).
pub fn same(a: &Ty, b: &Ty) -> bool {
    match (a, b) {
        (Ty::Int, Ty::Int) | (Ty::Bool, Ty::Bool) | (Ty::Text, Ty::Text) | (Ty::Unit, Ty::Unit) => true,
        (Ty::IdemKey, Ty::IdemKey) | (Ty::Never, Ty::Never) => true,
        (Ty::Adt(x), Ty::Adt(y)) => x == y,
        (Ty::Cap(x), Ty::Cap(y)) => x == y,
        (Ty::Draft(x, _), Ty::Draft(y, _)) => same(x, y),
        _ => false,
    }
}

/// `arg` pode ser passado onde `param` é esperado? Capacidades admitem atenuação:
/// uma `Cap[A + B]` serve onde `Cap[A]` é exigida.
pub fn assignable(arg: &Ty, param: &Ty) -> bool {
    match (arg, param) {
        (Ty::Never, _) | (_, Ty::Never) => true,
        (Ty::Cap(have), Ty::Cap(need)) => need.is_subset(have),
        _ => same(arg, param),
    }
}

pub fn unify(a: &Ty, b: &Ty) -> Option<Ty> {
    match (a, b) {
        (Ty::Never, t) | (t, Ty::Never) => Some(t.clone()),
        _ if same(a, b) => Some(a.clone()),
        _ => None,
    }
}

/// Refinamento decidível do Marco Zero: intervalo inteiro ou teto de caracteres.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refn {
    Int { lo: Option<i64>, hi: Option<i64> },
    Text { max: u32 },
}

impl Refn {
    /// `self ⊆ outer`: todo valor que satisfaz `self` satisfaz `outer`.
    pub fn within(self, outer: Refn) -> bool {
        match (self, outer) {
            (Refn::Int { lo: a, hi: b }, Refn::Int { lo: c, hi: d }) => {
                c.is_none_or(|c| a.is_some_and(|a| a >= c)) && d.is_none_or(|d| b.is_some_and(|b| b <= d))
            }
            (Refn::Text { max: a }, Refn::Text { max: b }) => a <= b,
            _ => false,
        }
    }

    pub fn admits_int(self, n: i64) -> bool {
        match self {
            Refn::Int { lo, hi } => lo.is_none_or(|l| n >= l) && hi.is_none_or(|h| n <= h),
            Refn::Text { .. } => false,
        }
    }

    pub fn admits_str(self, s: &str) -> bool {
        match self {
            Refn::Text { max } => s.chars().count() as u32 <= max,
            Refn::Int { .. } => false,
        }
    }

    pub fn show(self) -> String {
        let b = |x: Option<i64>| x.map(|n| n.to_string()).unwrap_or_default();
        match self {
            Refn::Int { lo, hi } => format!("Int[{}..{}]", b(lo), b(hi)),
            Refn::Text { max } => format!("Text[..{max}]"),
        }
    }
}

/// Tipo com rótulo de integridade, refinamento, limite de tokens (para `Text`) e cadeia de origem.
#[derive(Clone, Debug)]
pub struct LTy {
    pub ty: Ty,
    pub lab: Label,
    pub tok: Option<u64>,
    pub origin: Vec<String>,
    pub refn: Option<Refn>,
}

impl LTy {
    pub fn trusted(ty: Ty) -> Self {
        LTy { ty, lab: Label::Trusted, tok: None, origin: Vec::new(), refn: None }
    }

    /// Valor de um tipo declarado: o refinamento de texto também limita os tokens (4 caracteres por token).
    pub fn declared(ty: Ty, lab: Label, refn: Option<Refn>, origin: Vec<String>) -> Self {
        let tok = match refn {
            Some(Refn::Text { max }) => Some((max as u64).div_ceil(4)),
            _ => None,
        };
        LTy { ty, lab, tok, origin, refn }
    }

    pub fn never() -> Self {
        LTy::trusted(Ty::Never)
    }

    /// Encontro de rótulos; a origem segue o operando de menor integridade.
    pub fn meet_from(&mut self, other: &LTy) {
        if other.lab < self.lab {
            self.lab = other.lab;
            self.origin = other.origin.clone();
        }
    }

    pub fn with_step(mut self, step: impl Into<String>) -> Self {
        if self.lab < Label::Trusted {
            self.origin.push(step.into());
        }
        self
    }
}
