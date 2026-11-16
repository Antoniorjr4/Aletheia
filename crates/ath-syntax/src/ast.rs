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

/// Rótulo de confidencialidade (v1.0-RC10), dual da integridade: o quanto um valor pode ser
/// exposto. `Public ⊑ Internal ⊑ Secret`. A integridade diz de onde o dado veio (se pode
/// escolher uma ação); a confidencialidade diz para onde ele pode ir (se pode sair num canal).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Conf {
    Public,
    Internal,
    Secret,
}

impl Conf {
    /// Junção: o dado derivado é tão secreto quanto o mais secreto dos operandos.
    pub fn join(self, other: Conf) -> Conf {
        self.max(other)
    }

    pub fn name(self) -> &'static str {
        match self {
            Conf::Public => "Public",
            Conf::Internal => "Internal",
            Conf::Secret => "Secret",
        }
    }
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
    Authority(AuthorityDecl),
    Witness(WitnessDecl),
    Bind(BindDecl),
    Tool(ToolDecl),
    Policy(PolicyDecl),
    Agent(AgentDecl),
}

impl Item {
    pub fn ident(&self) -> &Ident {
        match self {
            Item::Type(d) => &d.name,
            Item::Model(d) => &d.name,
            Item::Authority(d) => &d.name,
            Item::Witness(d) => &d.name,
            Item::Bind(d) => &d.name,
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
            Item::Authority(_) => "authority",
            Item::Witness(_) => "witness",
            Item::Bind(_) => "bind",
            Item::Tool(_) => "tool",
            Item::Policy(_) => "policy",
            Item::Agent(_) => "agent",
        }
    }
}

/// `authority nome { … }`: quem pode conceder autoridade a este programa. Com uma autoridade
/// declarada, concessões, fotografia confiável e argumentos `Trusted` só valem assinados por
/// ela (`arx::sign`).
///
/// Duas formas:
///
/// * **textual** (RC2/RC3): `key = "…"`, `ops = "a, b"`, `max_age = N`, `ledger = "anchored"`;
/// * **tipada** (RC4): `ops = [a, b]` (operações resolvidas contra os contratos
///   `requires s.authorizes(…)`), `ledger = anchored`, `clock = testemunha` (a fonte de tempo
///   autenticado) e `outcomes = testemunha` (quem assina a evidência dos desfechos).
///
/// As duas formas não se misturam numa mesma autoridade, e o nível de garantia de cada uma é
/// explícito (`guarantee`): a compatibilidade nunca se confunde com a garantia mais forte.
#[derive(Clone, Debug)]
pub struct AuthorityDecl {
    pub name: Ident,
    /// **identidade**: a chave pública
    pub key: String,
    pub key_span: Span,
    /// **contexto**: as operações (`s.authorizes("op", …)`) que ela pode conceder; ausente:
    /// qualquer uma
    pub ops: Option<Vec<String>>,
    pub ops_span: Span,
    /// na forma tipada, cada operação com o seu lugar (para resolução e diagnósticos)
    pub op_idents: Vec<Ident>,
    /// a forma tipada (RC4)
    pub typed: bool,
    /// **validade**: a duração máxima, em segundos, de qualquer coisa que ela assine
    pub max_age: Option<u64>,
    /// `ledger = anchored`: o consumo das concessões dela exige âncora monotônica
    pub anchored: bool,
    /// a testemunha de tempo autenticado (forma tipada)
    pub clock: Option<Ident>,
    /// a testemunha que assina a evidência dos desfechos (forma tipada)
    pub outcomes: Option<Ident>,
}

/// O nível de garantia de uma autoridade declarada, do mais fraco ao mais forte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Guarantee {
    /// só `key` (RC2): identidade, sem contexto, prazo ou história
    Identity,
    /// forma textual (RC3): escopo em texto, prazo pelo relógio do executor
    Textual,
    /// forma tipada sem todos os vínculos do protocolo RC4
    Typed,
    /// forma tipada com escopo, prazo, registro ancorado, tempo autenticado e evidência de
    /// desfecho: o ciclo de vida inteiro é verificável (RC4)
    Temporal,
}

impl Guarantee {
    pub fn name(self) -> &'static str {
        match self {
            Guarantee::Identity => "identidade (RC2)",
            Guarantee::Textual => "textual (RC3)",
            Guarantee::Typed => "tipada, incompleta",
            Guarantee::Temporal => "temporal (RC4)",
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Guarantee::Identity => "identity",
            Guarantee::Textual => "textual",
            Guarantee::Typed => "typed",
            Guarantee::Temporal => "temporal",
        }
    }
}

impl AuthorityDecl {
    /// A autoridade declara vínculos: o que ela assina tem de estar ligado ao programa, à
    /// execução e a um intervalo de validade. Só com `key`, vale o formato do RC2.
    pub fn bound(&self) -> bool {
        self.ops.is_some() || self.max_age.is_some() || self.anchored || self.typed
    }

    /// O que falta para a garantia temporal (RC4).
    pub fn missing(&self) -> Vec<&'static str> {
        let mut m = Vec::new();
        if !self.typed {
            m.push("forma tipada (ops = [..], ledger = anchored)");
        }
        if self.ops.is_none() {
            m.push("escopo (ops)");
        }
        if self.max_age.is_none() {
            m.push("validade (max_age)");
        }
        if !self.anchored {
            m.push("história ancorada (ledger = anchored)");
        }
        if self.clock.is_none() {
            m.push("tempo autenticado (clock)");
        }
        if self.outcomes.is_none() {
            m.push("evidência de desfecho (outcomes)");
        }
        m
    }

    pub fn guarantee(&self) -> Guarantee {
        if self.typed {
            if self.missing().is_empty() {
                Guarantee::Temporal
            } else {
                Guarantee::Typed
            }
        } else if self.bound() {
            Guarantee::Textual
        } else {
            Guarantee::Identity
        }
    }
}

/// `bind nome { url = env("V") | "https://…", auth = secret("V"), status = "GET /rota",
/// ferramenta = "MÉTODO /rota -> campo", … }` (v1.0-RC5): liga ferramentas declaradas a uma API
/// HTTP, sem código de cola. O corpo da requisição são os parâmetros nomeados da ferramenta; a
/// chave vai em `Idempotency-Key`; `auth` nomeia um segredo do ambiente do executor, que o
/// programa não tem como ler.
#[derive(Clone, Debug)]
pub struct BindDecl {
    pub name: Ident,
    pub url: BindUrl,
    pub url_span: Span,
    /// o nome da variável de ambiente que guarda o segredo (nunca o valor)
    pub auth: Option<String>,
    /// a rota de consulta por chave, para os provedores `queryable`
    pub status: Option<Route>,
    pub routes: Vec<(Ident, Route)>,
    /// `ferramenta.key = "parâmetro"` (v1.0-RC6): o provedor ignora `Idempotency-Key`; a chave
    /// viaja como um marcador no fim deste parâmetro de texto
    pub keys: Vec<(Ident, String)>,
    /// `ferramenta.find = "GET /rota -> campo"` (v1.0-RC6): a consulta por chave; a operação é o
    /// elemento da lista devolvida que carrega o marcador
    pub finds: Vec<(Ident, Route)>,
    /// `ferramenta.settle = "N"` (v1.0-RC6): os segundos que a listagem do provedor pode levar
    /// para refletir uma escrita. Uma consulta que não acha a operação só conclui "não
    /// aplicada" depois de esperar esse tempo e consultar de novo.
    pub settles: Vec<(Ident, u32)>,
    /// `mcp = env("V") | "comando args…"` (v1.0-RC9): as ferramentas são as de um servidor MCP,
    /// iniciado pelo executor com este comando (stdio). Exclui `url`.
    pub mcp: Option<BindUrl>,
    /// `auth = secret("V") as "NOME"` (v1.0-RC9): num `bind` MCP, a variável do ambiente do
    /// processo do servidor que recebe o segredo
    pub secret_as: Option<String>,
    /// `arg.nome = env("V") | "texto"` (v1.0-RC9): um argumento de configuração do executor,
    /// usado nas chamadas como `@nome`
    pub args: Vec<(Ident, BindUrl)>,
    /// as chamadas MCP: `ferramenta = "nome_mcp(k: v, …) -> caminho"`
    pub calls: Vec<(Ident, McpCall)>,
    /// `ferramenta.find = "nome_mcp(k: v, …) -> lista[].caminho"`
    pub call_finds: Vec<(Ident, McpCall)>,
}

/// v1.0-RC9: o valor de um argumento numa chamada MCP.
#[derive(Clone, Debug, PartialEq)]
pub enum McpArg {
    /// um parâmetro de dados da ferramenta
    Param(String),
    /// `int(p)`: o texto do parâmetro, enviado como inteiro (se não for um, nada é enviado)
    IntOf(String),
    /// `@nome`: um `arg.nome` do `bind`
    Arg(String),
    /// `'texto'`
    Str(String),
    /// um inteiro literal
    Num(i64),
}

/// v1.0-RC9: uma chamada a uma ferramenta de um servidor MCP, e o que se lê do resultado.
#[derive(Clone, Debug, PartialEq)]
pub struct McpCall {
    pub tool: String,
    pub args: Vec<(String, McpArg)>,
    /// `-> a[].b`: o caminho até uma lista (a consulta por chave procura o elemento que carrega o
    /// marcador)
    pub list: Option<Vec<String>>,
    /// `-> a.b` (ou, depois de `[]`, o caminho dentro do elemento); vazio: o resultado inteiro
    /// (ou o elemento), e sem `->` a ferramenta não devolve valor que o programa leia
    pub path: Vec<String>,
    pub projected: bool,
}

impl McpCall {
    /// `nome(k: v, …) [-> caminho]`, com `v` um parâmetro, `int(p)`, `@arg`, `'texto'` ou um
    /// inteiro; o caminho é `a.b`, `a[]`, `a[].b`.
    pub fn parse(s: &str) -> Option<McpCall> {
        let word = |f: &str| !f.is_empty() && f.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let (call, proj) = match s.split_once("->") {
            Some((c, p)) => (c.trim(), Some(p.trim())),
            None => (s.trim(), None),
        };
        let (tool, rest) = call.split_once('(')?;
        let tool = tool.trim();
        if tool.is_empty() || !tool.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c)) {
            return None;
        }
        let inner = rest.trim_end().strip_suffix(')')?;
        let mut args = Vec::new();
        // separa por vírgulas fora de aspas simples
        let (mut parts, mut cur, mut quoted) = (Vec::new(), String::new(), false);
        for ch in inner.chars() {
            match ch {
                '\'' => {
                    quoted = !quoted;
                    cur.push(ch);
                }
                ',' if !quoted => parts.push(std::mem::take(&mut cur)),
                c => cur.push(c),
            }
        }
        if quoted {
            return None;
        }
        if !cur.trim().is_empty() || !parts.is_empty() {
            parts.push(cur);
        }
        for part in parts {
            let (k, v) = part.split_once(':')?;
            let (k, v) = (k.trim(), v.trim());
            if !word(k) || args.iter().any(|(n, _): &(String, McpArg)| n == k) {
                return None;
            }
            let val = if let Some(t) = v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
                if t.contains('\'') {
                    return None;
                }
                McpArg::Str(t.to_string())
            } else if let Some(a) = v.strip_prefix('@') {
                if !word(a) {
                    return None;
                }
                McpArg::Arg(a.to_string())
            } else if let Some(p) = v.strip_prefix("int(").and_then(|x| x.strip_suffix(')')) {
                if !word(p.trim()) {
                    return None;
                }
                McpArg::IntOf(p.trim().to_string())
            } else if let Ok(n) = v.parse::<i64>() {
                McpArg::Num(n)
            } else if word(v) && !v.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                McpArg::Param(v.to_string())
            } else {
                return None;
            };
            args.push((k.to_string(), val));
        }
        let (mut list, mut path) = (None, Vec::new());
        if let Some(p) = proj {
            let (before, after) = match p.split_once("[]") {
                Some((b, a)) => (b, Some(a)),
                None => (p, None),
            };
            let segs = |x: &str| -> Option<Vec<String>> {
                if x.is_empty() {
                    return Some(Vec::new());
                }
                let v: Vec<String> = x.split('.').map(String::from).collect();
                v.iter().all(|s| word(s)).then_some(v)
            };
            match after {
                Some(a) => {
                    let b = segs(before)?;
                    if b.is_empty() || a.contains("[]") {
                        return None;
                    }
                    let a = match a.strip_prefix('.') {
                        Some(x) => segs(x)?,
                        None if a.is_empty() => Vec::new(),
                        None => return None,
                    };
                    list = Some(b);
                    path = a;
                }
                None => {
                    path = segs(before)?;
                    if path.is_empty() {
                        return None;
                    }
                }
            }
        }
        Some(McpCall { tool: tool.to_string(), args, list, path, projected: proj.is_some() })
    }

    /// Os parâmetros da ferramenta que a chamada usa (`p` e `int(p)`), na ordem.
    pub fn params(&self) -> Vec<&str> {
        self.args
            .iter()
            .filter_map(|(_, a)| match a {
                McpArg::Param(p) | McpArg::IntOf(p) => Some(p.as_str()),
                _ => None,
            })
            .collect()
    }

    /// O caminho como texto: `a.b`, `a[].b`.
    pub fn shown_path(&self) -> String {
        match &self.list {
            Some(l) if self.path.is_empty() => format!("{}[]", l.join(".")),
            Some(l) => format!("{}[].{}", l.join("."), self.path.join(".")),
            None => self.path.join("."),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BindUrl {
    Lit(String),
    Env(String),
}

/// A autoridade (`host[:porta]`) de uma url http(s), ou `None` se a url não é http(s), não tem
/// host, ou carrega credenciais (`usuário@`): uma url de `bind` nunca as carrega.
pub fn url_authority(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))?;
    let auth = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (!auth.is_empty() && !auth.contains('@')).then_some(auth)
}

/// A url aponta para **o próprio host**? Só `localhost` e os endereços de loopback (`127.0.0.0/8`,
/// `[::1]`). Qualquer outra coisa (outro literal IPv6, um endereço mapeado, um nome que começa
/// com `localhost`, uma url com credenciais ou malformada) não é: na dúvida, é outro host.
pub fn loopback_url(url: &str) -> bool {
    let Some(auth) = url_authority(url) else { return false };
    let port_ok = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
    let host = if let Some(r) = auth.strip_prefix('[') {
        match r.split_once(']') {
            Some((h, "")) => h,
            Some((h, tail)) if tail.strip_prefix(':').is_some_and(port_ok) => h,
            _ => return false,
        }
    } else {
        match auth.rsplit_once(':') {
            Some((h, p)) if port_ok(p) => h,
            Some(_) => return false,
            None => auth,
        }
    };
    host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// `keeping campo by Política` num `refine` (v1.0-RC7).
#[derive(Clone, Debug)]
pub struct Keep {
    pub field: Ident,
    pub policy: Ident,
    /// `… as nome` (v1.0-RC8): liga, depois do `let`, a lista do que foi deixado de fora, com o
    /// rótulo `Untrusted` (são dados do modelo que a política recusou)
    pub bind: Option<Ident>,
}

/// `"MÉTODO /caminho -> campo"`: o campo da resposta que é o resultado (opcional).
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub field: Option<String>,
}

impl Route {
    pub fn parse(s: &str) -> Option<Route> {
        let (call, field) = match s.split_once("->") {
            Some((c, f)) => (c.trim(), Some(f.trim().to_string())),
            None => (s.trim(), None),
        };
        let (method, path) = call.split_once(' ')?;
        let path = path.trim();
        let word = |f: &str| !f.is_empty() && f.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        // v1.0-RC6: `{parâmetro}` num segmento do caminho; uma query literal depois de `?`;
        // `-> [].campo` projeta o campo de cada elemento de uma lista
        let (segs, query) = path.split_once('?').unwrap_or((path, ""));
        let seg_ok = |s: &str| match s.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
            Some(name) => word(name),
            None => s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)),
        };
        let ok = matches!(method, "GET" | "POST" | "PUT" | "PATCH")
            && segs.starts_with('/')
            && segs.split('/').skip(1).all(|s| !s.is_empty() && seg_ok(s))
            && query.chars().all(|c| c.is_ascii_alphanumeric() || "=&_-.".contains(c))
            && field.as_deref().is_none_or(|f| word(f.strip_prefix("[].").unwrap_or(f)));
        ok.then(|| Route { method: method.to_string(), path: path.to_string(), field })
    }

    /// Os parâmetros da ferramenta que vão no caminho (`{nome}`), na ordem.
    pub fn path_params(&self) -> Vec<&str> {
        let segs = self.path.split('?').next().unwrap_or_default();
        segs.split('/').filter_map(|s| s.strip_prefix('{').and_then(|x| x.strip_suffix('}'))).collect()
    }
}

/// `witness nome { key = "ed25519:…", tolerance = N }`: uma testemunha externa cuja assinatura
/// o programa aceita como evidência. Referida por uma autoridade como `clock` (o tempo
/// autenticado; `tolerance` é o atraso máximo admitido, em segundos, entre o pedido e a
/// resposta) ou como `outcomes` (os recibos do provedor).
#[derive(Clone, Debug)]
pub struct WitnessDecl {
    pub name: Ident,
    pub key: String,
    pub key_span: Span,
    pub tolerance: Option<u64>,
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
    /// Identidade versionada do contrato Δ: `id`, `provider`, `version` (valores textuais).
    pub meta: Vec<(Ident, String)>,
}

impl ModelDecl {
    pub fn prop(&self, key: &str) -> Option<f64> {
        self.props.iter().find(|p| p.key.name == key).map(|p| p.value)
    }

    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta.iter().find(|(k, _)| k.name == key).map(|(_, v)| v.as_str())
    }

    /// `provider:id@version` (partes ausentes omitidas); `None` se o contrato não declara identidade.
    pub fn contract_label(&self) -> Option<String> {
        let id = self.meta("id")?;
        let mut s = String::new();
        if let Some(p) = self.meta("provider") {
            s.push_str(p);
            s.push(':');
        }
        s.push_str(id);
        if let Some(v) = self.meta("version") {
            s.push('@');
            s.push_str(v);
        }
        Some(s)
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
    /// `requires <expr>`: o contrato de autorização da ferramenta, uma expressão `Bool` pura
    /// sobre os parâmetros e o estado confiável, conferida pelo runtime antes do efeito
    /// (monitor de referência na fronteira do efeito), qualquer que seja a política que
    /// endossou os argumentos.
    pub requires: Option<Expr>,
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
    /// v1.0-RC10: rótulo de confidencialidade.
    ConfLabeled(Box<TypeExpr>, Conf, Span),
    /// Lista delimitada `List[T, a..b]`: de `a` a `b` elementos (inclusivo).
    List {
        elem: Box<TypeExpr>,
        lo: u32,
        hi: u32,
        span: Span,
    },
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Named(id) => id.span,
            TypeExpr::Refined { span, .. }
            | TypeExpr::List { span, .. }
            | TypeExpr::Cap(_, span)
            | TypeExpr::Labeled(_, _, span)
            | TypeExpr::ConfLabeled(_, _, span)
            | TypeExpr::Own(_, span) => *span,
        }
    }

    /// Remove o rótulo de integridade e a marca `own`, se houver.
    pub fn unlabeled(&self) -> &TypeExpr {
        match self {
            TypeExpr::Labeled(inner, ..) | TypeExpr::ConfLabeled(inner, ..) | TypeExpr::Own(inner, _) => {
                inner.unlabeled()
            }
            other => other,
        }
    }

    /// O tipo foi declarado com posse linear (`own T`)?
    pub fn is_own(&self) -> bool {
        match self {
            TypeExpr::Own(..) => true,
            TypeExpr::Labeled(inner, ..) | TypeExpr::ConfLabeled(inner, ..) => inner.is_own(),
            _ => false,
        }
    }

    /// Nome do tipo base (`Int`, `Text`, nome do ADT…), sem rótulo nem refinamento.
    pub fn base_name(&self) -> &str {
        match self.unlabeled() {
            TypeExpr::Named(id) => &id.name,
            TypeExpr::Refined { base, .. } => &base.name,
            TypeExpr::Cap(..) => "Cap",
            TypeExpr::List { .. } => "List",
            TypeExpr::Labeled(..) | TypeExpr::ConfLabeled(..) | TypeExpr::Own(..) => unreachable!(),
        }
    }

    /// Assinatura textual completa: `Int[2..64]`, `List[Action, 1..3]`, `Text ^ Untrusted`.
    pub fn sig(&self) -> String {
        let b = |x: &Option<i64>| x.map(|n| n.to_string()).unwrap_or_default();
        match self {
            TypeExpr::Named(id) => id.name.clone(),
            TypeExpr::Refined { base, lo, hi, .. } => format!("{}[{}..{}]", base.name, b(lo), b(hi)),
            TypeExpr::Cap(effs, _) => {
                let mut n: Vec<&str> = effs.iter().map(|e| e.name.as_str()).collect();
                n.sort();
                format!("Cap[{}]", n.join(" + "))
            }
            TypeExpr::Labeled(inner, l, _) => format!("{} ^ {}", inner.sig(), l.name()),
            TypeExpr::ConfLabeled(inner, c, _) => format!("{} ^ {}", inner.sig(), c.name()),
            TypeExpr::Own(inner, _) => format!("own {}", inner.sig()),
            TypeExpr::List { elem, lo, hi, .. } => format!("List[{}, {lo}..{hi}]", elem.sig()),
        }
    }

    /// Nome do tipo nos prompts e mensagens: o nome base, ou a assinatura inteira de uma lista.
    pub fn display(&self) -> String {
        match self.unlabeled() {
            l @ TypeExpr::List { .. } => l.sig(),
            other => other.base_name().to_string(),
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

impl Expr {
    /// `s.managed()` ou `s.telemetry()`: um campo do prompt que vem da fotografia confiável do
    /// host. Os dois backends o rotulam como `Trusted` no payload do modelo.
    pub fn is_snapshot_field(&self) -> bool {
        self.inventory_anchor().is_some()
            || matches!(&self.kind, ExprKind::Method { recv, method, args }
                if args.is_empty()
                    && matches!(&recv.kind, ExprKind::Var(n) if n == "s")
                    && method.name == "telemetry")
    }

    /// O nome do inventário confiável que este campo de prompt entrega ao modelo:
    /// `s.managed()` é o inventário `managed`; `s.inventory("k")` é o inventário `k`.
    pub fn inventory_anchor(&self) -> Option<&str> {
        let ExprKind::Method { recv, method, args } = &self.kind else { return None };
        if !matches!(&recv.kind, ExprKind::Var(n) if n == "s") {
            return None;
        }
        match (method.name.as_str(), args.as_slice()) {
            ("managed", []) => Some("managed"),
            ("inventory", [Expr { kind: ExprKind::Str(k), .. }]) => Some(k.as_str()),
            _ => None,
        }
    }
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
        /// `keeping campo by Política` (v1.0-RC7): antes de a política do plano decidir, os
        /// elementos da lista `campo` que `Política` recusa são deixados de fora e registrados
        keep: Option<Keep>,
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
    /// Lista literal `[e1, …, en]`.
    List(Vec<Expr>),
    /// `for x in xs { … }`: itera uma lista delimitada; o custo é multiplicado pelo teto da lista.
    For {
        var: Ident,
        iter: Box<Expr>,
        body: Block,
    },
    /// `all x in xs => cond` / `any x in xs => cond`: quantificador limitado (puro em políticas).
    Quant {
        all: bool,
        var: Ident,
        iter: Box<Expr>,
        body: Box<Expr>,
    },
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

#[cfg(test)]
mod mcp_call_tests {
    use super::*;

    #[test]
    fn parses_the_calls_of_the_triage() {
        let c = McpCall::parse(
            "add_issue_comment(owner: @owner, repo: @repo, issue_number: int(issue), body: body) -> html_url",
        )
        .unwrap();
        assert_eq!(c.tool, "add_issue_comment");
        assert_eq!(c.args[2], ("issue_number".into(), McpArg::IntOf("issue".into())));
        assert_eq!(
            (c.list.clone(), c.path.clone(), c.params()),
            (None, vec!["html_url".to_string()], vec!["issue", "body"])
        );
        let f = McpCall::parse(
            "issue_read(method: 'get_comments', perPage: 100, issue_number: int(issue)) -> comments[].html_url",
        )
        .unwrap();
        assert_eq!((f.list.unwrap(), f.path), (vec!["comments".to_string()], vec!["html_url".to_string()]));
        assert_eq!(f.args[0].1, McpArg::Str("get_comments".into()));
        assert_eq!(f.args[1].1, McpArg::Num(100));
        let l = McpCall::parse("list_issues(state: 'OPEN') -> issues[]").unwrap();
        assert!(l.list.is_some() && l.path.is_empty() && l.projected);
        let u = McpCall::parse("issue_write(method: 'create', title: title)").unwrap();
        assert!(!u.projected && u.path.is_empty());
        assert_eq!(McpCall::parse("x(texto: 'a, b')").unwrap().args[0].1, McpArg::Str("a, b".into()));
    }

    #[test]
    fn refuses_what_is_not_a_call() {
        for s in [
            "GET /issues",
            "x(a: b",
            "x(a b)",
            "x(a: 'b)",
            "x(a: b, a: c)",
            "x(a: @)",
            "x(a: int())",
            "x() -> a[].b[]",
            "x() -> []",
            "x() -> a..b",
            "x y(a: b)",
            "(a: b)",
            "x(a: 1b)",
        ] {
            assert!(McpCall::parse(s).is_none(), "aceitou {s:?}");
        }
    }
}
