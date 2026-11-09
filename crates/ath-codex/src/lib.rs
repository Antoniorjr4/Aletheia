//! Codex: cada definição vira uma forma canônica (s-expressão) endereçada pelo
//! hash BLAKE3 do seu conteúdo. Referências a outras definições entram pelo
//! hash delas (DAG de Merkle); nomes de definições e de variáveis locais são
//! metadados — renomear não muda nenhum hash.

use ath_syntax::ast::*;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

pub const DOMAIN: &str = "ath0";

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub kind: &'static str,
    pub hash: String,
    pub canon: String,
    pub deps: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Codex {
    pub entries: Vec<Entry>,
    /// Hash da raiz do módulo: liga cada nome ao hash da sua definição.
    pub root: String,
}

impl Codex {
    pub fn hash_of(&self, name: &str) -> Option<&str> {
        self.entries.iter().find(|e| e.name == name).map(|e| e.hash.as_str())
    }
}

pub fn short(hash: &str) -> &str {
    &hash[..hash.len().min(12)]
}

fn digest(s: &str) -> String {
    blake3::hash(format!("{DOMAIN}\n{s}").as_bytes()).to_hex().to_string()
}

pub fn address(m: &Module) -> Result<Codex, String> {
    let items: HashMap<&str, &Item> = m.items.iter().map(|i| (i.name(), i)).collect();
    let mut owner: HashMap<String, String> = HashMap::new();
    for item in &m.items {
        if let Item::Type(t) = item {
            for v in &t.variants {
                owner.insert(v.name.name.clone(), t.name.name.clone());
            }
        }
    }
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    for item in &m.items {
        let mut d = Vec::new();
        Deps { items: &items, owner: &owner, out: &mut d }.item(item);
        d.sort();
        d.dedup();
        d.retain(|n| n != item.name());
        deps.insert(item.name().to_string(), d);
    }

    let mut hashes: HashMap<String, String> = HashMap::new();
    let mut entries = Vec::new();
    let mut state: HashMap<String, u8> = HashMap::new();
    for item in &m.items {
        visit(item.name(), &items, &deps, &owner, &mut state, &mut hashes, &mut entries)?;
    }

    let mut root_src = String::from("(module");
    let sorted: BTreeMap<&str, &Entry> = entries.iter().map(|e| (e.name.as_str(), e)).collect();
    for (name, e) in sorted {
        let _ = write!(root_src, " ({} {:?} {})", e.kind, name, e.hash);
    }
    root_src.push(')');
    let root = digest(&root_src);
    let order: HashMap<&str, usize> = m.items.iter().enumerate().map(|(i, it)| (it.name(), i)).collect();
    entries.sort_by_key(|e| order.get(e.name.as_str()).copied().unwrap_or(usize::MAX));
    Ok(Codex { entries, root })
}

fn visit(
    name: &str,
    items: &HashMap<&str, &Item>,
    deps: &HashMap<String, Vec<String>>,
    owner: &HashMap<String, String>,
    state: &mut HashMap<String, u8>,
    hashes: &mut HashMap<String, String>,
    entries: &mut Vec<Entry>,
) -> Result<(), String> {
    match state.get(name) {
        Some(2) => return Ok(()),
        Some(1) => {
            return Err(format!(
                "ciclo de definições envolvendo `{name}` (recursão entre definições não é suportada no Marco Zero)"
            ))
        }
        _ => {}
    }
    state.insert(name.to_string(), 1);
    for d in &deps[name] {
        visit(d, items, deps, owner, state, hashes, entries)?;
    }
    let item = items[name];
    let mut c = Canon { hashes, owner, locals: Vec::new(), out: String::new() };
    c.item(item);
    let canon = c.out;
    let hash = digest(&canon);
    hashes.insert(name.to_string(), hash.clone());
    entries.push(Entry { name: name.to_string(), kind: item.kind(), hash, canon, deps: deps[name].clone() });
    state.insert(name.to_string(), 2);
    Ok(())
}

/// Grava o Codex em disco: `objects/<hash>` com a forma canônica e `names` com o índice.
pub fn write_store(c: &Codex, dir: &Path) -> std::io::Result<()> {
    let objects = dir.join("objects");
    std::fs::create_dir_all(&objects)?;
    let mut index = String::new();
    for e in &c.entries {
        std::fs::write(objects.join(&e.hash), &e.canon)?;
        let _ = writeln!(index, "{}\t{}\t{}", e.kind, e.name, e.hash);
    }
    let _ = writeln!(index, "root\t-\t{}", c.root);
    std::fs::write(dir.join("names"), index)
}

// ------------------------------------------------------------------ dependências

struct Deps<'a> {
    items: &'a HashMap<&'a str, &'a Item>,
    owner: &'a HashMap<String, String>,
    out: &'a mut Vec<String>,
}

impl Deps<'_> {
    fn name(&mut self, n: &str) {
        if self.items.contains_key(n) {
            self.out.push(n.to_string());
        }
    }

    fn variant(&mut self, v: &str) {
        if let Some(t) = self.owner.get(v) {
            self.out.push(t.clone());
        }
    }

    fn ty(&mut self, t: &TypeExpr) {
        match t {
            TypeExpr::Named(id) => self.name(&id.name),
            TypeExpr::Refined { .. } | TypeExpr::Cap(..) => {}
            TypeExpr::Labeled(inner, ..) => self.ty(inner),
        }
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Type(d) => d.variants.iter().flat_map(|v| &v.fields).for_each(|f| self.ty(&f.ty)),
            Item::Model(_) => {}
            Item::Tool(d) => {
                d.params.iter().for_each(|p| self.ty(&p.ty));
                self.ty(&d.ret);
            }
            Item::Policy(d) => {
                d.params.iter().for_each(|p| self.ty(&p.ty));
                self.ty(&d.ret);
                self.expr(&d.body);
            }
            Item::Agent(d) => {
                d.params.iter().for_each(|p| self.ty(&p.ty));
                for h in &d.handlers {
                    h.params.iter().for_each(|p| self.ty(&p.ty));
                    self.ty(&h.ret);
                    self.block(&h.body);
                }
            }
        }
    }

    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            match s {
                Stmt::Let { value, .. } => self.expr(value),
                Stmt::Expr(e) => self.expr(e),
            }
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Var(_) => {}
            ExprKind::Field(r, _) => self.expr(r),
            ExprKind::Call { callee, args } => {
                self.name(&callee.name);
                args.iter().for_each(|a| self.expr(a));
            }
            ExprKind::Method { recv, args, .. } => {
                self.expr(recv);
                args.iter().for_each(|a| self.expr(a));
            }
            ExprKind::Ctor { name, fields } => {
                self.variant(&name.name);
                fields.iter().for_each(|(_, x)| self.expr(x));
            }
            ExprKind::Binary(_, l, r) => {
                self.expr(l);
                self.expr(r);
            }
            ExprKind::Unary(_, v) | ExprKind::Quote(v) | ExprKind::Commit(v) | ExprKind::Return(v) => self.expr(v),
            ExprKind::If { cond, then, els } => {
                self.expr(cond);
                self.block(then);
                if let Some(b) = els {
                    self.block(b);
                }
            }
            ExprKind::Match { scrut, arms } => {
                self.expr(scrut);
                for a in arms {
                    self.variant(&a.variant.name);
                    self.expr(&a.body);
                }
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Infer { model, out, fields } => {
                self.name(&model.name);
                self.ty(out);
                fields.iter().for_each(|(_, x)| self.expr(x));
            }
            ExprKind::Refine { draft, policy, els, .. } => {
                self.expr(draft);
                self.name(&policy.name);
                self.block(els);
            }
        }
    }
}

// ------------------------------------------------------------------ forma canônica

struct Canon<'a> {
    hashes: &'a HashMap<String, String>,
    owner: &'a HashMap<String, String>,
    locals: Vec<String>,
    out: String,
}

impl Canon<'_> {
    fn w(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn reference(&mut self, n: &str) {
        match self.hashes.get(n) {
            Some(h) => {
                let h = h.clone();
                let _ = write!(self.out, "@{h}");
            }
            None => {
                let _ = write!(self.out, "?{n}");
            }
        }
    }

    fn var(&mut self, n: &str) {
        match self.locals.iter().rev().position(|l| l == n) {
            Some(i) => {
                let _ = write!(self.out, "#{i}");
            }
            None => self.reference(n),
        }
    }

    fn ty(&mut self, t: &TypeExpr) {
        match t {
            TypeExpr::Named(id) => match id.name.as_str() {
                "Int" | "Bool" | "Text" | "Unit" | "IdemKey" => self.w(&id.name),
                n => self.reference(n),
            },
            TypeExpr::Refined { base, lo, hi, .. } => {
                let b = |x: &Option<i64>| x.map(|n| n.to_string()).unwrap_or_else(|| "_".into());
                let _ = write!(self.out, "({} {} {})", base.name, b(lo), b(hi));
            }
            TypeExpr::Cap(effs, _) => {
                let mut names: Vec<&str> = effs.iter().map(|e| e.name.as_str()).collect();
                names.sort();
                names.dedup();
                let _ = write!(self.out, "(cap {})", names.join(" "));
            }
            TypeExpr::Labeled(inner, l, _) => {
                let _ = write!(self.out, "(lab {} ", l.name());
                self.ty(inner);
                self.w(")");
            }
        }
    }

    fn params(&mut self, ps: &[Param]) {
        self.w("(params");
        for p in ps {
            self.w(" ");
            self.ty(&p.ty);
        }
        self.w(")");
    }

    fn effects(&mut self, effs: &[Ident]) {
        let mut names: Vec<&str> = effs.iter().map(|e| e.name.as_str()).collect();
        names.sort();
        names.dedup();
        let _ = write!(self.out, "(effects {})", names.join(" "));
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Type(d) => {
                self.w("(type");
                for v in &d.variants {
                    let _ = write!(self.out, " (variant {}", v.name.name);
                    for f in &v.fields {
                        let _ = write!(self.out, " ({} ", f.name.name);
                        self.ty(&f.ty);
                        self.w(")");
                    }
                    self.w(")");
                }
                self.w(")");
            }
            Item::Model(d) => {
                let mut props: Vec<(&str, f64)> = d.props.iter().map(|p| (p.key.name.as_str(), p.value)).collect();
                props.sort_by(|a, b| a.0.cmp(b.0));
                self.w("(model");
                for (k, v) in props {
                    let _ = write!(self.out, " ({k} {v})");
                }
                self.w(")");
            }
            Item::Tool(d) => {
                let _ = write!(self.out, "(tool (host {:?}) ", d.name.name);
                self.params(&d.params);
                self.w(" (ret ");
                self.ty(&d.ret);
                self.w(") ");
                self.effects(&d.effects);
                let _ = write!(self.out, " {} (latency {}) (usd {}))", d.class.name(), d.latency, d.usd);
            }
            Item::Policy(d) => {
                self.w("(policy ");
                self.params(&d.params);
                self.w(" (ret ");
                self.ty(&d.ret);
                self.w(") ");
                for p in &d.params {
                    self.locals.push(p.name.name.clone());
                }
                self.expr(&d.body);
                self.locals.clear();
                self.w(")");
            }
            Item::Agent(d) => {
                self.w("(agent ");
                self.params(&d.params);
                let mut budget: Vec<(&str, f64)> = d.budget.iter().map(|p| (p.key.name.as_str(), p.value)).collect();
                budget.sort_by(|a, b| a.0.cmp(b.0));
                self.w(" (budget");
                for (k, v) in budget {
                    let _ = write!(self.out, " ({k} {v})");
                }
                self.w(")");
                for p in &d.params {
                    self.locals.push(p.name.name.clone());
                }
                for h in &d.handlers {
                    let _ = write!(self.out, " (on {:?} ", h.event.name);
                    self.params(&h.params);
                    self.w(" (ret ");
                    self.ty(&h.ret);
                    self.w(") ");
                    self.effects(&h.effects);
                    self.w(" ");
                    let depth = self.locals.len();
                    for p in &h.params {
                        self.locals.push(p.name.name.clone());
                    }
                    self.block(&h.body);
                    self.locals.truncate(depth);
                    self.w(")");
                }
                self.locals.clear();
                self.w(")");
            }
        }
    }

    fn block(&mut self, b: &Block) {
        let depth = self.locals.len();
        self.w("(block");
        for s in &b.stmts {
            self.w(" ");
            match s {
                Stmt::Let { name, value, .. } => {
                    self.w("(let ");
                    self.expr(value);
                    self.w(")");
                    self.locals.push(name.name.clone());
                }
                Stmt::Expr(e) => self.expr(e),
            }
        }
        if let Some(t) = &b.tail {
            self.w(" ");
            self.expr(t);
        }
        self.w(")");
        self.locals.truncate(depth);
    }

    fn list(&mut self, es: &[Expr]) {
        for e in es {
            self.w(" ");
            self.expr(e);
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Int(n) => {
                let _ = write!(self.out, "{n}");
            }
            ExprKind::Bool(b) => {
                let _ = write!(self.out, "{b}");
            }
            ExprKind::Str(s) => {
                let _ = write!(self.out, "{s:?}");
            }
            ExprKind::Var(n) => self.var(n),
            ExprKind::Field(r, f) => {
                self.w("(field ");
                self.expr(r);
                let _ = write!(self.out, " {})", f.name);
            }
            ExprKind::Call { callee, args } => {
                self.w("(call ");
                match callee.name.as_str() {
                    "fit" | "sample" => {
                        let _ = write!(self.out, "prim:{}", callee.name);
                    }
                    n => self.reference(n),
                }
                self.list(args);
                self.w(")");
            }
            ExprKind::Method { recv, method, args } => {
                self.w("(method ");
                self.expr(recv);
                let _ = write!(self.out, " {}", method.name);
                self.list(args);
                self.w(")");
            }
            ExprKind::Ctor { name, fields } => {
                self.w("(ctor ");
                match self.owner.get(&name.name).cloned() {
                    Some(t) => self.reference(&t),
                    None => self.w("?"),
                }
                let _ = write!(self.out, " {}", name.name);
                for (f, x) in fields {
                    let _ = write!(self.out, " ({} ", f.name);
                    self.expr(x);
                    self.w(")");
                }
                self.w(")");
            }
            ExprKind::Binary(op, l, r) => {
                let _ = write!(self.out, "({} ", op.symbol());
                self.expr(l);
                self.w(" ");
                self.expr(r);
                self.w(")");
            }
            ExprKind::Unary(op, v) => {
                self.w(if *op == UnOp::Not { "(not " } else { "(neg " });
                self.expr(v);
                self.w(")");
            }
            ExprKind::If { cond, then, els } => {
                self.w("(if ");
                self.expr(cond);
                self.w(" ");
                self.block(then);
                if let Some(b) = els {
                    self.w(" ");
                    self.block(b);
                }
                self.w(")");
            }
            ExprKind::Match { scrut, arms } => {
                self.w("(match ");
                self.expr(scrut);
                for a in arms {
                    let _ = write!(
                        self.out,
                        " (arm {} ({})",
                        a.variant.name,
                        a.binds.iter().map(|b| b.name.as_str()).collect::<Vec<_>>().join(" ")
                    );
                    let depth = self.locals.len();
                    for b in &a.binds {
                        self.locals.push(b.name.clone());
                    }
                    self.w(" ");
                    self.expr(&a.body);
                    self.locals.truncate(depth);
                    self.w(")");
                }
                self.w(")");
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Return(v) => {
                self.w("(return ");
                self.expr(v);
                self.w(")");
            }
            ExprKind::Infer { model, out, fields } => {
                self.w("(infer ");
                self.reference(&model.name);
                self.w(" ");
                self.ty(out);
                for (k, v) in fields {
                    let _ = write!(self.out, " ({} ", k.name);
                    self.expr(v);
                    self.w(")");
                }
                self.w(")");
            }
            ExprKind::Refine { draft, policy, retry, els } => {
                self.w("(refine ");
                self.expr(draft);
                self.w(" ");
                self.reference(&policy.name);
                let _ = write!(self.out, " (retry {retry}) ");
                self.block(els);
                self.w(")");
            }
            ExprKind::Quote(v) => {
                self.w("(quote ");
                self.expr(v);
                self.w(")");
            }
            ExprKind::Commit(v) => {
                self.w("(commit ");
                self.expr(v);
                self.w(")");
            }
        }
    }
}
