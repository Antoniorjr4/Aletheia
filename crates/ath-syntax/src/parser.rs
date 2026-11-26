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
    parse_module_all(src).map_err(|mut errs| errs.swap_remove(0))
}

/// Como [`parse_module`], mas devolve todos os erros de sintaxe independentes do módulo.
///
/// A recuperação é por item de topo: um erro dentro de um item descarta o item inteiro e a análise
/// recomeça no próximo `type`/`model`/`authority`/`witness`/`bind`/`tool`/`policy`/`agent` em
/// profundidade zero de chaves, colchetes e parênteses. Se os delimitadores do item não se
/// equilibram, não há ponto seguro de recomeço e a análise para ali: na dúvida, um erro a menos,
/// nunca um erro em cascata. Nenhuma árvore sai daqui se houver erro.
pub fn parse_module_all(src: &str) -> Result<Module, Vec<ParseError>> {
    let toks = lex(src).map_err(|e| vec![ParseError { msg: e.msg, span: e.span }])?;
    // Primeira passada: só para conhecer os tipos algébricos do módulo. O braço curinga `_` de
    // um `match` é expandido na segunda, para as variantes que os outros braços não nomeiam; assim
    // o resto do compilador e os dois runtimes só veem braços com variante explícita. Com erro de
    // sintaxe na primeira, a segunda não roda: sem todos os tipos, a expansão do curinga poderia
    // acusar um erro que é só consequência do item descartado.
    let mut first = Parser { toks: toks.clone(), pos: 0, no_ctor: false, adts: None };
    let (m, errs) = first.module_recovering();
    if !errs.is_empty() {
        return Err(errs);
    }
    let mut p = Parser { toks, pos: 0, no_ctor: false, adts: Some(adt_index(&m)) };
    let (m, errs) = p.module_recovering();
    if errs.is_empty() {
        Ok(m)
    } else {
        Err(errs)
    }
}

/// Para cada variante, a lista de variantes do tipo que a declara, na ordem de declaração.
fn adt_index(m: &Module) -> std::collections::HashMap<String, Vec<String>> {
    let mut out = std::collections::HashMap::new();
    for it in &m.items {
        if let Item::Type(t) = it {
            let all: Vec<String> = t.variants.iter().map(|v| v.name.name.clone()).collect();
            for v in &all {
                out.entry(v.clone()).or_insert_with(|| all.clone());
            }
        }
    }
    out
}

/// Analisa uma expressão isolada (usado para decodificar saídas de modelo e argumentos de CLI).
pub fn parse_expr_str(src: &str) -> PResult<Expr> {
    let toks = lex(src).map_err(|e| ParseError { msg: e.msg, span: e.span })?;
    let mut p = Parser { toks, pos: 0, no_ctor: false, adts: None };
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
    /// Variante → variantes do seu tipo. `None` na primeira passada e em expressões isoladas.
    adts: Option<std::collections::HashMap<String, Vec<String>>>,
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

    fn module_header(&mut self) -> PResult<String> {
        self.expect(Tok::Module)?;
        let mut name = self.ident()?.name;
        while self.eat(&Tok::Dot) {
            name.push('.');
            name.push_str(&self.ident()?.name);
        }
        Ok(name)
    }

    /// Itens analisados sem erro e todos os erros independentes (ver [`parse_module_all`]).
    fn module_recovering(&mut self) -> (Module, Vec<ParseError>) {
        let mut errs = Vec::new();
        let name = match self.module_header() {
            Ok(n) => n,
            // Sem cabeçalho válido não há como saber onde começa o primeiro item.
            Err(e) => return (Module { name: String::new(), items: Vec::new() }, vec![e]),
        };
        let mut items = Vec::new();
        while *self.peek() != Tok::Eof {
            let start = self.pos;
            match self.item() {
                Ok(it) => items.push(it),
                Err(e) => {
                    let at = self.pos;
                    errs.push(e);
                    match self.resync(start, at) {
                        Some(next) => self.pos = next,
                        None => break,
                    }
                }
            }
        }
        (Module { name, items }, errs)
    }

    /// Próximo início de item em profundidade zero, a partir do começo do item que falhou e não
    /// antes do ponto do erro. `None` se os delimitadores se desequilibram ou se o arquivo acaba.
    fn resync(&self, start: usize, at: usize) -> Option<usize> {
        let mut depth: u32 = 0;
        for (i, tk) in self.toks.iter().enumerate().skip(start) {
            let t = &tk.tok;
            if depth == 0 && i > start && i >= at && Self::starts_item(t) {
                return Some(i);
            }
            match t {
                Tok::LBrace | Tok::LBrack | Tok::LParen => depth += 1,
                Tok::RBrace | Tok::RBrack | Tok::RParen => depth = depth.checked_sub(1)?,
                Tok::Eof => return None,
                _ => {}
            }
        }
        None
    }

    fn starts_item(t: &Tok) -> bool {
        matches!(
            t,
            Tok::Type | Tok::Model | Tok::Authority | Tok::Witness | Tok::Bind | Tok::Tool | Tok::Policy | Tok::Agent
        )
    }

    fn item(&mut self) -> PResult<Item> {
        match self.peek() {
            Tok::Type => self.type_decl().map(Item::Type),
            Tok::Model => self.model_decl().map(Item::Model),
            Tok::Authority => self.authority_decl().map(Item::Authority),
            Tok::Witness => self.witness_decl().map(Item::Witness),
            Tok::Bind => self.bind_decl().map(Item::Bind),
            Tok::Tool => self.tool_decl().map(Item::Tool),
            Tok::Policy => self.policy_decl().map(Item::Policy),
            Tok::Agent => self.agent_decl().map(Item::Agent),
            other => Err(self.err(format!(
                "esperado `type`, `model`, `authority`, `witness`, `bind`, `tool`, `policy` ou `agent`, encontrado {}",
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
        let mut meta = Vec::new();
        while *self.peek() != Tok::RBrace {
            let key = self.ident()?;
            self.expect(Tok::Assign)?;
            if let Tok::Str(s) = self.peek().clone() {
                self.bump();
                meta.push((key, s));
            } else {
                let value = self.number()?;
                props.push(Prop { key, value });
            }
            self.eat(&Tok::Comma);
        }
        self.expect(Tok::RBrace)?;
        Ok(ModelDecl { name, props, meta })
    }

    fn authority_decl(&mut self) -> PResult<AuthorityDecl> {
        self.expect(Tok::Authority)?;
        let name = self.ident()?;
        self.expect(Tok::LBrace)?;
        let mut d = AuthorityDecl {
            name,
            key: String::new(),
            key_span: self.span(),
            ops: None,
            ops_span: self.span(),
            op_idents: Vec::new(),
            typed: false,
            max_age: None,
            anchored: false,
            clock: None,
            outcomes: None,
        };
        let (mut has_key, mut textual) = (false, false);
        while *self.peek() != Tok::RBrace {
            let field = self.ident()?;
            self.expect(Tok::Assign)?;
            let span = self.span();
            match (field.name.as_str(), self.peek().clone()) {
                ("key", Tok::Str(k)) => {
                    self.bump();
                    (d.key, d.key_span, has_key) = (k, span, true);
                }
                // forma tipada: uma lista de operações
                ("ops", Tok::LBrack) => {
                    self.bump();
                    while *self.peek() != Tok::RBrack {
                        d.op_idents.push(self.ident()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(Tok::RBrack)?;
                    d.ops = Some(d.op_idents.iter().map(|i| i.name.clone()).collect());
                    (d.ops_span, d.typed) = (span, true);
                }
                // forma textual (RC3)
                ("ops", Tok::Str(o)) => {
                    self.bump();
                    d.ops = Some(o.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect());
                    (d.ops_span, textual) = (span, true);
                }
                ("max_age", Tok::Int(n)) if n > 0 => {
                    self.bump();
                    d.max_age = Some(n as u64);
                }
                ("ledger", Tok::Ident(l)) if l == "anchored" => {
                    self.bump();
                    (d.anchored, d.typed) = (true, true);
                }
                ("ledger", Tok::Str(l)) if l == "anchored" => {
                    self.bump();
                    (d.anchored, textual) = (true, true);
                }
                ("clock", Tok::Ident(_)) => {
                    d.clock = Some(self.ident()?);
                    d.typed = true;
                }
                ("outcomes", Tok::Ident(_)) => {
                    d.outcomes = Some(self.ident()?);
                    d.typed = true;
                }
                (f, _) => {
                    return Err(self.err(format!(
                        "campo inválido em `authority`: `{f}` (esperado key = \"ed25519:…\", ops = [op, op], \
                         max_age = segundos, ledger = anchored, clock = testemunha, outcomes = testemunha)"
                    )))
                }
            }
            self.eat(&Tok::Comma);
        }
        self.expect(Tok::RBrace)?;
        if !has_key {
            return Err(self.err("uma autoridade declara `key`".to_string()));
        }
        // a forma textual não se mistura com a tipada: a garantia de cada uma é distinta
        if textual && d.typed {
            return Err(self.err(format!(
                "a autoridade `{}` mistura a forma textual (RC3: ops = \"…\", ledger = \"anchored\") com a \
                 tipada (RC4: ops = [..], ledger = anchored, clock, outcomes)",
                d.name.name
            )));
        }
        Ok(d)
    }

    fn witness_decl(&mut self) -> PResult<WitnessDecl> {
        self.expect(Tok::Witness)?;
        let name = self.ident()?;
        self.expect(Tok::LBrace)?;
        let mut d = WitnessDecl { name, key: String::new(), key_span: self.span(), tolerance: None };
        let mut has_key = false;
        while *self.peek() != Tok::RBrace {
            let field = self.ident()?;
            self.expect(Tok::Assign)?;
            let span = self.span();
            match (field.name.as_str(), self.peek().clone()) {
                ("key", Tok::Str(k)) => {
                    self.bump();
                    (d.key, d.key_span, has_key) = (k, span, true);
                }
                ("tolerance", Tok::Int(n)) if n > 0 => {
                    self.bump();
                    d.tolerance = Some(n as u64);
                }
                (f, _) => {
                    return Err(self.err(format!(
                        "campo inválido em `witness`: `{f}` (esperado key = \"ed25519:…\", tolerance = segundos)"
                    )))
                }
            }
            self.eat(&Tok::Comma);
        }
        self.expect(Tok::RBrace)?;
        if !has_key {
            return Err(self.err("uma testemunha declara `key`".to_string()));
        }
        Ok(d)
    }

    /// `bind nome { url = …, auth = secret("V"), status = "…", ferramenta = "MÉTODO /rota -> campo" }`
    fn bind_decl(&mut self) -> PResult<BindDecl> {
        self.expect(Tok::Bind)?;
        let name = self.ident()?;
        self.expect(Tok::LBrace)?;
        let mut d = BindDecl {
            name,
            url: BindUrl::Lit(String::new()),
            url_span: self.span(),
            auth: None,
            status: None,
            routes: Vec::new(),
            keys: Vec::new(),
            finds: Vec::new(),
            settles: Vec::new(),
            mcp: None,
            secret_as: None,
            args: Vec::new(),
            calls: Vec::new(),
            call_finds: Vec::new(),
        };
        let mut has_url = false;
        while *self.peek() != Tok::RBrace {
            let field = self.ident()?;
            // v1.0-RC6: `ferramenta.key = "parâmetro"` e `ferramenta.find = "GET /rota -> campo"`
            if self.eat(&Tok::Dot) {
                let what = self.ident()?;
                self.expect(Tok::Assign)?;
                // v1.0-RC9: `arg.nome = env("V") | "texto"`, configuração do executor
                if field.name == "arg" {
                    let v = match self.peek().clone() {
                        Tok::Ident(w) if w == "env" => {
                            self.bump();
                            self.expect(Tok::LParen)?;
                            let Tok::Str(v) = self.peek().clone() else {
                                return Err(self.err("`env(\"NOME\")` nomeia uma variável de ambiente".to_string()));
                            };
                            self.bump();
                            self.expect(Tok::RParen)?;
                            if v.is_empty() || !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                                return Err(self.err("nome de variável de ambiente inválido em `env`".to_string()));
                            }
                            BindUrl::Env(v)
                        }
                        Tok::Str(v) => {
                            self.bump();
                            BindUrl::Lit(v)
                        }
                        _ => return Err(self.err(format!("`arg.{}` é um texto ou `env(\"NOME\")`", what.name))),
                    };
                    if d.args.iter().any(|(a, _)| a.name == what.name) {
                        return Err(self.err(format!("`arg.{}` declarado duas vezes", what.name)));
                    }
                    d.args.push((what, v));
                    self.eat(&Tok::Comma);
                    continue;
                }
                let Tok::Str(v) = self.peek().clone() else {
                    return Err(self.err(format!("`{}.{}` é um texto", field.name, what.name)));
                };
                match what.name.as_str() {
                    "key" => d.keys.push((field, v)),
                    "find" => match (Route::parse(&v), McpCall::parse(&v)) {
                        (Some(r), _) => d.finds.push((field, r)),
                        (None, Some(c)) => d.call_finds.push((field, c)),
                        _ => return Err(self.err(format!("rota inválida para `{}.find`: `{v}`", field.name))),
                    },
                    "settle" => match v.parse::<u32>() {
                        Ok(n) if n <= 300 => d.settles.push((field, n)),
                        _ => return Err(self.err(format!("`{}.settle` são segundos, de \"0\" a \"300\"", field.name))),
                    },
                    other => {
                        return Err(self.err(format!(
                            "`{}.{other}`: esperado `.key` (o parâmetro que carrega a chave), `.find` (a consulta por chave) ou `.settle` (segundos até a listagem refletir uma escrita)",
                            field.name
                        )))
                    }
                }
                self.bump();
                self.eat(&Tok::Comma);
                continue;
            }
            self.expect(Tok::Assign)?;
            let span = self.span();
            // `env("V")` e `secret("V")`: uma referência ao ambiente do executor, nunca um valor
            let reference = |me: &mut Self, word: &str| -> PResult<Option<String>> {
                match me.peek().clone() {
                    Tok::Ident(w) if w == word => {
                        me.bump();
                        me.expect(Tok::LParen)?;
                        let Tok::Str(v) = me.peek().clone() else {
                            return Err(me.err(format!("`{word}(\"NOME\")` nomeia uma variável de ambiente")));
                        };
                        me.bump();
                        me.expect(Tok::RParen)?;
                        if v.is_empty() || !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                            return Err(me.err(format!("nome de variável de ambiente inválido em `{word}`")));
                        }
                        Ok(Some(v))
                    }
                    _ => Ok(None),
                }
            };
            match field.name.as_str() {
                "url" => {
                    d.url = match reference(self, "env")? {
                        Some(v) => BindUrl::Env(v),
                        None => match self.peek().clone() {
                            Tok::Str(u) => {
                                self.bump();
                                BindUrl::Lit(u)
                            }
                            _ => return Err(self.err("`url` é um texto ou `env(\"NOME\")`".to_string())),
                        },
                    };
                    (d.url_span, has_url) = (span, true);
                }
                "mcp" => {
                    d.mcp = Some(match reference(self, "env")? {
                        Some(v) => BindUrl::Env(v),
                        None => match self.peek().clone() {
                            Tok::Str(u) if !u.trim().is_empty() => {
                                self.bump();
                                BindUrl::Lit(u)
                            }
                            _ => return Err(self.err("`mcp` é o comando do servidor, um texto ou `env(\"NOME\")`".to_string())),
                        },
                    });
                    d.url_span = span;
                }
                "auth" => match reference(self, "secret")? {
                    Some(v) => {
                        d.auth = Some(v);
                        // v1.0-RC9: `as "NOME"`, a variável do ambiente do servidor MCP
                        if matches!(self.peek(), Tok::Ident(w) if w == "as") {
                            self.bump();
                            let Tok::Str(n) = self.peek().clone() else {
                                return Err(self.err("`as` nomeia a variável do ambiente do servidor: `as \"NOME\"`".to_string()));
                            };
                            if n.is_empty() || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                                return Err(self.err(format!("nome de variável inválido em `as \"{n}\"`")));
                            }
                            self.bump();
                            d.secret_as = Some(n);
                        }
                    }
                    None => {
                        return Err(self.err(
                            "`auth` é `secret(\"NOME\")`: o segredo é uma referência ao ambiente do executor, nunca um literal do programa"
                                .to_string(),
                        ))
                    }
                },
                f => {
                    let Tok::Str(r) = self.peek().clone() else {
                        return Err(self.err(format!("a rota de `{f}` é um texto: \"MÉTODO /caminho -> campo\"")));
                    };
                    // v1.0-RC9: uma chamada MCP, `nome(k: v, …) -> caminho`
                    if let (None, Some(c)) = (Route::parse(&r), McpCall::parse(&r)) {
                        if f == "status" {
                            return Err(self.err("`status` é uma rota HTTP; num `bind` MCP a consulta é `.find`".to_string()));
                        }
                        self.bump();
                        d.calls.push((field, c));
                        self.eat(&Tok::Comma);
                        continue;
                    }
                    let Some(route) = Route::parse(&r) else {
                        return Err(self.err(format!(
                            "rota inválida para `{f}`: `{r}` (esperado \"GET|POST|PUT|PATCH /caminho\", com `{{parâmetro}}` e `?query` opcionais, e \"-> campo\" ou \"-> [].campo\"; ou, num `bind` MCP, \"ferramenta(k: v, …) -> caminho\")"
                        )));
                    };
                    self.bump();
                    if f == "status" {
                        d.status = Some(route);
                    } else {
                        d.routes.push((field, route));
                    }
                }
            }
            self.eat(&Tok::Comma);
        }
        self.expect(Tok::RBrace)?;
        match (has_url, d.mcp.is_some()) {
            (false, false) => {
                return Err(
                    self.err("um `bind` declara `url` (HTTP) ou `mcp` (o comando de um servidor MCP)".to_string())
                )
            }
            (true, true) => return Err(self.err("um `bind` é HTTP (`url`) ou MCP (`mcp`), não os dois".to_string())),
            (true, false)
                if !d.calls.is_empty() || !d.call_finds.is_empty() || !d.args.is_empty() || d.secret_as.is_some() =>
            {
                return Err(
                    self.err("chamadas MCP, `arg.…` e `auth … as` só valem num `bind` MCP (`mcp = …`)".to_string())
                )
            }
            (false, true) if !d.routes.is_empty() || !d.finds.is_empty() || d.status.is_some() => {
                return Err(self.err(
                    "um `bind` MCP liga ferramentas a chamadas \"ferramenta(k: v) -> caminho\", não a rotas HTTP"
                        .to_string(),
                ))
            }
            (false, true) if d.auth.is_some() && d.secret_as.is_none() => {
                return Err(self.err(
                    "num `bind` MCP, o segredo vai para o ambiente do servidor: `auth = secret(\"V\") as \"NOME\"`"
                        .to_string(),
                ))
            }
            _ => {}
        }
        Ok(d)
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
        let mut class_span = name.span;
        let mut compensator = None;
        let mut provider = ProviderClass::Opaque;
        let mut latency = 0.0;
        let mut usd = 0.0;
        let mut requires = None;
        loop {
            if self.is_contextual("requires") {
                self.bump();
                requires = Some(self.expr()?);
            } else if self.is_contextual("reversible") {
                class_span = self.bump().span;
                class = EffClass::Reversible;
            } else if self.is_contextual("compensable") {
                class_span = self.bump().span;
                class = EffClass::Compensable;
                if self.eat(&Tok::By) {
                    compensator = Some(self.ident()?);
                }
            } else if self.is_contextual("irrevocable") {
                class_span = self.bump().span;
                class = EffClass::Irrevocable;
            } else if self.is_contextual("provider") {
                self.bump();
                let k = self.ident()?;
                provider = match k.name.as_str() {
                    "idempotent" => ProviderClass::Idempotent,
                    "queryable" => ProviderClass::Queryable,
                    "opaque" => ProviderClass::Opaque,
                    other => {
                        return Err(ParseError {
                            msg: format!(
                                "classe de provedor desconhecida `{other}` (use idempotent, queryable ou opaque)"
                            ),
                            span: k.span,
                        })
                    }
                };
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
        Ok(ToolDecl { name, params, ret, effects, class, class_span, compensator, provider, latency, usd, requires })
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
        // `budget` é palavra contextual: só aqui, depois dos parâmetros do agente, ela abre o
        // orçamento. Em qualquer outro lugar é um nome comum (campo, variável, parâmetro).
        if !self.is_contextual("budget") {
            return Err(self.err(format!("esperado `budget`, encontrado {}", self.peek().describe())));
        }
        let bstart = self.bump().span;
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
        let mut handles = Vec::new();
        loop {
            if self.eat(&Tok::On) {
                let event = self.ident()?;
                let params = self.params()?;
                self.expect(Tok::Arrow)?;
                let ret = self.ty()?;
                let effects = self.effect_row()?;
                let body = self.block()?;
                handlers.push(Handler { event, params, ret, effects, body });
            } else if self.is_contextual("handle") {
                let start = self.bump().span;
                let effect = self.ident()?;
                let params = self.params()?;
                self.expect(Tok::Arrow)?;
                let ret = self.ty()?;
                let body = self.block()?;
                let span = start.to(body.span);
                handles.push(HandleClause { effect, params, ret, body, span });
            } else {
                break;
            }
        }
        self.expect(Tok::RBrace)?;
        Ok(AgentDecl { name, params, budget, budget_span: bstart.to(bend), handlers, handles })
    }

    /// `Int[lo..hi]`, `Int[lo..]`, `Int[..hi]` (intervalo inclusivo) e `Text[..n]` (até n caracteres).
    fn refined(&mut self, base: Ident) -> PResult<TypeExpr> {
        self.expect(Tok::LBrack)?;
        let bound = |p: &mut Self| -> PResult<Option<i64>> {
            let neg = p.eat(&Tok::Minus);
            match p.peek().clone() {
                Tok::Int(n) => {
                    p.bump();
                    Ok(Some(if neg { -n } else { n }))
                }
                _ if neg => Err(p.err("esperado inteiro após `-`")),
                _ => Ok(None),
            }
        };
        let lo = bound(self)?;
        self.expect(Tok::Dot)?;
        self.expect(Tok::Dot)?;
        let hi = bound(self)?;
        let end = self.expect(Tok::RBrack)?;
        let span = base.span.to(end);
        if lo.is_none() && hi.is_none() {
            return Err(ParseError { msg: "refinamento vazio: informe ao menos um limite".into(), span });
        }
        if let (Some(a), Some(b)) = (lo, hi) {
            if a > b {
                return Err(ParseError { msg: format!("intervalo vazio: {a} > {b}"), span });
            }
        }
        if base.name == "Text" && (lo.is_some() || hi.is_none_or(|h| h < 0)) {
            return Err(ParseError { msg: "texto aceita apenas teto de caracteres: `Text[..n]`".into(), span });
        }
        Ok(TypeExpr::Refined { base, lo, hi, span })
    }

    /// `List[T, a..b]` (inclusivo) ou `List[T, ..b]` (de 0 a b): toda lista tem teto.
    fn list_type(&mut self, base: Ident) -> PResult<TypeExpr> {
        self.expect(Tok::LBrack)?;
        let elem = self.ty()?;
        self.expect(Tok::Comma)?;
        let nat = |p: &mut Self| -> PResult<Option<u32>> {
            match p.peek().clone() {
                Tok::Int(n) if (0..=u32::MAX as i64).contains(&n) => {
                    p.bump();
                    Ok(Some(n as u32))
                }
                Tok::Int(_) | Tok::Minus => Err(p.err("limites de lista são inteiros não negativos")),
                _ => Ok(None),
            }
        };
        let lo = nat(self)?.unwrap_or(0);
        self.expect(Tok::Dot)?;
        self.expect(Tok::Dot)?;
        let Some(hi) = nat(self)? else {
            return Err(self.err("lista sem teto: `List[T, a..b]` exige o limite superior b"));
        };
        let end = self.expect(Tok::RBrack)?;
        let span = base.span.to(end);
        if lo > hi {
            return Err(ParseError { msg: format!("intervalo de tamanhos vazio: {lo} > {hi}"), span });
        }
        Ok(TypeExpr::List { elem: Box::new(elem), lo, hi, span })
    }

    fn ty(&mut self) -> PResult<TypeExpr> {
        let start = self.span();
        if self.is_contextual("own") {
            self.bump();
            let inner = self.ty()?;
            let span = start.to(inner.span());
            return Ok(TypeExpr::Own(Box::new(inner), span));
        }
        let mut base = if self.is_contextual("Cap") && *self.peek_at(1) == Tok::LBrack {
            self.bump();
            self.bump();
            let mut effs = vec![self.effect_name()?];
            while self.eat(&Tok::Plus) {
                effs.push(self.effect_name()?);
            }
            let end = self.expect(Tok::RBrack)?;
            TypeExpr::Cap(effs, start.to(end))
        } else {
            let id = self.ident()?;
            if *self.peek() == Tok::LBrack && id.name == "List" {
                self.list_type(id)?
            } else if *self.peek() == Tok::LBrack && (id.name == "Int" || id.name == "Text") {
                self.refined(id)?
            } else if *self.peek() == Tok::LBrack && id.name == "Pending" {
                // `Pending[k]`: o índice é a chave da operação em aberto; `_` = qualquer chave.
                self.bump();
                self.ident()?;
                self.expect(Tok::RBrack)?;
                TypeExpr::Named(id)
            } else {
                TypeExpr::Named(id)
            }
        };
        // v1.0-RC10: um ou dois rótulos `^ …`, integridade e/ou confidencialidade, em qualquer
        // ordem; no máximo um de cada espécie.
        let (mut seen_int, mut seen_conf) = (false, false);
        while self.eat(&Tok::Caret) {
            let l = self.ident()?;
            let span = start.to(l.span);
            base = match l.name.as_str() {
                "Untrusted" => Some(Label::Untrusted),
                "Endorsed" => Some(Label::Endorsed),
                "Trusted" => Some(Label::Trusted),
                _ => None,
            }
            .map(|lab| {
                if seen_int {
                    return Err(ParseError { msg: "rótulo de integridade repetido".into(), span: l.span });
                }
                seen_int = true;
                Ok(TypeExpr::Labeled(Box::new(base.clone()), lab, span))
            })
            .or_else(|| {
                match l.name.as_str() {
                    "Public" => Some(Conf::Public),
                    "Internal" => Some(Conf::Internal),
                    "Secret" => Some(Conf::Secret),
                    _ => None,
                }
                .map(|c| {
                    if seen_conf {
                        return Err(ParseError { msg: "rótulo de confidencialidade repetido".into(), span: l.span });
                    }
                    seen_conf = true;
                    Ok(TypeExpr::ConfLabeled(Box::new(base.clone()), c, span))
                })
            })
            .unwrap_or_else(|| {
                Err(ParseError {
                    msg: format!(
                        "rótulo desconhecido `{}` (integridade: Untrusted, Endorsed, Trusted; confidencialidade: Public, Internal, Secret)",
                        l.name
                    ),
                    span: l.span,
                })
            })?;
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
            Tok::Ident(name) if name == "atomic" && matches!(self.peek_at(1), Tok::Ident(r) if r == "recover") => {
                self.atomic()
            }
            Tok::Ident(name) if name == "await" => {
                self.bump();
                let cond = self.expr()?;
                if !self.is_contextual("within") {
                    return Err(self.err("`await` exige `within <segundos>`"));
                }
                self.bump();
                let within = self.number()?;
                let span = start.to(self.prev_span());
                Ok(Expr { kind: ExprKind::Await { cond: Box::new(cond), within }, span })
            }
            Tok::Ident(name) if name == "reconcile" && matches!(self.peek_at(1), Tok::Ident(_)) => {
                self.bump();
                let pending = self.ident()?;
                if !self.is_contextual("fuel") {
                    return Err(self.err("`reconcile p` exige `fuel <tentativas>`"));
                }
                self.bump();
                let fuel = match self.peek().clone() {
                    Tok::Int(n) if (1..=64).contains(&n) => {
                        self.bump();
                        n as u32
                    }
                    _ => return Err(self.err("`fuel` exige um inteiro entre 1 e 64")),
                };
                self.expect(Tok::Else)?;
                let els = self.block()?;
                let span = start.to(els.span);
                Ok(Expr { kind: ExprKind::Reconcile { pending, fuel, els }, span })
            }
            Tok::Ident(name)
                if name == "for"
                    && matches!(self.peek_at(1), Tok::Ident(_))
                    && matches!(self.peek_at(2), Tok::Ident(k) if k == "in") =>
            {
                self.bump();
                let var = self.ident()?;
                self.bump(); // in
                let saved = self.no_ctor;
                self.no_ctor = true;
                let iter = self.expr()?;
                self.no_ctor = saved;
                let body = self.block()?;
                let span = start.to(body.span);
                Ok(Expr { kind: ExprKind::For { var, iter: Box::new(iter), body }, span })
            }
            Tok::Ident(name)
                if (name == "all" || name == "any" || name == "sum")
                    && matches!(self.peek_at(1), Tok::Ident(_))
                    && matches!(self.peek_at(2), Tok::Ident(k) if k == "in") =>
            {
                let op = match name.as_str() {
                    "all" => QuantOp::All,
                    "any" => QuantOp::Any,
                    _ => QuantOp::Sum,
                };
                self.bump();
                let var = self.ident()?;
                self.bump(); // in
                let saved = self.no_ctor;
                self.no_ctor = true;
                let iter = self.concat_expr()?;
                self.no_ctor = saved;
                self.expect(Tok::FatArrow)?;
                let body = self.expr()?;
                let span = start.to(body.span);
                Ok(Expr { kind: ExprKind::Quant { op, var, iter: Box::new(iter), body: Box::new(body) }, span })
            }
            Tok::LBrack => {
                self.bump();
                let saved = self.no_ctor;
                self.no_ctor = false;
                let mut items = Vec::new();
                while *self.peek() != Tok::RBrack {
                    items.push(self.expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.no_ctor = saved;
                let end = self.expect(Tok::RBrack)?;
                Ok(Expr { kind: ExprKind::List(items), span: start.to(end) })
            }
            Tok::Ident(name) if name == "transfer" && matches!(self.peek_at(1), Tok::Ident(_)) => {
                self.bump();
                let p = self.ident()?;
                let span = start.to(p.span);
                Ok(Expr { kind: ExprKind::Transfer(p), span })
            }
            Tok::Ident(name) => {
                let id = self.ident()?;
                // `(` numa linha nova abre outra expressão, não chama o nome da linha anterior:
                // `let mine = order` seguido de `(sum … ) <= n` na linha de baixo são duas coisas.
                if *self.peek() == Tok::LParen && !self.toks[self.pos].nl_before {
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
                let mut arms: Vec<Arm> = Vec::new();
                let mut wildcard: Option<Arm> = None;
                while *self.peek() != Tok::RBrace {
                    if let Some(w) = &wildcard {
                        return Err(ParseError {
                            msg: "braço depois do curinga `_`: o curinga tem de ser o último braço do `match`".into(),
                            span: w.variant.span,
                        });
                    }
                    let variant = self.ident()?;
                    let mut binds = Vec::new();
                    if self.eat(&Tok::LBrace) {
                        while *self.peek() != Tok::RBrace {
                            // `..` (os demais campos) e `_` (campo ignorado) não ligam nada:
                            // os campos de um padrão já são opcionais.
                            if *self.peek() == Tok::Dot {
                                self.bump();
                                self.expect(Tok::Dot)?;
                            } else {
                                let b = self.ident()?;
                                if b.name != "_" {
                                    binds.push(b);
                                }
                            }
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                        self.expect(Tok::RBrace)?;
                    }
                    self.expect(Tok::FatArrow)?;
                    let body = self.expr()?;
                    let span = variant.span.to(body.span);
                    if variant.name == "_" {
                        if !binds.is_empty() {
                            return Err(ParseError { msg: "o curinga `_` não tem campos".into(), span: variant.span });
                        }
                        wildcard = Some(Arm { variant, binds, body, span });
                    } else {
                        arms.push(Arm { variant, binds, body, span });
                    }
                    // A vírgula entre braços é opcional: o próximo braço começa por um nome.
                    self.eat(&Tok::Comma);
                }
                if let Some(w) = wildcard {
                    match &self.adts {
                        // Primeira passada ou expressão isolada: o curinga fica como está.
                        None => arms.push(w),
                        Some(adts) => {
                            let Some(all) = arms.iter().find_map(|a| adts.get(&a.variant.name)) else {
                                return Err(ParseError {
                                    msg:
                                        "curinga `_` sem variante que identifique o tipo: nomeie ao menos uma variante \
                                          do tipo antes do `_`"
                                            .into(),
                                    span: w.variant.span,
                                });
                            };
                            for v in all {
                                if !arms.iter().any(|a| &a.variant.name == v) {
                                    arms.push(Arm {
                                        variant: Ident { name: v.clone(), span: w.variant.span },
                                        binds: Vec::new(),
                                        body: w.body.clone(),
                                        span: w.span,
                                    });
                                }
                            }
                        }
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
                // v1.0-RC7: `keeping campo by Política` (palavra contextual: depois da política
                // do `refine` só podem vir `keeping`, `retry` ou `else`)
                let mut keep = None;
                if matches!(self.peek(), Tok::Ident(w) if w == "keeping") {
                    self.bump();
                    // `all` só é o modo quando outro nome o segue: `keeping all by Q` ainda é o
                    // campo chamado `all`.
                    let all = matches!(self.peek(), Tok::Ident(w) if w == "all")
                        && matches!(self.toks[self.pos + 1].tok, Tok::Ident(_));
                    if all {
                        self.bump();
                    }
                    let field = self.ident()?;
                    self.expect(Tok::By)?;
                    let policy = self.ident()?;
                    // `as` é palavra contextual aqui (não é reservada)
                    let bind = if matches!(self.peek(), Tok::Ident(w) if w == "as") {
                        self.bump();
                        Some(self.ident()?)
                    } else {
                        None
                    };
                    keep = Some(Keep { field, policy, all, bind });
                }
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
                Ok(Expr { kind: ExprKind::Refine { draft: Box::new(draft), policy, retry, els, keep }, span })
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

    fn atomic(&mut self) -> PResult<Expr> {
        let start = self.bump().span; // atomic
        self.bump(); // recover
        let saved = self.no_ctor;
        self.no_ctor = true;
        let recover = self.expr()?;
        self.no_ctor = saved;
        let body = self.block()?;
        let mut end = body.span;
        let mut on_failure = None;
        let mut on_cf = None;
        while *self.peek() == Tok::On
            && matches!(self.peek_at(1), Tok::Ident(n) if n == "failure" || n == "compensation_failed")
        {
            self.bump();
            let which = self.ident()?;
            self.expect(Tok::LParen)?;
            let bind = self.ident()?;
            self.expect(Tok::RParen)?;
            let b = self.block()?;
            end = b.span;
            let slot = if which.name == "failure" { &mut on_failure } else { &mut on_cf };
            if slot.is_some() {
                return Err(ParseError { msg: format!("`on {}` repetido", which.name), span: which.span });
            }
            *slot = Some((bind, b));
        }
        Ok(Expr { kind: ExprKind::Atomic { recover: Box::new(recover), body, on_failure, on_cf }, span: start.to(end) })
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
    fn parses_bounded_lists_loops_and_quantifiers() {
        let src = r#"
module t
type Action = | Restart { deploy: Text[..32] }
model m { price_in = 1, id = "openai/gpt-oss-20b", provider = "groq", version = "2026-11-09", overhead = 640 }
policy P(p: List[Action, 1..3]) -> Bool = all a in p => true
"#;
        let m = parse_module(src).unwrap();
        let Item::Model(md) = &m.items[1] else { panic!() };
        assert_eq!(md.contract_label().as_deref(), Some("groq:openai/gpt-oss-20b@2026-11-09"));
        assert_eq!(md.prop("overhead"), Some(640.0));
        let Item::Policy(p) = &m.items[2] else { panic!() };
        assert_eq!(p.params[0].ty.sig(), "List[Action, 1..3]");
        assert!(matches!(p.body.kind, ExprKind::Quant { op: QuantOp::All, .. }));
        let e = parse_expr_str(r#"for x in [1, 2] { f(x) }"#).unwrap();
        let ExprKind::For { iter, .. } = &e.kind else { panic!("{e:?}") };
        assert!(matches!(&iter.kind, ExprKind::List(v) if v.len() == 2));
        assert!(
            parse_module(
                "module t
tool f(x: List[Int, 1..]) -> Unit ! {}"
            )
            .is_err(),
            "lista sem teto"
        );
        assert!(parse_module(
            "module t
tool f(x: List[Int, 3..1]) -> Unit ! {}"
        )
        .is_err());
    }

    #[test]
    fn parses_ctor_expression() {
        let e = parse_expr_str(r#"Scale { deploy: "api", to: 3 }"#).unwrap();
        assert!(matches!(e.kind, ExprKind::Ctor { .. }));
    }
}
