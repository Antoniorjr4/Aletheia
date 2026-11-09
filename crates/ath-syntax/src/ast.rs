//! AST de Aletheia Core 0. Todo nó relevante carrega seu `Span`.

use crate::span::Span;

/// Reticulado de integridade do Marco Zero: Untrusted ⊑ Endorsed ⊑ Trusted.
/// (O elemento `Bounded(ρ, ε, x₀)` da spec v0.2 entra junto com `certify`.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Label {
    Untrusted,
    Endorsed,
    Trusted,
}

impl Label {
    pub fn meet(self, other: Label) -> Label {
        if self <= other {
            self
        } else {
            other
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Label::Untrusted => "Untrusted",
            Label::Endorsed => "Endorsed",
            Label::Trusted => "Trusted",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Type(TypeDecl),
    Model(ModelDecl),
    Tool(ToolDecl),
    Policy(PolicyDecl),
    Agent(AgentDecl),
}

impl Item {
    pub fn ident(&self) -> &Ident {
        match self {
            Item::Type(d) => &d.name,
            Item::Model(d) => &d.name,
            Item::Tool(d) => &d.name,
            Item::Policy(d) => &d.name,
            Item::Agent(d) => &d.name,
        }
    }

    pub fn name(&self) -> &str {
        &self.ident().name
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Item::Type(_) => "type",
            Item::Model(_) => "model",
            Item::Tool(_) => "tool",
            Item::Policy(_) => "policy",
            Item::Agent(_) => "agent",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub name: Ident,
    pub variants: Vec<Variant>,
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub name: Ident,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: Ident,
    pub ty: TypeExpr,
}

#[derive(Clone, Debug)]
pub struct ModelDecl {
    pub name: Ident,
    pub props: Vec<Prop>,
}

impl ModelDecl {
    pub fn prop(&self, key: &str) -> Option<f64> {
        self.props.iter().find(|p| p.key.name == key).map(|p| p.value)
    }
}

#[derive(Clone, Debug)]
pub struct Prop {
    pub key: Ident,
    pub value: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffClass {
    Reversible,
    Compensable,
    Irrevocable,
}

impl EffClass {
    pub fn name(self) -> &'static str {
        match self {
            EffClass::Reversible => "reversible",
            EffClass::Compensable => "compensable",
            EffClass::Irrevocable => "irrevocable",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub effects: Vec<Ident>,
    pub class: EffClass,
    /// Latência declarada (contrato Δ), em segundos.
    pub latency: f64,
    /// Custo declarado por chamada, em US$.
    pub usd: f64,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: Ident,
    pub ty: TypeExpr,
}

#[derive(Clone, Debug)]
pub struct PolicyDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub struct AgentDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub budget: Vec<Prop>,
    pub budget_span: Span,
    pub handlers: Vec<Handler>,
}

impl AgentDecl {
    pub fn budget(&self, key: &str) -> Option<f64> {
        self.budget.iter().find(|p| p.key.name == key).map(|p| p.value)
    }
}

#[derive(Clone, Debug)]
pub struct Handler {
    pub event: Ident,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub effects: Vec<Ident>,
    pub body: Block,
}

#[derive(Clone, Debug)]
pub enum TypeExpr {
    Named(Ident),
    /// Tipo base refinado por intervalo: `Int[2..64]` (inclusivo) ou `Text[..280]` (até 280 caracteres).
    Refined {
        base: Ident,
        lo: Option<i64>,
        hi: Option<i64>,
        span: Span,
    },
    Cap(Vec<Ident>, Span),
    Labeled(Box<TypeExpr>, Label, Span),
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Named(id) => id.span,
            TypeExpr::Refined { span, .. } | TypeExpr::Cap(_, span) | TypeExpr::Labeled(_, _, span) => *span,
        }
    }

    /// Remove o rótulo de integridade, se houver.
    pub fn unlabeled(&self) -> &TypeExpr {
        match self {
            TypeExpr::Labeled(inner, ..) => inner.unlabeled(),
            other => other,
        }
    }

    /// Nome do tipo base (`Int`, `Text`, nome do ADT…), sem rótulo nem refinamento.
    pub fn base_name(&self) -> &str {
        match self.unlabeled() {
            TypeExpr::Named(id) => &id.name,
            TypeExpr::Refined { base, .. } => &base.name,
            TypeExpr::Cap(..) => "Cap",
            TypeExpr::Labeled(..) => unreachable!(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let { name: Ident, value: Expr, span: Span },
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    Str(String),
    Var(String),
    Field(Box<Expr>, Ident),
    Call { callee: Ident, args: Vec<Expr> },
    Method { recv: Box<Expr>, method: Ident, args: Vec<Expr> },
    Ctor { name: Ident, fields: Vec<(Ident, Expr)> },
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    If { cond: Box<Expr>, then: Block, els: Option<Block> },
    Match { scrut: Box<Expr>, arms: Vec<Arm> },
    Block(Block),
    Return(Box<Expr>),
    Infer { model: Ident, out: TypeExpr, fields: Vec<(Ident, Expr)> },
    Refine { draft: Box<Expr>, policy: Ident, retry: u32, els: Block },
    Quote(Box<Expr>),
    Commit(Box<Expr>),
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub variant: Ident,
    pub binds: Vec<Ident>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Concat,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::Concat => "++",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
}
