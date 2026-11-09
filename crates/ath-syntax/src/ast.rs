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
    pub class_span: Span,
    /// `compensable by <ferramenta>`: a transição que leva o mundo de volta ao invariante.
    pub compensator: Option<Ident>,
    /// Como reconciliar um resultado desconhecido (seção 3.5 da spec).
    pub provider: ProviderClass,
    /// Latência declarada (contrato Δ), em segundos.
    pub latency: f64,
    /// Custo declarado por chamada, em US$.
    pub usd: f64,
}

impl ToolDecl {
    /// Ferramenta com consequência remota: recebe uma chave de idempotência.
    pub fn keyed(&self) -> bool {
        self.params.iter().any(|p| p.ty.base_name() == "IdemKey")
    }
}

/// Classe de provedor: o protocolo de reconciliação disponível após `TimeoutUnknown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderClass {
    /// Reenviar com a mesma chave não repete o efeito.
    Idempotent,
    /// O estado da operação pode ser consultado pela chave.
    Queryable,
    /// Nenhum protocolo: resultado desconhecido não é reconciliável.
    Opaque,
}

impl ProviderClass {
    pub fn name(self) -> &'static str {
        match self {
            ProviderClass::Idempotent => "idempotent",
            ProviderClass::Queryable => "queryable",
            ProviderClass::Opaque => "opaque",
        }
    }
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
    /// `handle <efeito>(…)`: tratadores de efeitos algébricos na fronteira do agente.
    pub handles: Vec<HandleClause>,
}

/// `handle timeout_unknown(p: own Pending[_]) -> T { … }`
#[derive(Clone, Debug)]
pub struct HandleClause {
    pub effect: Ident,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Block,
    pub span: Span,
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
    /// `own T`: posse linear (usado exatamente uma vez).
    Own(Box<TypeExpr>, Span),
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
            TypeExpr::Refined { span, .. }
            | TypeExpr::Cap(_, span)
            | TypeExpr::Labeled(_, _, span)
            | TypeExpr::Own(_, span) => *span,
        }
    }

    /// Remove o rótulo de integridade e a marca `own`, se houver.
    pub fn unlabeled(&self) -> &TypeExpr {
        match self {
            TypeExpr::Labeled(inner, ..) | TypeExpr::Own(inner, _) => inner.unlabeled(),
            other => other,
        }
    }

    /// O tipo foi declarado com posse linear (`own T`)?
    pub fn is_own(&self) -> bool {
        match self {
            TypeExpr::Own(..) => true,
            TypeExpr::Labeled(inner, ..) => inner.is_own(),
            _ => false,
        }
    }

    /// Nome do tipo base (`Int`, `Text`, nome do ADT…), sem rótulo nem refinamento.
    pub fn base_name(&self) -> &str {
        match self.unlabeled() {
            TypeExpr::Named(id) => &id.name,
            TypeExpr::Refined { base, .. } => &base.name,
            TypeExpr::Cap(..) => "Cap",
            TypeExpr::Labeled(..) | TypeExpr::Own(..) => unreachable!(),
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
    Call {
        callee: Ident,
        args: Vec<Expr>,
    },
    Method {
        recv: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
    },
    Ctor {
        name: Ident,
        fields: Vec<(Ident, Expr)>,
    },
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    If {
        cond: Box<Expr>,
        then: Block,
        els: Option<Block>,
    },
    Match {
        scrut: Box<Expr>,
        arms: Vec<Arm>,
    },
    Block(Block),
    Return(Box<Expr>),
    Infer {
        model: Ident,
        out: TypeExpr,
        fields: Vec<(Ident, Expr)>,
    },
    Refine {
        draft: Box<Expr>,
        policy: Ident,
        retry: u32,
        els: Block,
    },
    Quote(Box<Expr>),
    Commit(Box<Expr>),
    /// `atomic recover <sonda> { … } on failure(e) { … } on compensation_failed(cf) { … }`
    Atomic {
        recover: Box<Expr>,
        body: Block,
        on_failure: Option<(Ident, Block)>,
        on_cf: Option<(Ident, Block)>,
    },
    /// `await <condição> within <segundos>`
    Await {
        cond: Box<Expr>,
        within: f64,
    },
    /// `reconcile p fuel n else { … }`: protocolo de reconciliação de um resultado desconhecido.
    Reconcile {
        pending: Ident,
        fuel: u32,
        els: Block,
    },
    /// `transfer p`: entrega a obrigação ao reconciliador durável.
    Transfer(Ident),
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
