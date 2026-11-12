use crate::span::Span;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    // palavras-chave
    Module,
    Type,
    Model,
    Authority,
    Witness,
    Tool,
    Policy,
    Agent,
    On,
    Let,
    Return,
    If,
    Else,
    Match,
    Infer,
    Refine,
    By,
    Retry,
    Quote,
    Commit,
    Budget,
    True,
    False,
    Not,
    // pontuação
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBrack,
    RBrack,
    Comma,
    Colon,
    Semi,
    Dot,
    Assign,
    EqEq,
    Ne,
    Le,
    Ge,
    Lt,
    Gt,
    Arrow,
    FatArrow,
    Bang,
    Caret,
    Pipe,
    Plus,
    PlusPlus,
    Minus,
    Star,
    Slash,
    AndAnd,
    OrOr,
    Eof,
}

impl Tok {
    pub fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Int(n) => format!("`{n}`"),
            Tok::Float(x) => format!("`{x}`"),
            Tok::Str(_) => "texto".to_string(),
            Tok::Eof => "fim do arquivo".to_string(),
            other => format!("`{}`", symbol(other)),
        }
    }
}

fn symbol(t: &Tok) -> &'static str {
    match t {
        Tok::Module => "module",
        Tok::Type => "type",
        Tok::Model => "model",
        Tok::Authority => "authority",
        Tok::Witness => "witness",
        Tok::Tool => "tool",
        Tok::Policy => "policy",
        Tok::Agent => "agent",
        Tok::On => "on",
        Tok::Let => "let",
        Tok::Return => "return",
        Tok::If => "if",
        Tok::Else => "else",
        Tok::Match => "match",
        Tok::Infer => "infer",
        Tok::Refine => "refine",
        Tok::By => "by",
        Tok::Retry => "retry",
        Tok::Quote => "quote",
        Tok::Commit => "commit",
        Tok::Budget => "budget",
        Tok::True => "true",
        Tok::False => "false",
        Tok::Not => "not",
        Tok::LBrace => "{",
        Tok::RBrace => "}",
        Tok::LParen => "(",
        Tok::RParen => ")",
        Tok::LBrack => "[",
        Tok::RBrack => "]",
        Tok::Comma => ",",
        Tok::Colon => ":",
        Tok::Semi => ";",
        Tok::Dot => ".",
        Tok::Assign => "=",
        Tok::EqEq => "==",
        Tok::Ne => "!=",
        Tok::Le => "<=",
        Tok::Ge => ">=",
        Tok::Lt => "<",
        Tok::Gt => ">",
        Tok::Arrow => "->",
        Tok::FatArrow => "=>",
        Tok::Bang => "!",
        Tok::Caret => "^",
        Tok::Pipe => "|",
        Tok::Plus => "+",
        Tok::PlusPlus => "++",
        Tok::Minus => "-",
        Tok::Star => "*",
        Tok::Slash => "/",
        Tok::AndAnd => "&&",
        Tok::OrOr => "||",
        _ => "?",
    }
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct LexError {
    pub msg: String,
    pub span: Span,
}

fn keyword(word: &str) -> Option<Tok> {
    Some(match word {
        "module" => Tok::Module,
        "type" => Tok::Type,
        "model" => Tok::Model,
        "authority" => Tok::Authority,
        "witness" => Tok::Witness,
        "tool" => Tok::Tool,
        "policy" => Tok::Policy,
        "agent" => Tok::Agent,
        "on" => Tok::On,
        "let" => Tok::Let,
        "return" => Tok::Return,
        "if" => Tok::If,
        "else" => Tok::Else,
        "match" => Tok::Match,
        "infer" => Tok::Infer,
        "refine" => Tok::Refine,
        "by" => Tok::By,
        "retry" => Tok::Retry,
        "quote" => Tok::Quote,
        "commit" => Tok::Commit,
        "budget" => Tok::Budget,
        "true" => Tok::True,
        "false" => Tok::False,
        "not" => Tok::Not,
        _ => return None,
    })
}

pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // comentário de linha: `--` até o fim da linha
        if c == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &src[start..i];
            let tok = keyword(word).unwrap_or_else(|| Tok::Ident(word.to_string()));
            out.push(Token { tok, span: Span::new(start, i) });
            continue;
        }
        if c.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
            let mut is_float = false;
            if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
                is_float = true;
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                let save = i;
                i += 1;
                if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
                    i += 1;
                }
                if i < bytes.len() && bytes[i].is_ascii_digit() {
                    is_float = true;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                } else {
                    i = save;
                }
            }
            let text: String = src[start..i].chars().filter(|&c| c != '_').collect();
            let span = Span::new(start, i);
            let tok = if is_float {
                Tok::Float(text.parse().map_err(|_| LexError { msg: "número inválido".into(), span })?)
            } else {
                Tok::Int(text.parse().map_err(|_| LexError { msg: "inteiro fora do intervalo".into(), span })?)
            };
            out.push(Token { tok, span });
            continue;
        }
        if c == b'"' {
            i += 1;
            let mut s = String::new();
            loop {
                if i >= bytes.len() {
                    return Err(LexError { msg: "texto sem aspas de fechamento".into(), span: Span::new(start, i) });
                }
                let ch = src[i..].chars().next().unwrap();
                if ch == '"' {
                    i += 1;
                    break;
                }
                if ch == '\n' {
                    return Err(LexError { msg: "texto sem aspas de fechamento".into(), span: Span::new(start, i) });
                }
                if ch == '\\' {
                    i += 1;
                    let Some(e) = src[i..].chars().next() else {
                        return Err(LexError { msg: "escape incompleto".into(), span: Span::new(start, i) });
                    };
                    match e {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        other => s.push(other),
                    }
                    i += e.len_utf8();
                    continue;
                }
                s.push(ch);
                i += ch.len_utf8();
            }
            out.push(Token { tok: Tok::Str(s), span: Span::new(start, i) });
            continue;
        }
        if !c.is_ascii() {
            let ch = src[i..].chars().next().unwrap();
            return Err(LexError {
                msg: format!("caractere inesperado `{ch}` (use a forma ASCII: `->`, `<=`, ...)"),
                span: Span::new(start, start + ch.len_utf8()),
            });
        }
        let next = if i + 1 < bytes.len() && bytes[i + 1].is_ascii() { bytes[i + 1] } else { 0 };
        let (tok, len) = match (c, next) {
            (b'-', b'>') => (Tok::Arrow, 2),
            (b'=', b'>') => (Tok::FatArrow, 2),
            (b'=', b'=') => (Tok::EqEq, 2),
            (b'!', b'=') => (Tok::Ne, 2),
            (b'<', b'=') => (Tok::Le, 2),
            (b'>', b'=') => (Tok::Ge, 2),
            (b'+', b'+') => (Tok::PlusPlus, 2),
            (b'&', b'&') => (Tok::AndAnd, 2),
            (b'|', b'|') => (Tok::OrOr, 2),
            (b'{', _) => (Tok::LBrace, 1),
            (b'}', _) => (Tok::RBrace, 1),
            (b'(', _) => (Tok::LParen, 1),
            (b')', _) => (Tok::RParen, 1),
            (b'[', _) => (Tok::LBrack, 1),
            (b']', _) => (Tok::RBrack, 1),
            (b',', _) => (Tok::Comma, 1),
            (b':', _) => (Tok::Colon, 1),
            (b';', _) => (Tok::Semi, 1),
            (b'.', _) => (Tok::Dot, 1),
            (b'=', _) => (Tok::Assign, 1),
            (b'<', _) => (Tok::Lt, 1),
            (b'>', _) => (Tok::Gt, 1),
            (b'!', _) => (Tok::Bang, 1),
            (b'^', _) => (Tok::Caret, 1),
            (b'|', _) => (Tok::Pipe, 1),
            (b'+', _) => (Tok::Plus, 1),
            (b'-', _) => (Tok::Minus, 1),
            (b'*', _) => (Tok::Star, 1),
            (b'/', _) => (Tok::Slash, 1),
            _ => {
                return Err(LexError {
                    msg: format!("caractere inesperado `{}`", c as char),
                    span: Span::new(start, start + 1),
                })
            }
        };
        i += len;
        out.push(Token { tok, span: Span::new(start, i) });
    }
    out.push(Token { tok: Tok::Eof, span: Span::new(src.len(), src.len()) });
    Ok(out)
}
