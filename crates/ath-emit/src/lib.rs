//! Emissor Python de Aletheia Core 0.
//!
//! Não é tradução de sintaxe: o código emitido é uma **segunda realização da semântica
//! operacional do ARX** sobre o `aletheia_runtime` (diretório `runtimes/python`). O programa
//! emitido produz o mesmo journal que o ARX — mesmas entradas, mesma ordem, mesmos bytes, mesma
//! cabeça de hash —, e a suíte diferencial do repositório verifica isso traço a traço.
//!
//! Esquema de compilação:
//!
//! - **forma normal administrativa (ANF)**: toda subexpressão com efeito vira uma instrução,
//!   na ordem de avaliação do ARX; o que sobra numa expressão é um átomo (variável ou literal);
//! - **nomes únicos**: cada `let`, parâmetro e variável de laço ganha um nome Python próprio,
//!   então o sombreamento de Aletheia nunca vira reatribuição em Python;
//! - **`return` como exceção** (`Ret`), capturada pelo runtime na fronteira do handler: a mesma
//!   propagação do `Unwind::Ret` do ARX, inclusive através de sagas e tratadores;
//! - **sagas, `await` e sondas de recuperação** viram funções aninhadas entregues ao
//!   coordenador transacional do runtime (`R.atomic`, `R.await_`), que repete exatamente a
//!   ordem de compensações, journal e relógio do ARX;
//! - **metadados estáticos** (ferramentas, contratos Δ com hash do Codex, formas de saída com
//!   seus tetos de bytes, parâmetros de agentes) são calculados aqui, em Rust, e emitidos como
//!   dados: o runtime não reimplementa o compilador de gramática.

use ath_check::CheckResult;
use ath_codex::Codex;
use ath_grammar::{max_bytes, shape_of, to_gbnf, to_json_schema_with, Dialect, Shape};
use ath_syntax::ast::*;
use serde_json::{json, Value as Json};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

/// Emite o módulo Python de um programa já aceito por `athc check`.
pub fn python(m: &Module, _check: &CheckResult, codex: &Codex, source: &str) -> Result<String, String> {
    let mut g = Gen::new(m);
    let mut funcs: Vec<String> = Vec::new();

    for item in &m.items {
        if let Item::Policy(p) = item {
            g.reset();
            g.env.push(Vec::new());
            let params: Vec<String> = p.params.iter().map(|x| g.bind(&x.name.name)).collect();
            g.line(format!("def policy_{}(R, {}):", ident(&p.name.name), params.join(", ")));
            g.ind += 1;
            let a = g.expr(&p.body)?;
            g.line(format!("return {a}"));
            g.ind -= 1;
            g.env.pop();
            funcs.push(g.take());
        }
    }

    // Os contratos `requires`: uma função por ferramenta, ligada à entrada da ferramenta.
    for item in &m.items {
        if let Item::Tool(t) = item {
            let Some(req) = &t.requires else { continue };
            g.reset();
            g.env.push(Vec::new());
            let params: Vec<String> = t.params.iter().map(|x| g.bind(&x.name.name)).collect();
            g.line(format!("def contract_{}(R, {}):", ident(&t.name.name), params.join(", ")));
            g.ind += 1;
            let a = g.expr(req)?;
            g.line(format!("return {a}"));
            g.ind -= 1;
            g.env.pop();
            g.line(format!("TOOLS[{}][\"contract\"] = contract_{}", py_str(&t.name.name), ident(&t.name.name)));
            funcs.push(g.take());
        }
    }

    let mut agents = String::from("AGENTS = {\n");
    for item in &m.items {
        let Item::Agent(a) = item else { continue };
        let caps: Vec<(String, Vec<String>)> = a
            .params
            .iter()
            .map(|p| {
                let mut effs: Vec<String> = match p.ty.unlabeled() {
                    TypeExpr::Cap(e, _) => e.iter().map(|x| x.name.clone()).collect(),
                    _ => Vec::new(),
                };
                effs.sort();
                effs.dedup();
                (p.name.name.clone(), effs)
            })
            .collect();
        let mut handlers = Vec::new();
        for h in &a.handlers {
            g.reset();
            g.env.push(Vec::new());
            let mut names: Vec<String> = a.params.iter().map(|p| g.bind(&p.name.name)).collect();
            names.extend(h.params.iter().map(|p| g.bind(&p.name.name)));
            let fname = format!("on_{}_{}", ident(&a.name.name), ident(&h.event.name));
            g.line(format!("def {fname}(R, {}):", names.join(", ")));
            g.ind += 1;
            let r = g.block(&h.body)?;
            g.line(format!("return {r}"));
            g.ind -= 1;
            g.env.pop();
            funcs.push(g.take());
            let params: Vec<String> = h
                .params
                .iter()
                .map(|p| {
                    let shape = shape_of(m, &p.ty).ok().map(|s| py(&shape_json(&s))).unwrap_or_else(|| "None".into());
                    format!("({}, {shape}, {})", py_str(&p.name.name), py_str(p.ty.base_name()))
                })
                .collect();
            handlers.push(format!("{}: ({fname}, [{}])", py_str(&h.event.name), params.join(", ")));
        }
        let mut handles = Vec::new();
        for h in &a.handles {
            g.reset();
            g.env.push(Vec::new());
            let mut names: Vec<String> = a.params.iter().map(|p| g.bind(&p.name.name)).collect();
            names.extend(h.params.iter().map(|p| g.bind(&p.name.name)));
            let fname = format!("handle_{}_{}", ident(&a.name.name), ident(&h.effect.name));
            g.line(format!("def {fname}(R, {}):", names.join(", ")));
            g.ind += 1;
            let r = g.block(&h.body)?;
            g.line(format!("return {r}"));
            g.ind -= 1;
            g.env.pop();
            funcs.push(g.take());
            handles.push(format!("{}: {fname}", py_str(&h.effect.name)));
        }
        let budget: serde_json::Map<String, Json> =
            a.budget.iter().map(|b| (b.key.name.clone(), json!(b.value))).collect();
        let params: Vec<String> = caps.iter().map(|(n, e)| format!("({}, {})", py_str(n), py(&json!(e)))).collect();
        let _ = writeln!(
            agents,
            "    {}: {{\n        \"params\": [{}],\n        \"budget\": {},\n        \"handlers\": {{{}}},\n        \"handles\": {{{}}},\n    }},",
            py_str(&a.name.name),
            params.join(", "),
            py(&Json::Object(budget)),
            handlers.join(", "),
            handles.join(", ")
        );
    }
    agents.push_str("}\n");

    let mut tools = serde_json::Map::new();
    let mut models = serde_json::Map::new();
    for item in &m.items {
        match item {
            Item::Tool(t) => {
                tools.insert(
                    t.name.name.clone(),
                    json!({
                        "class": t.class.name(),
                        "effects": t.effects.iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
                        "compensator": t.compensator.as_ref().map(|c| c.name.clone()),
                        "provider": t.provider.name(),
                        "latency": t.latency,
                        "usd": t.usd,
                        // a forma dos argumentos, para a recuperação ler as intenções do WAL
                        "params": t.params.iter().map(|p| match p.ty.unlabeled() {
                            ath_syntax::ast::TypeExpr::Cap(..) => "cap",
                            t if t.base_name() == "IdemKey" => "key",
                            _ => "val",
                        }).collect::<Vec<_>>(),
                    }),
                );
            }
            Item::Model(d) => {
                let props: serde_json::Map<String, Json> =
                    d.props.iter().map(|p| (p.key.name.clone(), json!(p.value))).collect();
                let meta: serde_json::Map<String, Json> =
                    d.meta.iter().map(|(k, v)| (k.name.clone(), json!(v))).collect();
                models.insert(
                    d.name.name.clone(),
                    json!({ "props": props, "meta": meta, "contract_hash": codex.hash_of(&d.name.name) }),
                );
            }
            _ => {}
        }
    }

    let mut out = String::new();
    let _ = writeln!(out, "# Gerado por `athc emit --target py` a partir de {source}. Não edite à mão.");
    let _ = writeln!(out, "#");
    let _ = writeln!(out, "# Segunda realização da semântica de Aletheia Core 0: roda sobre o aletheia_runtime e");
    let _ =
        writeln!(out, "# produz o mesmo journal que o ARX (mesmos bytes, mesma cabeça de hash). Codex #{}", codex.root);
    let _ = writeln!(out);
    let _ = writeln!(out, "from aletheia_runtime.runtime import Program, Ret, RtErr");
    let _ = writeln!(out, "from aletheia_runtime.values import Adt");
    let _ = writeln!(out);
    let _ = writeln!(out, "MODULE = {}", py_str(&m.name));
    let _ = writeln!(out, "CODEX_ROOT = {}", py_str(&codex.root));
    let _ = writeln!(out, "TOOLS = {}", py(&Json::Object(tools)));
    let _ = writeln!(out, "MODELS = {}", py(&Json::Object(models)));
    for (name, shape, bytes, schemas, gbnf) in &g.shapes {
        let _ = writeln!(out, "{name} = {}", py(shape));
        let _ = writeln!(out, "{name}_BYTES = {}", bytes.map(|b| b.to_string()).unwrap_or_else(|| "None".into()));
        // a restrição que o provedor remoto recebe, calculada pelo compilador de gramática
        let _ = writeln!(out, "{name}_SCHEMA = {}", py(schemas));
        let _ = writeln!(out, "{name}_GBNF = {}", py_str(gbnf));
    }
    for f in funcs {
        let _ = writeln!(out, "\n");
        out.push_str(&f);
    }
    let _ = writeln!(out, "\n");
    out.push_str(&agents);
    // as autoridades declaradas e os tratadores com argumento `Trusted` (só quando há autoridade:
    // um módulo sem ela é emitido como antes)
    let authorities: serde_json::Map<String, Json> = m
        .items
        .iter()
        .filter_map(|i| {
            let Item::Authority(d) = i else { return None };
            // só com `key`, a chave como texto (como no RC2); com vínculos, o registro inteiro
            let v = if d.bound() {
                json!({ "key": d.key, "ops": d.ops, "max_age": d.max_age, "anchored": d.anchored, "bound": true })
            } else {
                json!(d.key)
            };
            Some((d.name.name.clone(), v))
        })
        .collect();
    if authorities.is_empty() {
        let _ = writeln!(out, "\nPROGRAM = Program(MODULE, CODEX_ROOT, TOOLS, MODELS, AGENTS)");
    } else {
        let mut trusted: Vec<Json> = Vec::new();
        for i in &m.items {
            if let Item::Agent(a) = i {
                for h in &a.handlers {
                    let t = |p: &ath_syntax::ast::Param| {
                        matches!(&p.ty, ath_syntax::ast::TypeExpr::Labeled(_, ath_syntax::ast::Label::Trusted, _))
                    };
                    if h.params.iter().any(t) {
                        trusted.push(json!(format!("{}.{}", a.name.name, h.event.name)));
                    }
                }
            }
        }
        let _ = writeln!(out, "\nAUTHORITIES = {}", py(&Json::Object(authorities)));
        let _ = writeln!(out, "TRUSTED_ARGS = {}", py(&Json::Array(trusted)));
        let _ =
            writeln!(out, "PROGRAM = Program(MODULE, CODEX_ROOT, TOOLS, MODELS, AGENTS, AUTHORITIES, TRUSTED_ARGS)");
    }
    Ok(out)
}

/// Forma de saída como dado para o runtime (`aletheia_runtime.shape`).
pub fn shape_json(s: &Shape) -> Json {
    match s {
        Shape::Int { min, max } => json!({ "t": "int", "min": min, "max": max }),
        Shape::Bool => json!({ "t": "bool" }),
        Shape::Text { max_chars } => json!({ "t": "text", "max": max_chars }),
        Shape::List { elem, min, max } => json!({ "t": "list", "elem": shape_json(elem), "min": min, "max": max }),
        Shape::Adt { name, variants } => json!({
            "t": "adt",
            "name": name,
            "variants": variants.iter().map(|v| json!({
                "name": v.name,
                "fields": v.fields.iter().map(|(f, fs)| json!([f, shape_json(fs)])).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),
    }
}

fn ident(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

/// Literal de texto Python (UTF-8 cru; escapa aspas, barra e caracteres de controle).
fn py_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => {
                let _ = write!(o, "\\U{:08x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Literal Python de um valor JSON (floats com a grafia exata do Rust, que o Python relê igual).
fn py(j: &Json) -> String {
    match j {
        Json::Null => "None".into(),
        Json::Bool(b) => if *b { "True" } else { "False" }.into(),
        Json::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) if !n.is_f64() => i.to_string(),
            (_, Some(u), _) if !n.is_f64() => u.to_string(),
            (_, _, Some(f)) => format!("{f:?}"),
            _ => "None".into(),
        },
        Json::String(s) => py_str(s),
        Json::Array(a) => format!("[{}]", a.iter().map(py).collect::<Vec<_>>().join(", ")),
        Json::Object(o) => {
            format!("{{{}}}", o.iter().map(|(k, v)| format!("{}: {}", py_str(k), py(v))).collect::<Vec<_>>().join(", "))
        }
    }
}

struct Gen<'m> {
    m: &'m Module,
    /// as obrigações `requires` levantadas para os `refine` e os sítios descarregados (M-I)
    ob: ath_syntax::obligations::Obligations,
    /// o sítio da expressão em emissão (para marcar a chamada descarregada)
    site: Option<ath_syntax::Span>,
    tools: HashSet<String>,
    policies: HashSet<String>,
    variants: HashMap<String, String>,
    lines: Vec<String>,
    ind: usize,
    env: Vec<Vec<(String, String)>>,
    n: usize,
    shapes: Vec<(String, Json, Option<u64>, Json, String)>,
}

impl<'m> Gen<'m> {
    fn new(m: &'m Module) -> Self {
        let mut g = Gen {
            ob: ath_syntax::obligations::analyze(m),
            site: None,
            m,
            tools: HashSet::new(),
            policies: HashSet::new(),
            variants: HashMap::new(),
            lines: Vec::new(),
            ind: 0,
            env: Vec::new(),
            n: 0,
            shapes: Vec::new(),
        };
        for item in &m.items {
            match item {
                Item::Tool(t) => {
                    g.tools.insert(t.name.name.clone());
                }
                Item::Policy(p) => {
                    g.policies.insert(p.name.name.clone());
                }
                Item::Type(t) => {
                    for v in &t.variants {
                        g.variants.insert(v.name.name.clone(), t.name.name.clone());
                    }
                }
                _ => {}
            }
        }
        g
    }

    fn reset(&mut self) {
        self.lines.clear();
        self.ind = 0;
        self.env.clear();
    }

    fn take(&mut self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        self.lines.clear();
        s
    }

    fn line(&mut self, s: impl AsRef<str>) {
        self.lines.push(format!("{}{}", "    ".repeat(self.ind), s.as_ref()));
    }

    fn fresh(&mut self, p: &str) -> String {
        self.n += 1;
        format!("_{p}{}", self.n)
    }

    fn bind(&mut self, name: &str) -> String {
        self.n += 1;
        let py = format!("v_{}_{}", ident(name), self.n);
        self.env.last_mut().expect("escopo").push((name.to_string(), py.clone()));
        py
    }

    fn lookup(&self, name: &str) -> Result<String, String> {
        for f in self.env.iter().rev() {
            for (n, p) in f.iter().rev() {
                if n == name {
                    return Ok(p.clone());
                }
            }
        }
        Err(format!("variável `{name}` não vinculada"))
    }

    /// Corpo de bloco Python não vazio (instruções compostas exigem ao menos uma).
    fn indented<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        let start = self.lines.len();
        self.ind += 1;
        let r = f(self);
        if self.lines.len() == start {
            self.line("pass");
        }
        self.ind -= 1;
        r
    }

    fn block(&mut self, b: &Block) -> Result<String, String> {
        self.env.push(Vec::new());
        let r = self.block_inner(b);
        self.env.pop();
        r
    }

    fn block_inner(&mut self, b: &Block) -> Result<String, String> {
        for s in &b.stmts {
            match s {
                Stmt::Let { name, value, .. } => {
                    let a = self.expr(value)?;
                    let p = self.bind(&name.name);
                    self.line(format!("{p} = {a}"));
                }
                Stmt::Expr(e) => {
                    self.expr(e)?;
                }
            }
        }
        match &b.tail {
            Some(t) => self.expr(t),
            None => Ok("None".into()),
        }
    }

    /// Função aninhada sem parâmetros que devolve o valor de uma expressão (sondas, condições).
    fn thunk(&mut self, prefix: &str, e: &Expr) -> Result<String, String> {
        let name = self.fresh(prefix);
        self.line(format!("def {name}():"));
        self.indented(|g| {
            let a = g.expr(e)?;
            g.line(format!("return {a}"));
            Ok(())
        })?;
        Ok(name)
    }

    fn block_fn(&mut self, prefix: &str, bind: Option<&Ident>, b: &Block) -> Result<String, String> {
        let name = self.fresh(prefix);
        self.env.push(Vec::new());
        let param = bind.map(|i| self.bind(&i.name)).unwrap_or_default();
        self.line(format!("def {name}({param}):"));
        let r = self.indented(|g| {
            let a = g.block(b)?;
            g.line(format!("return {a}"));
            Ok(())
        });
        self.env.pop();
        r?;
        Ok(name)
    }

    fn atoms(&mut self, es: &[Expr]) -> Result<Vec<String>, String> {
        es.iter().map(|e| self.expr(e)).collect()
    }

    fn assign(&mut self, prefix: &str, rhs: String) -> String {
        let t = self.fresh(prefix);
        self.line(format!("{t} = {rhs}"));
        t
    }

    fn expr(&mut self, e: &Expr) -> Result<String, String> {
        Ok(match &e.kind {
            ExprKind::Int(n) => n.to_string(),
            ExprKind::Bool(b) => if *b { "True" } else { "False" }.into(),
            ExprKind::Str(s) => py_str(s),
            ExprKind::Var(n) => self.lookup(n)?,
            ExprKind::Quote(x) => self.expr(x)?,
            ExprKind::Field(r, f) => {
                let r = self.expr(r)?;
                self.assign("t", format!("R.field({r}, {})", py_str(&f.name)))
            }
            ExprKind::Call { callee, args } => {
                self.site = Some(e.span);
                self.call(callee, args)?
            }
            ExprKind::Method { recv, method, args } => {
                if matches!(&recv.kind, ExprKind::Var(name) if name == "s") {
                    let rendered: Vec<String> = args.iter().map(|a| self.expr(a)).collect::<Result<_, _>>()?;
                    let builtin = match method.name.as_str() {
                        "managed" if rendered.is_empty() => "managed",
                        "telemetry" if rendered.is_empty() => "telemetry",
                        "is_managed" if rendered.len() == 1 => "is_managed",
                        "scale_justified" if rendered.len() == 2 => "scale_justified",
                        "inventory" if rendered.len() == 1 => "inventory",
                        "member" if rendered.len() == 2 => "member",
                        "within" if rendered.len() == 2 => "within",
                        "mentions" if rendered.len() == 2 => "mentions",
                        "binds" if rendered.len() >= 2 => "binds",
                        "authorizes" if rendered.len() >= 2 => "authorizes",
                        _ => return Err(format!("método intrínseco s.{} inválido", method.name)),
                    };
                    self.assign("t", format!("R.{builtin}({})", rendered.join(", ")))
                } else {
                    let r = self.expr(recv)?;
                    if method.name != "mint" {
                        return Err(format!("método `{}` sem semântica no runtime", method.name));
                    }
                    self.assign("k", format!("R.mint({r})"))
                }
            }
            ExprKind::Ctor { name, fields } => {
                let ty = self.variants.get(&name.name).cloned().ok_or_else(|| format!("construtor `{}`", name.name))?;
                let mut parts = Vec::new();
                for (f, x) in fields {
                    let a = self.expr(x)?;
                    parts.push(format!("({}, {a})", py_str(&f.name)));
                }
                self.assign("t", format!("Adt({}, {}, [{}])", py_str(&ty), py_str(&name.name), parts.join(", ")))
            }
            ExprKind::Binary(op @ (BinOp::And | BinOp::Or), l, r) => {
                let l = self.expr(l)?;
                let t = self.assign("b", format!("R.bool_operand({l})"));
                let test = if *op == BinOp::And { t.clone() } else { format!("not {t}") };
                self.line(format!("if {test}:"));
                self.indented(|g| {
                    let r = g.expr(r)?;
                    g.line(format!("{t} = R.bool_operand({r})"));
                    Ok(())
                })?;
                t
            }
            ExprKind::Binary(op, l, r) => {
                let l = self.expr(l)?;
                let r = self.expr(r)?;
                self.assign("t", format!("R.binop({}, {l}, {r})", py_str(op.symbol())))
            }
            ExprKind::Unary(op, x) => {
                let a = self.expr(x)?;
                let k = if *op == UnOp::Not { "not" } else { "neg" };
                self.assign("t", format!("R.unop(\"{k}\", {a})"))
            }
            ExprKind::If { cond, then, els } => {
                let c = self.expr(cond)?;
                let t = self.fresh("t");
                self.line(format!("if R.cond({c}):"));
                self.indented(|g| {
                    let a = g.block(then)?;
                    g.line(format!("{t} = {a}"));
                    Ok(())
                })?;
                self.line("else:");
                self.indented(|g| {
                    let a = match els {
                        Some(b) => g.block(b)?,
                        None => "None".into(),
                    };
                    g.line(format!("{t} = {a}"));
                    Ok(())
                })?;
                t
            }
            ExprKind::Match { scrut, arms } => {
                let s = self.expr(scrut)?;
                let v = self.assign("v", format!("R.variant({s})"));
                let t = self.fresh("t");
                for (i, arm) in arms.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "elif" };
                    self.line(format!("{kw} {v} == {}:", py_str(&arm.variant.name)));
                    self.indented(|g| {
                        g.env.push(Vec::new());
                        for b in &arm.binds {
                            let p = g.bind(&b.name);
                            g.line(format!("{p} = R.bind({s}, {})", py_str(&b.name)));
                        }
                        let a = g.expr(&arm.body);
                        g.env.pop();
                        let a = a?;
                        g.line(format!("{t} = {a}"));
                        Ok(())
                    })?;
                }
                self.line(if arms.is_empty() { "if True:" } else { "else:" });
                self.indented(|g| {
                    g.line("raise RtErr(\"R0104\", \"nenhum braço casou\")");
                    Ok(())
                })?;
                t
            }
            ExprKind::Block(b) => self.block(b)?,
            ExprKind::Return(v) => {
                let a = self.expr(v)?;
                self.line(format!("raise Ret({a})"));
                "None".into()
            }
            ExprKind::Infer { model, out, fields } => {
                let mut parts = Vec::new();
                for (k, v) in fields {
                    let quoted = matches!(v.kind, ExprKind::Quote(_));
                    // O rótulo do campo: dado citado (Untrusted), fotografia do host (Trusted) ou instrução.
                    let tag = if quoted {
                        "True"
                    } else if v.is_snapshot_field() {
                        "\"trusted\""
                    } else {
                        "False"
                    };
                    let a = self.expr(v)?;
                    parts.push(format!("({}, {tag}, {a})", py_str(&k.name)));
                }
                let shape = shape_of(self.m, out).map_err(|m| format!("forma de saída: {m}"))?;
                let name = format!("SHAPE_{}", self.shapes.len());
                let schemas = json!({
                    "core": to_json_schema_with(&shape, Dialect::Core),
                    "full": to_json_schema_with(&shape, Dialect::Full),
                });
                self.shapes.push((name.clone(), shape_json(&shape), max_bytes(&shape), schemas, to_gbnf(&shape)));
                self.assign(
                    "d",
                    format!(
                        "R.draft({}, {}, ({name}, {name}_BYTES, {name}_SCHEMA, {name}_GBNF), [{}])",
                        py_str(&model.name),
                        py_str(&out.display()),
                        parts.join(", ")
                    ),
                )
            }
            ExprKind::Refine { draft, policy, retry, els } => {
                if !self.policies.contains(&policy.name) {
                    return Err(format!("política `{}` ausente", policy.name));
                }
                let d = self.expr(draft)?;
                let ok = self.fresh("ok");
                let v = self.fresh("v");
                let t = self.fresh("t");
                // M-I: as obrigações levantadas para este `refine`, como dados
                let lifted = self.ob.lifted_at(e.span).to_vec();
                let extra = if lifted.is_empty() {
                    String::new()
                } else {
                    let specs: Vec<String> = lifted
                        .iter()
                        .map(|l| {
                            let tool = self.m.items.iter().find_map(|i| match i {
                                Item::Tool(t) if t.name.name == l.tool => Some(t),
                                _ => None,
                            });
                            let path: Vec<String> = l
                                .path
                                .iter()
                                .map(|s| match s {
                                    ath_syntax::obligations::Step::Elem => "(\"e\",)".to_string(),
                                    ath_syntax::obligations::Step::Field(v, f) => {
                                        format!("(\"f\", {}, {})", py_str(v), py_str(f))
                                    }
                                })
                                .collect();
                            let binds: Vec<String> = l
                                .binds
                                .iter()
                                .filter_map(|(p, f)| {
                                    let i = tool?.params.iter().position(|x| &x.name.name == p)?;
                                    Some(format!("({i}, {})", py_str(f)))
                                })
                                .collect();
                            let n = tool.map_or(0, |t| t.params.len());
                            format!(
                                "({}, [{}], {}, [{}], {n})",
                                py_str(&l.tool),
                                path.join(", "),
                                py_str(&l.variant),
                                binds.join(", ")
                            )
                        })
                        .collect();
                    format!(", lifted=[{}]", specs.join(", "))
                };
                self.line(format!(
                    "{ok}, {v} = R.refine({d}, policy_{}, {}, {retry}{extra})",
                    ident(&policy.name),
                    py_str(&policy.name)
                ));
                self.line(format!("if {ok}:"));
                self.indented(|g| {
                    g.line(format!("{t} = {v}"));
                    Ok(())
                })?;
                self.line("else:");
                self.indented(|g| {
                    let a = g.block(els)?;
                    g.line(format!("{t} = {a}"));
                    Ok(())
                })?;
                t
            }
            ExprKind::Commit(x) => {
                let t = self.fresh("t");
                self.line("R.committing = True");
                self.line("try:");
                self.indented(|g| {
                    let a = g.expr(x)?;
                    g.line(format!("{t} = {a}"));
                    Ok(())
                })?;
                self.line("finally:");
                self.indented(|g| {
                    g.line("R.committing = False");
                    Ok(())
                })?;
                t
            }
            ExprKind::Atomic { recover, body, on_failure, on_cf } => {
                let rec = self.thunk("recover", recover)?;
                let bod = self.block_fn("saga", None, body)?;
                let onf = match on_failure {
                    Some((bind, b)) => self.block_fn("on_failure", Some(bind), b)?,
                    None => "None".into(),
                };
                let oncf = match on_cf {
                    Some((bind, b)) => self.block_fn("on_compensation_failed", Some(bind), b)?,
                    None => "None".into(),
                };
                self.assign("t", format!("R.atomic({rec}, {bod}, {onf}, {oncf})"))
            }
            ExprKind::Await { cond, within } => {
                let c = self.thunk("cond", cond)?;
                self.line(format!("R.await_({c}, {within:?})"));
                "None".into()
            }
            ExprKind::Reconcile { pending, fuel, els } => {
                let p = self.lookup(&pending.name)?;
                self.line(format!("R.reconcile({p}, {fuel})"));
                self.block(els)?
            }
            ExprKind::Transfer(p) => {
                let p = self.lookup(&p.name)?;
                self.line(format!("R.transfer({p})"));
                "None".into()
            }
            ExprKind::List(items) => {
                let a = self.atoms(items)?;
                self.assign("l", format!("[{}]", a.join(", ")))
            }
            ExprKind::For { var, iter, body } => {
                let it = self.expr(iter)?;
                self.env.push(Vec::new());
                let x = self.bind(&var.name);
                self.line(format!("for {x} in R.iter_list({it}, False):"));
                let r = self.indented(|g| g.block(body).map(|_| ()));
                self.env.pop();
                r?;
                "None".into()
            }
            ExprKind::Quant { all, var, iter, body } => {
                let it = self.expr(iter)?;
                let t = self.assign("q", if *all { "True" } else { "False" }.into());
                self.env.push(Vec::new());
                let x = self.bind(&var.name);
                self.line(format!("for {x} in R.iter_list({it}, True):"));
                let all_py = if *all { "True" } else { "False" };
                let r = self.indented(|g| {
                    let a = g.expr(body)?;
                    let b = g.assign("b", format!("R.quant_body({a})"));
                    g.line(format!("if {b} != {all_py}:"));
                    g.indented(|g| {
                        g.line(format!("{t} = {b}"));
                        g.line("break");
                        Ok(())
                    })
                });
                self.env.pop();
                r?;
                t
            }
        })
    }

    fn call(&mut self, callee: &Ident, args: &[Expr]) -> Result<String, String> {
        // o sítio desta chamada, antes que os argumentos (que podem ter chamadas) o troquem
        let site = self.site.take();
        match callee.name.as_str() {
            "fit" => {
                let a = self.expr(&args[0])?;
                let n = match args.get(1).map(|x| &x.kind) {
                    Some(ExprKind::Int(n)) => (*n).max(0),
                    _ => 0,
                };
                return Ok(self.assign("t", format!("R.fit({a}, {n})")));
            }
            "sample" => {
                let a = self.expr(&args[0])?;
                return Ok(self.assign("t", format!("R.sample({a})")));
            }
            "len" => {
                let a = self.expr(&args[0])?;
                return Ok(self.assign("t", format!("R.len_({a})")));
            }
            _ => {}
        }
        if self.tools.contains(&callee.name) {
            let a = self.atoms(args)?;
            let discharged = site.is_some_and(|s| self.ob.discharged(s));
            if discharged {
                return Ok(self.assign(
                    "t",
                    format!("R.call_tool({}, [{}], discharged=True)", py_str(&callee.name), a.join(", ")),
                ));
            }
            return Ok(self.assign("t", format!("R.call_tool({}, [{}])", py_str(&callee.name), a.join(", "))));
        }
        if self.policies.contains(&callee.name) {
            let a = self.atoms(args)?;
            return Ok(self.assign("t", format!("R.call_policy(policy_{}, [{}])", ident(&callee.name), a.join(", "))));
        }
        Err(format!("função `{}` desconhecida", callee.name))
    }
}
