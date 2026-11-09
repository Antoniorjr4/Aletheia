use ath_syntax::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stratum {
    Parse,
    E0,
    E1,
    E2,
    E3,
    E5,
    E6,
}

impl Stratum {
    pub fn name(self) -> &'static str {
        match self {
            Stratum::Parse => "sintaxe",
            Stratum::E0 => "E0 estrutura",
            Stratum::E1 => "E1 multiplicidade",
            Stratum::E2 => "E2 efeitos",
            Stratum::E3 => "E3 integridade",
            Stratum::E5 => "E5 refinamentos",
            Stratum::E6 => "E6 orçamento",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Diag {
    pub code: String,
    pub stratum: Stratum,
    pub msg: String,
    pub span: Span,
    pub label: String,
    pub secondary: Vec<(Span, String)>,
    pub notes: Vec<String>,
    pub help: Option<String>,
}

impl Diag {
    pub fn new(stratum: Stratum, code: &str, msg: impl Into<String>, span: Span, label: impl Into<String>) -> Self {
        Diag {
            code: code.to_string(),
            stratum,
            msg: msg.into(),
            span,
            label: label.into(),
            secondary: Vec::new(),
            notes: Vec::new(),
            help: None,
        }
    }

    pub fn note(mut self, n: impl Into<String>) -> Self {
        self.notes.push(n.into());
        self
    }

    pub fn help(mut self, h: impl Into<String>) -> Self {
        self.help = Some(h.into());
        self
    }

    pub fn secondary(mut self, span: Span, label: impl Into<String>) -> Self {
        self.secondary.push((span, label.into()));
        self
    }
}
