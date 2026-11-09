//! Superfície textual de Aletheia Core 0: spans, lexer, AST e parser.
//!
//! O Codex (crate `ath-codex`) é a fonte da verdade; este crate só produz
//! a AST a partir do texto `.ath`.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod span;

pub use parser::{parse_expr_str, parse_module, ParseError};
pub use span::Span;

/// Estimativa de tokens usada pelo Marco Zero: 4 caracteres por token.
/// O tokenizer real de cada modelo entra quando `Model[...]` carregar o hash do tokenizer.
pub fn est_tokens(s: &str) -> u64 {
    let n = s.chars().count() as u64;
    n.div_ceil(4)
}
