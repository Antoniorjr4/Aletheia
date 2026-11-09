//! Parser descendente recursivo de Aletheia Core 0.

use crate::ast::*;
use crate::lexer::{lex, Tok, Token};
use crate::span::Span;

#[derive(Clone, Debug)]
pub struct ParseError {
    pub msg: String,
    pub span: Span,
}

type PResult<T> = Result<T, ParseError>;

pub fn parse_module(src: &str) -> PResult<Module> {
    let toks = lex(src).map_err(|e| ParseError { msg: e.msg, span: e.span })?;
    let mut p = Parser { toks, pos: 0, no_ctor: false };
    p.module()
}

/// Analisa uma expressão isolada (usado para decodificar saídas de modelo e argumentos de CLI).
pub fn parse_expr_str(src: &str) -> PResult<Expr> {
    let toks = lex(src).map_err(|e| ParseError { msg: e.msg, span: e.span })?;
    let mut p = Parser { toks, pos: 0, no_ctor: false };
    let e = p.expr()?;
    if *p.peek() != Tok::Eof {
        return Err(p.err(format!("conteúdo inesperado após a expressão: {}", p.peek().describe())));
    }
    Ok(e)
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    /// Proíbe `Nome { ... }` como construtor (condições de `if` e escrutínios de `match`).
    no_ctor: bool,
}

fn is_upper(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == t {
            self.bump();
            true
        } else {
            false
        }
    }

    fn err(&self, msg: impl Into<String>) -> ParseError {
        ParseError { msg: msg.into(), span: self.span() }
    }

    fn expect(&mut self, t: Tok) -> PResult<Span> {
        if *self.peek() == t {
            Ok(self.bump().span)
        } else {
            Err(self.err(format!("esperado {}, encontrado {}", t.describe(), self.peek().describe())))
        }
    }

    fn ident(&mut self) -> PResult<Ident> {
        match self.peek().clone() {
            Tok::Ident(name) => {
                let span = self.bump().span;
                Ok(Ident { name, span })
            }
            other => Err(self.err(format!("esperado identificador, encontrado {}", other.describe()))),
        }
    }

    fn is_contextual(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s == kw)
    }

    fn number(&mut self) -> PResult<f64> {
        let neg = self.eat(&Tok::Minus);
        let v = match self.peek().clone() {
            Tok::Int(n) => n as f64,
            Tok::Float(x) => x,
            other => return Err(self.err(format!("esperado número, encontrado {}", other.describe()))),
        };
        self.bump();
        Ok(if neg { -v } else { v })
    }

    // ------------------------------------------------------------ itens

    fn module(&mut self) -> PResult<Module> {
        self.expect(Tok::Module)?;
        let mut name = self.ident()?.name;
        while self.eat(&Tok::Dot) {
            name.push('.');
            name.push_str(&self.ident()?.name);
        }
        let mut items = Vec::new();
        while *self.peek() != Tok::Eof {
            items.push(self.item()?);
        }
        Ok(Module { name, items })
    }

    fn item(&mut self) -> PResult<Item> {
        match self.peek() {
            Tok::Type => self.type_decl().map(Item::Type),
            Tok::Model => self.model_decl().map(Item::Model),
            Tok::Tool => self.tool_decl().map(Item::Tool),
            Tok::Policy => self.policy_decl().map(Item::Policy),
            Tok::Agent => self.agent_decl().map(Item::Agent),
            other => Err(self.err(format!(
                "esperado `type`, `model`, `tool`, `policy` ou `agent`, encontrado {}",
                other.describe()
            ))),
        }
    }

    fn type_decl(&mut self) -> PResult<TypeDecl> {
        self.expect(Tok::Type)?;
        let name = self.ident()?;
        self.expect(Tok::Assign)?;
        let mut variants = Vec::new();
        while self.eat(&Tok::Pipe) {
            let vname = self.ident()?;
            let mut fields = Vec::new();
            if self.eat(&Tok::LBrace) {
                while *self.peek() != Tok::RBrace {
                    let fname = self.ident()?;
                    self.expect(Tok::Colon)?;
                    let ty = self.ty()?;
                    fields.push(Field { name: fname, ty });
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RBrace)?;
            }
            variants.push(Variant { name: vname, fields });
        }
        if variants.is_empty() {
            return Err(self.err("um tipo precisa de ao menos uma variante: `type T = | A | B { x: Int }`"));
        }
        Ok(TypeDecl { name, variants })
    }

    fn model_decl(&mut self) -> PResult<ModelDecl> {
        self.expect(Tok::Model)?;
        let name = self.ident()?;
        self.expect(Tok::LBrace)?;
        let mut props = Vec::new();
        while *self.peek() != Tok::RBrace {
            let key = self.ident()?;
            self.expect(Tok::Assign)?;
            let value = self.number()?;
            props.push(Prop { key, value });
            self.eat(&Tok::Comma);
        }
        self.expect(Tok::RBrace)?;
        Ok(ModelDecl { name, props })
    }

    fn params(&mut self) -> PResult<Vec<Param>> {
        self.expect(Tok::LParen)?;
        let mut params = Vec::new();
        while *self.peek() != Tok::RParen {
            let name = self.ident()?;
            self.expect(Tok::Colon)?;
            let ty = self.ty()?;
            params.push(Param { name, ty });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen)?;
        Ok(params)
    }

    fn effect_row(&mut self) -> PResult<Vec<Ident>> {
        self.expect(Tok::Bang)?;
        self.expect(Tok::LBrace)?;
        let mut effs = Vec::new();
        while *self.peek() != Tok::RBrace {
            effs.push(self.effect_name()?);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RBrace)?;
        Ok(effs)
    }

    fn effect_name(&mut self) -> PResult<Ident> {
        let first = self.ident()?;
        let mut name = first.name;
        let mut span = first.span;
        while self.eat(&Tok::Dot) {
            let next = self.ident()?;
            name.push('.');
            name.push_str(&next.name);
            span = span.to(next.span);
        }
        Ok(Ident { name, span })
    }

    fn tool_decl(&mut self) -> PResult<ToolDecl> {
        self.expect(Tok::Tool)?;
        let name = self.ident()?;
        let params = self.params()?;
        self.expect(Tok::Arrow)?;
        let ret = self.ty()?;
        let effects = self.effect_row()?;
        let mut class = EffClass::Reversible;
        let mut latency = 0.0;
        let mut usd = 0.0;
        loop {
            if self.is_contextual("reversible") {
                self.bump();
                class = EffClass::Reversible;
            } else if self.is_contextual("compensable") {
                self.bump();
                class = EffClass::Compensable;
            } else if self.is_contextual("irrevocable") {
                self.bump();
                class = EffClass::Irrevocable;
            } else if self.is_contextual("latency") {
                self.bump();
                latency = self.number()?;
            } else if self.is_contextual("usd") {
                self.bump();
                usd = self.number()?;
            } else {
                break;
            }
        }
        Ok(ToolDecl { name, params, ret, effects, class, latency, usd })
    }

    fn policy_decl(&mut self) -> PResult<PolicyDecl> {
        self.expect(Tok::Policy)?;
        let name = self.ident()?;
        let params = self.params()?;
        self.expect(Tok::Arrow)?;
        let ret = self.ty()?;
        self.expect(Tok::Assign)?;
        let body = self.expr()?;
        Ok(PolicyDecl { name, params, ret, body })
    }

    fn agent_decl(&mut self) -> PResult<AgentDecl> {
        self.expect(Tok::Agent)?;
        let name = self.ident()?;
        let params = self.params()?;
        let bstart = self.expect(Tok::Budget)?;
        self.expect(Tok::LBrack)?;
        let mut budget = Vec::new();
        while *self.peek() != Tok::RBrack {
            let key = self.ident()?;
            self.expect(Tok::Le)?;
            let value = self.number()?;
            budget.push(Prop { key, value });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        let bend = self.expect(Tok::RBrack)?;
        self.expect(Tok::LBrace)?;
        let mut handlers = Vec::new();
        while self.eat(&Tok::On) {
            let event = self.ident()?;
            let params = self.params()?;
            self.expect(Tok::Arrow)?;
            let ret = self.ty()?;
            let effects = self.effect_row()?;
            let body = self.block()?;
            handlers.push(Handler { event, params, ret, effects, body });
        }
        self.expect(Tok::RBrace)?;
        Ok(AgentDecl { name, params, budget, budget_span: bstart.to(bend), handlers })
    }

    fn ty(&mut self) -> PResult<TypeExpr> {
        let start = self.span();
        let base = if self.is_contextual("Cap") && *self.peek_at(1) == Tok::LBrack {
            self.bump();
            self.bump();
            let mut effs = vec![self.effect_name()?];
            while self.eat(&Tok::Plus) {
                effs.push(self.effect_name()?);
            }
            let end = self.expect(Tok::RBrack)?;
            TypeExpr::Cap(effs, start.to(end))
        } else {
            TypeExpr::Named(self.ident()?)
        };
        if self.eat(&Tok::Caret) {
            let l = self.ident()?;
            let label = match l.name.as_str() {
                "Untrusted" => Label::Untrusted,
                "Endorsed" => Label::Endorsed,
                "Trusted" => Label::Trusted,
                other => {
                    return Err(ParseError {
                        msg: format!("rótulo de integridade desconhecido `{other}` (use Untrusted, Endorsed ou Trusted)"),
                        span: l.span,
                    })
                }
            };
            return Ok(TypeExpr::Labeled(Box::new(base), label, start.to(l.span)));
        }
        Ok(base)
    }

    // ------------------------------------------------------------ blocos

    fn block(&mut self) -> PResult<Block> {
        let lo = self.expect(Tok::LBrace)?;
        let saved = self.no_ctor;
        self.no_ctor = false;
        let mut stmts = Vec::new();
        let mut tail = None;
        loop {
            if *self.peek() == Tok::RBrace {
                break;
            }
            if *self.peek() == Tok::Eof {
                return Err(self.err("bloco sem `}` de fechamento"));
            }
            if *self.peek() == Tok::Let {
                let start = self.bump().span;
                let name = self.ident()?;
                self.expect(Tok::Assign)?;
                let value = self.expr()?;
                let span = start.to(value.span);
                stmts.push(Stmt::Let { name, value, span });
                self.eat(&Tok::Semi);
                continue;
            }
            let e = self.expr()?;
            self.eat(&Tok::Semi);
            if *self.peek() == Tok::RBrace {
                tail = Some(Box::new(e));
            } else {
                stmts.push(Stmt::Expr(e));
            }
        }
        let hi = self.expect(Tok::RBrace)?;
        self.no_ctor = saved;
        Ok(Block { stmts, tail, span: lo.to(hi) })
    }

    // ------------------------------------------------------------ expressões

    pub fn expr(&mut self) -> PResult<Expr> {
        self.or_expr()
    }

    fn bin(op: BinOp, l: Expr, r: Expr) -> Expr {
        let span = l.span.to(r.span);
        Expr { kind: ExprKind::Binary(op, Box::new(l), Box::new(r)), span }
    }

    fn or_expr(&mut self) -> PResult<Expr> {
        let mut l = self.and_expr()?;
        while self.eat(&Tok::OrOr) {
            let r = self.and_expr()?;
            l = Self::bin(BinOp::Or, l, r);
        }
        Ok(l)
    }

    fn and_expr(&mut self) -> PResult<Expr> {
        let mut l = self.cmp_expr()?;
        while self.eat(&Tok::AndAnd) {
            let r = self.cmp_expr()?;
            l = Self::bin(BinOp::And, l, r);
        }
        Ok(l)
    }

    fn cmp_expr(&mut self) -> PResult<Expr> {
        let l = self.concat_expr()?;
        let op = match self.peek() {
            Tok::EqEq => BinOp::Eq,
            Tok::Ne => BinOp::Ne,
            Tok::Lt => BinOp::Lt,
            Tok::Le => BinOp::Le,
            Tok::Gt => BinOp::Gt,
            Tok::Ge => BinOp::Ge,
            _ => return Ok(l),
        };
        self.bump();
        let r = self.concat_expr()?;
        Ok(Self::bin(op, l, r))
    }

    fn concat_expr(&mut self) -> PResult<Expr> {
        let mut l = self.add_expr()?;
        while self.eat(&Tok::PlusPlus) {
            let r = self.add_expr()?;
            l = Self::bin(BinOp::Concat, l, r);
        }
        Ok(l)
    }

    fn add_expr(&mut self) -> PResult<Expr> {
        let mut l = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => return Ok(l),
            };
            self.bump();
            let r = self.mul_expr()?;
            l = Self::bin(op, l, r);
        }
    }

    fn mul_expr(&mut self) -> PResult<Expr> {
        let mut l = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                _ => return Ok(l),
            };
            self.bump();
            let r = self.unary()?;
            l = Self::bin(op, l, r);
        }
    }

    fn unary(&mut self) -> PResult<Expr> {
        let start = self.span();
        if self.eat(&Tok::Not) {
            let e = self.unary()?;
            let span = start.to(e.span);
            return Ok(Expr { kind: ExprKind::Unary(UnOp::Not, Box::new(e)), span });
        }
        if self.eat(&Tok::Minus) {
            let e = self.unary()?;
            let span = start.to(e.span);
            return Ok(Expr { kind: ExprKind::Unary(UnOp::Neg, Box::new(e)), span });
        }
        self.postfix()
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        while *self.peek() == Tok::Dot {
            self.bump();
            let name = self.ident()?;
            if *self.peek() == Tok::LParen {
                let args = self.args()?;
                let span = e.span.to(self.prev_span());
                e = Expr { kind: ExprKind::Method { recv: Box::new(e), method: name, args }, span };
            } else {
                let span = e.span.to(name.span);
                e = Expr { kind: ExprKind::Field(Box::new(e), name), span };
            }
        }
        Ok(e)
    }

    fn args(&mut self) -> PResult<Vec<Expr>> {
        self.expect(Tok::LParen)?;
        let saved = self.no_ctor;
        self.no_ctor = false;
        let mut args = Vec::new();
        while *self.peek() != Tok::RParen {
            args.push(self.expr()?);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen)?;
        self.no_ctor = saved;
        Ok(args)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let start = self.span();
        match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                Ok(Expr { kind: ExprKind::Int(n), span: start })
            }
            Tok::Float(_) => Err(self.err("números decimais só são aceitos em declarações (model, tool, budget)")),
            Tok::Str(s) => {
                self.bump();
                Ok(Expr { kind: ExprKind::Str(s), span: start })
            }
            Tok::True => {
                self.bump();
                Ok(Expr { kind: ExprKind::Bool(true), span: start })
            }
            Tok::False => {
                self.bump();
                Ok(Expr { kind: ExprKind::Bool(false), span: start })
            }
            Tok::Ident(name) => {
                let id = self.ident()?;
                if *self.peek() == Tok::LParen {
                    let args = self.args()?;
                    let span = start.to(self.prev_span());
                    return Ok(Expr { kind: ExprKind::Call { callee: id, args }, span });
                }
                if is_upper(&name) {
                    let mut fields = Vec::new();
                    if *self.peek() == Tok::LBrace && !self.no_ctor {
                        self.bump();
                        while *self.peek() != Tok::RBrace {
                            let f = self.ident()?;
                            self.expect(Tok::Colon)?;
                            let v = self.expr()?;
                            fields.push((f, v));
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                        self.expect(Tok::RBrace)?;
                    }
                    let span = start.to(self.prev_span());
                    return Ok(Expr { kind: ExprKind::Ctor { name: id, fields }, span });
                }
                Ok(Expr { kind: ExprKind::Var(name), span: start })
            }
            Tok::LParen => {
                self.bump();
                let saved = self.no_ctor;
                self.no_ctor = false;
                let mut e = self.expr()?;
                self.no_ctor = saved;
                let end = self.expect(Tok::RParen)?;
                e.span = start.to(end);
                Ok(e)
            }
            Tok::LBrace => {
                let b = self.block()?;
                let span = b.span;
                Ok(Expr { kind: ExprKind::Block(b), span })
            }
            Tok::If => self.if_expr(),
            Tok::Match => {
                self.bump();
                let saved = self.no_ctor;
                self.no_ctor = true;
                let scrut = self.expr()?;
                self.no_ctor = saved;
                self.expect(Tok::LBrace)?;
                let mut arms = Vec::new();
                while *self.peek() != Tok::RBrace {
                    let variant = self.ident()?;
                    let mut binds = Vec::new();
                    if self.eat(&Tok::LBrace) {
                        while *self.peek() != Tok::RBrace {
                            binds.push(self.ident()?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                        self.expect(Tok::RBrace)?;
                    }
                    self.expect(Tok::FatArrow)?;
                    let body = self.expr()?;
                    let span = variant.span.to(body.span);
                    arms.push(Arm { variant, binds, body, span });
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                let end = self.expect(Tok::RBrace)?;
                Ok(Expr { kind: ExprKind::Match { scrut: Box::new(scrut), arms }, span: start.to(end) })
            }
            Tok::Return => {
                self.bump();
                let v = self.expr()?;
                let span = start.to(v.span);
                Ok(Expr { kind: ExprKind::Return(Box::new(v)), span })
            }
            Tok::Infer => {
                self.bump();
                let model = self.ident()?;
                self.expect(Tok::Arrow)?;
                let out = self.ty()?;
                self.expect(Tok::LBrace)?;
                let saved = self.no_ctor;
                self.no_ctor = false;
                let mut fields = Vec::new();
                while *self.peek() != Tok::RBrace {
                    let k = self.ident()?;
                    self.expect(Tok::Assign)?;
                    let v = self.expr()?;
                    fields.push((k, v));
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.no_ctor = saved;
                let end = self.expect(Tok::RBrace)?;
                Ok(Expr { kind: ExprKind::Infer { model, out, fields }, span: start.to(end) })
            }
            Tok::Refine => {
                self.bump();
                let draft = self.postfix()?;
                self.expect(Tok::By)?;
                let policy = self.ident()?;
                let mut retry = 1;
                if self.eat(&Tok::Retry) {
                    match self.peek().clone() {
                        Tok::Int(n) if (1..=64).contains(&n) => {
                            self.bump();
                            retry = n as u32;
                        }
                        _ => return Err(self.err("`retry` exige um inteiro entre 1 e 64")),
                    }
                }
                self.expect(Tok::Else)?;
                let els = self.block()?;
                let span = start.to(els.span);
                Ok(Expr { kind: ExprKind::Refine { draft: Box::new(draft), policy, retry, els }, span })
            }
            Tok::Quote => {
                self.bump();
                self.expect(Tok::LParen)?;
                let saved = self.no_ctor;
                self.no_ctor = false;
                let e = self.expr()?;
                self.no_ctor = saved;
                let end = self.expect(Tok::RParen)?;
                Ok(Expr { kind: ExprKind::Quote(Box::new(e)), span: start.to(end) })
            }
            Tok::Commit => {
                self.bump();
                let e = self.unary()?;
                let span = start.to(e.span);
                Ok(Expr { kind: ExprKind::Commit(Box::new(e)), span })
            }
            other => Err(self.err(format!("esperada uma expressão, encontrado {}", other.describe()))),
        }
    }

    fn if_expr(&mut self) -> PResult<Expr> {
        let start = self.expect(Tok::If)?;
        let saved = self.no_ctor;
        self.no_ctor = true;
        let cond = self.expr()?;
        self.no_ctor = saved;
        let then = self.block()?;
        let mut end = then.span;
        let els = if self.eat(&Tok::Else) {
            if *self.peek() == Tok::If {
                let nested = self.if_expr()?;
                end = nested.span;
                Some(Block { span: nested.span, stmts: vec![], tail: Some(Box::new(nested)) })
            } else {
                let b = self.block()?;
                end = b.span;
                Some(b)
            }
        } else {
            None
        };
        Ok(Expr { kind: ExprKind::If { cond: Box::new(cond), then, els }, span: start.to(end) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_type_and_policy() {
        let src = r#"
module t
type Action = | Restart { deploy: Text } | Scale { deploy: Text, to: Int }
policy Safe(a: Action) -> Bool = match a { Restart { deploy } => deploy == "api", Scale { to } => to >= 2 }
"#;
        let m = parse_module(src).unwrap();
        assert_eq!(m.items.len(), 2);
        assert_eq!(m.items[0].name(), "Action");
    }

    #[test]
    fn parses_ctor_expression() {
        let e = parse_expr_str(r#"Scale { deploy: "api", to: 3 }"#).unwrap();
        assert!(matches!(e.kind, ExprKind::Ctor { .. }));
    }
}
