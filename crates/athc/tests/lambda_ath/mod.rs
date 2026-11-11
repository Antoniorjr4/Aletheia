//! λ_Ath executável: um gerador de programas do núcleo formal (`lean/Aletheia/Syntax.lean`)
//! e a tradução de cada um para Aletheia de superfície.
//!
//! O gerador é dirigido por tipo e produz só programas do domínio dos teoremas do Lean: bem
//! tipados (`HasType`), lineares (`Linear`: cada `Cap[E]` consumida exatamente uma vez em todo
//! caminho) e, com sagas, `Guarded` (todo `await` dentro de um corpo de `atomic`). Isso não é só intenção: [`Prog::check`] reimplementa as regras de `Typing.lean`
//! (o julgamento sem o orçamento, que o diferencial não compara) e a contagem `consumes`, e
//! recusa qualquer programa fora do fragmento.
//!
//! A tradução respeita a ordem de avaliação do avaliador canônico (`Eval.lean`): todo
//! subtermo com evento é nomeado num `let` (forma A-normal), e `infer` só aparece como
//! rascunho imediato de um `refine`, que é onde o ARX e o runtime Python chamam o modelo.
//! Assim a `i`-ésima chamada ao oráculo do Lean é a `i`-ésima amostra do roteiro nos runtimes.

use serde_json::{json, Value as Json};

// ------------------------------------------------------------------ gerador pseudoaleatório

/// SplitMix64: determinístico e sem dependências; a semente identifica o programa.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }

    fn chance(&mut self, num: u64, den: u64) -> bool {
        self.next() % den < num
    }
}

// ------------------------------------------------------------------ o núcleo

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Int,
    Bool,
    Unit,
    List(Box<Ty>, usize),
}

/// `U ⊑ E ⊑ T`; a ordem da declaração é a do reticulado, então `min` é o encontro.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lab {
    U,
    E,
    T,
}

impl Lab {
    fn meet(self, o: Lab) -> Lab {
        self.min(o)
    }

    fn low(self) -> bool {
        self != Lab::U
    }
}

#[derive(Clone, Debug)]
pub enum V {
    Int(i64),
    Bool(bool),
    Unit,
    List(Vec<V>),
}

#[derive(Clone, Copy, Debug)]
pub enum Op {
    Add,
    Sub,
    Le,
    Eq,
}

impl Op {
    fn ty(self) -> Ty {
        match self {
            Op::Add | Op::Sub => Ty::Int,
            Op::Le | Op::Eq => Ty::Bool,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Op::Add => "add",
            Op::Sub => "sub",
            Op::Le => "le",
            Op::Eq => "eq",
        }
    }

    fn surface(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Le => "<=",
            Op::Eq => "==",
        }
    }
}

/// As expressões de `Exp` (`Syntax.lean`), com índices de de Bruijn.
#[derive(Clone, Debug)]
pub enum E {
    Val(V),
    Var(usize),
    Let(Box<E>, Box<E>),
    Ite(Box<E>, Box<E>, Box<E>),
    Prim(Op, Box<E>, Box<E>),
    Endorse(usize, Box<E>, Box<E>),
    Act(usize, usize, Box<E>),
    Drop(usize),
    For(usize, Box<E>, Box<E>),
    Infer(usize, Ty, Box<E>),
    /// `await q`: a sonda `q` precisa observar a pós-condição
    Await(usize),
    /// `atomic corpo r f g`: a saga, com a sonda de recuperação `r`, o tratador `f` (on
    /// failure) e `g` (on compensation_failed)
    Atomic(Box<E>, usize, Box<E>, Box<E>),
    /// `call t k a`: a chamada com chave da ferramenta `t` pela capacidade `k`. O combustível e
    /// o tratador de `TimeoutUnknown` são os do programa (`Prog::fuel`, `Prog::handler`), como
    /// o `handle timeout_unknown` único de um agente
    Call(usize, usize, Box<E>),
    /// `transfer p`: entrega a obrigação `Pending[k]` da posição `p` (só no tratador)
    Transfer(usize),
}

#[derive(Clone, Debug)]
pub enum Bind {
    Data(Ty, Lab),
    Cap(Vec<usize>),
    /// a obrigação `Pending[k]`, no ambiente do tratador
    Pend,
}

/// A classe do provedor de uma ferramenta com chave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prov {
    Idempotent,
    Queryable,
    Opaque,
}

impl Prov {
    pub fn name(self) -> &'static str {
        match self {
            Prov::Idempotent => "idempotent",
            Prov::Queryable => "queryable",
            Prov::Opaque => "opaque",
        }
    }
}

/// O provedor de uma ferramenta com chave, como o `SimWorld` o simula: falhas injetadas por
/// número de chamada (1 é a chamada; 2, 3, … os reenvios de um provedor `idempotent`) e, para
/// `queryable`, as consultas de status que respondem "desconhecido" antes da verdade.
#[derive(Clone, Debug)]
pub struct KeySpec {
    /// a capacidade que dá a chave à chamada (a chave do Lean)
    pub cap: usize,
    pub provider: Prov,
    pub faults: Vec<(u32, &'static str)>,
    pub unknown: u32,
}

/// O que o provedor responde a cada tentativa e se o efeito existe no fim.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyRun {
    pub replies: Vec<&'static str>,
    pub applied: bool,
}

impl KeySpec {
    /// A simulação do `SimWorld` (`crates/arx/src/host.rs`): uma chave já aplicada responde o
    /// recibo sem contar a chamada; senão conta e consulta a falha da vez. A ferramenta tem um
    /// único ponto de chamada executado, então a contagem só depende desta chave.
    pub fn simulate(&self, fuel: usize) -> KeyRun {
        let mut calls = 0u32;
        let mut applied = false;
        let mut unknown = self.unknown;
        let faults = &self.faults;
        let mut call = |applied: &mut bool| -> &'static str {
            if *applied {
                return "done";
            }
            calls += 1;
            match faults.iter().find(|f| f.0 == calls).map(|f| f.1) {
                Some("error") => "failed",
                Some("timeout_lost") => "unknown",
                Some("timeout_applied") => {
                    *applied = true;
                    "unknown"
                }
                _ => {
                    *applied = true;
                    "done"
                }
            }
        };
        let mut replies = vec![call(&mut applied)];
        if replies[0] == "unknown" {
            for _ in 0..fuel {
                let r = match self.provider {
                    Prov::Opaque => break,
                    Prov::Idempotent => call(&mut applied),
                    Prov::Queryable => {
                        if unknown > 0 {
                            unknown -= 1;
                            "unknown"
                        } else if applied {
                            "done"
                        } else {
                            "failed"
                        }
                    }
                };
                replies.push(r);
                if r != "unknown" {
                    break;
                }
            }
        }
        KeyRun { replies, applied }
    }
}

pub struct Tool {
    pub effect: usize,
    pub arg: Ty,
    /// a célula do mundo que a ferramenta altera (`w[cell] += v`)
    pub cell: Option<usize>,
    /// `compensable by u<i>`: entra no registro da saga
    pub compensable: bool,
    /// o provedor recusa a compensação
    pub undo_fails: bool,
    /// ferramenta com chave (`k: own IdemKey`) e o seu provedor
    pub key: Option<KeySpec>,
}

/// Sonda de célula: `lo ≤ w[cell] ≤ hi`.
pub struct Probe {
    pub cell: usize,
    pub lo: i64,
    pub hi: i64,
}

pub struct Policy {
    pub lo: i64,
    pub hi: i64,
}

/// Um programa do núcleo, com a assinatura Δ, o ambiente inicial e o roteiro do oráculo.
pub struct Prog {
    pub seed: u64,
    pub effects: usize,
    pub tools: Vec<Tool>,
    pub policies: Vec<Policy>,
    /// `caps[id]`: os efeitos que o runtime concede à capacidade `id` (o seu tipo `Cap[E]`).
    pub caps: Vec<Vec<usize>>,
    pub oracle: Vec<i64>,
    pub exp: E,
    pub ret: Ty,
    pub probes: Vec<Probe>,
    /// o combustível da reconciliação no tratador de `TimeoutUnknown`
    pub fuel: usize,
    /// o valor com que o tratador encerra o evento depois de transferir a obrigação
    pub hval: V,
}

impl Prog {
    /// Γ inicial: a capacidade 0 é a mais antiga, então o índice 0 é a última.
    pub fn gamma(&self) -> Vec<Bind> {
        self.caps.iter().map(|e| Bind::Cap(e.clone())).collect()
    }

    /// O tratador de `TimeoutUnknown` no núcleo: `let _ = transfer 0 in hval`, o `else` de
    /// `reconcile p fuel n else { transfer p; hval }`.
    pub fn handler(&self) -> E {
        E::Let(Box::new(E::Transfer(0)), Box::new(E::Val(self.hval.clone())))
    }

    pub fn has_calls(&self) -> bool {
        fn go(e: &E) -> bool {
            match e {
                E::Call(..) => true,
                E::Val(_) | E::Var(_) | E::Drop(_) | E::Await(_) | E::Transfer(_) => false,
                E::Let(a, b) | E::Prim(_, a, b) | E::Endorse(_, a, b) => go(a) || go(b),
                E::Ite(c, t, f) => go(c) || go(t) || go(f),
                E::Act(_, _, a) | E::Infer(_, _, a) => go(a),
                E::For(_, xs, b) => go(xs) || go(b),
                E::Atomic(b, _, f, g) => go(b) || go(f) || go(g),
            }
        }
        go(&self.exp)
    }

    /// O roteiro do provedor de cada chave (a capacidade que a carrega).
    pub fn key_runs(&self) -> Vec<(usize, usize, KeyRun)> {
        self.tools
            .iter()
            .enumerate()
            .filter_map(|(t, tl)| tl.key.as_ref().map(|k| (t, k.cap, k.simulate(self.fuel))))
            .collect()
    }
}

fn get(ctx: &[Bind], k: usize) -> Option<&Bind> {
    ctx.len().checked_sub(k + 1).map(|i| &ctx[i])
}

// ------------------------------------------------------------------ Typing.lean, reimplementado

fn val_ty(v: &V) -> Option<Ty> {
    Some(match v {
        V::Int(_) => Ty::Int,
        V::Bool(_) => Ty::Bool,
        V::Unit => Ty::Unit,
        V::List(vs) => {
            let t = val_ty(vs.first()?)?;
            if vs.iter().any(|x| val_ty(x).as_ref() != Some(&t)) {
                return None;
            }
            Ty::List(Box::new(t), vs.len())
        }
    })
}

/// `HasType S Γ pc e τ ι _`, sem o orçamento: devolve `(τ, ι)` ou `None`.
fn has_type(p: &Prog, ctx: &mut Vec<Bind>, pc: Lab, e: &E) -> Option<(Ty, Lab)> {
    Some(match e {
        E::Val(v) => (val_ty(v)?, Lab::T),
        E::Var(k) => match get(ctx, *k)? {
            Bind::Data(t, l) => (t.clone(), *l),
            Bind::Cap(_) | Bind::Pend => return None,
        },
        E::Let(a, b) => {
            let (t1, l1) = has_type(p, ctx, pc, a)?;
            ctx.push(Bind::Data(t1, l1));
            let r = has_type(p, ctx, pc, b);
            ctx.pop();
            r?
        }
        E::Ite(c, t, f) => {
            let (tc, lc) = has_type(p, ctx, pc, c)?;
            if tc != Ty::Bool {
                return None;
            }
            let (tt, lt) = has_type(p, ctx, pc.meet(lc), t)?;
            let (tf, lf) = has_type(p, ctx, pc.meet(lc), f)?;
            if tt != tf {
                return None;
            }
            (tt, lc.meet(lt.meet(lf)))
        }
        E::Prim(op, a, b) => {
            let (ta, la) = has_type(p, ctx, pc, a)?;
            let (tb, lb) = has_type(p, ctx, pc, b)?;
            if ta != Ty::Int || tb != Ty::Int {
                return None;
            }
            (op.ty(), la.meet(lb))
        }
        E::Endorse(q, a, d) => {
            if !pc.low() || *q >= p.policies.len() {
                return None;
            }
            let (ta, _) = has_type(p, ctx, pc, a)?;
            let (td, ld) = has_type(p, ctx, pc, d)?;
            if ta != td {
                return None;
            }
            (ta, Lab::E.meet(ld))
        }
        E::Act(t, k, a) => {
            let tool = p.tools.get(*t)?;
            if tool.key.is_some() {
                return None;
            }
            let Bind::Cap(eff) = get(ctx, *k)? else { return None };
            if !pc.low() || !eff.contains(&tool.effect) {
                return None;
            }
            let (ta, la) = has_type(p, ctx, pc, a)?;
            if ta != tool.arg || !la.low() {
                return None;
            }
            (Ty::Unit, Lab::T)
        }
        E::Drop(k) => match get(ctx, *k)? {
            Bind::Cap(_) => (Ty::Unit, Lab::T),
            Bind::Data(..) | Bind::Pend => return None,
        },
        E::Call(t, k, a) => {
            let tool = p.tools.get(*t)?;
            tool.key.as_ref()?;
            let Bind::Cap(eff) = get(ctx, *k)? else { return None };
            if !pc.low() || !eff.contains(&tool.effect) {
                return None;
            }
            let (ta, la) = has_type(p, ctx, pc, a)?;
            if ta != tool.arg || !la.low() {
                return None;
            }
            (Ty::Unit, Lab::T)
        }
        E::Transfer(k) => match get(ctx, *k)? {
            Bind::Pend if pc.low() => (Ty::Unit, Lab::T),
            _ => return None,
        },
        E::For(n, xs, body) => {
            let (tx, lx) = has_type(p, ctx, pc, xs)?;
            let Ty::List(t, m) = tx else { return None };
            if m > *n {
                return None;
            }
            ctx.push(Bind::Data(*t, lx));
            let r = has_type(p, ctx, pc.meet(lx), body);
            ctx.pop();
            r?;
            (Ty::Unit, Lab::T)
        }
        E::Infer(_, t, a) => {
            has_type(p, ctx, pc, a)?;
            (t.clone(), Lab::U)
        }
        E::Await(q) => {
            if !pc.low() || *q >= p.probes.len() {
                return None;
            }
            (Ty::Unit, Lab::T)
        }
        E::Atomic(body, r, f, g) => {
            if !pc.low() || *r >= p.probes.len() {
                return None;
            }
            let (tb, lb) = has_type(p, ctx, pc, body)?;
            let (tf, lf) = has_type(p, ctx, pc, f)?;
            let (tg, lg) = has_type(p, ctx, pc, g)?;
            if tb != tf || tb != tg {
                return None;
            }
            (tb, lb.meet(lf.meet(lg)))
        }
    })
}

/// `guarded` de `Eval.lean`: todo `await` dentro de um corpo de saga.
fn guarded(sg: bool, e: &E) -> bool {
    match e {
        E::Val(_) | E::Var(_) | E::Drop(_) => true,
        E::Let(a, b) | E::Prim(_, a, b) | E::Endorse(_, a, b) => guarded(sg, a) && guarded(sg, b),
        E::Ite(c, t, f) => guarded(sg, c) && guarded(sg, t) && guarded(sg, f),
        E::Act(_, _, a) | E::Infer(_, _, a) => guarded(sg, a),
        E::For(_, xs, body) => guarded(sg, xs) && guarded(sg, body),
        E::Await(_) => sg,
        E::Atomic(body, _, f, g) => guarded(true, body) && guarded(sg, f) && guarded(sg, g),
        E::Call(_, _, a) => guarded(sg, a),
        E::Transfer(_) => true,
    }
}

/// `pureH` de `Typing.lean`: o tratador não tem efeito, inferência, saga nem chamada.
fn pure_h(e: &E) -> bool {
    match e {
        E::Val(_) | E::Var(_) | E::Transfer(_) => true,
        E::Let(a, b) | E::Prim(_, a, b) => pure_h(a) && pure_h(b),
        E::Ite(c, t, f) => pure_h(c) && pure_h(t) && pure_h(f),
        _ => false,
    }
}

fn add(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    Some(a? + b?)
}

fn same(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    if a? == b? {
        a
    } else {
        None
    }
}

fn zero(a: Option<usize>) -> Option<usize> {
    (a? == 0).then_some(0)
}

/// `consumes e k` de `Typing.lean`.
fn consumes(e: &E, k: usize) -> Option<usize> {
    match e {
        E::Val(_) | E::Var(_) => Some(0),
        E::Let(a, b) => add(consumes(a, k), consumes(b, k + 1)),
        E::Ite(c, t, f) => add(consumes(c, k), same(consumes(t, k), consumes(f, k))),
        E::Prim(_, a, b) => add(consumes(a, k), consumes(b, k)),
        E::Endorse(_, a, d) => add(consumes(a, k), zero(consumes(d, k))),
        E::Act(_, j, a) => add(consumes(a, k), Some(usize::from(*j == k))),
        E::Drop(j) => Some(usize::from(*j == k)),
        E::For(_, xs, body) => add(consumes(xs, k), zero(consumes(body, k + 1))),
        E::Infer(_, _, a) => consumes(a, k),
        E::Await(_) => Some(0),
        E::Atomic(body, _, f, g) => add(consumes(body, k), same(consumes(f, k), consumes(g, k))),
        E::Call(_, j, a) => add(consumes(a, k), Some(usize::from(*j == k))),
        E::Transfer(j) => Some(usize::from(*j == k)),
    }
}

impl Prog {
    /// O programa está no domínio do T1–T3: bem tipado em Γ inicial com `pc = Trusted`, do
    /// tipo de retorno declarado, e linear em cada capacidade.
    pub fn check(&self) -> Result<(), String> {
        let mut ctx = self.gamma();
        let (t, _) = has_type(self, &mut ctx, Lab::T, &self.exp).ok_or("fora de HasType")?;
        if t != self.ret {
            return Err(format!("tipo {t:?}, declarado {:?}", self.ret));
        }
        for k in 0..self.caps.len() {
            if consumes(&self.exp, k) != Some(1) {
                return Err(format!("capacidade no índice {k} não é consumida exatamente uma vez"));
            }
        }
        if !guarded(false, &self.exp) {
            return Err("`await` fora de saga (não Guarded)".into());
        }
        // `HasType.call`: o tratador é puro, do tipo de retorno e consome a obrigação uma vez
        if self.has_calls() {
            let h = self.handler();
            let mut hctx = vec![Bind::Pend];
            let (t, _) = has_type(self, &mut hctx, Lab::T, &h).ok_or("tratador fora de HasType")?;
            if t != self.ret || !pure_h(&h) || consumes(&h, 0) != Some(1) {
                return Err("tratador de TimeoutUnknown inválido".into());
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ o gerador

struct Gen<'a> {
    rng: &'a mut Rng,
    tools: &'a [Tool],
    npol: usize,
    /// Γ corrente, em ordem de ligação (o último é o índice 0).
    ctx: Vec<Bind>,
    /// número de sondas (0: sem sagas)
    nprobes: usize,
    /// dentro do corpo de uma saga: `await` permitido
    saga: bool,
    /// o próximo bloco é o corpo de uma saga: termina com um `await` (às vezes)
    tail: bool,
    /// com chaves: a ferramenta com chave da capacidade `i` é `keyed + i`
    keyed: Option<usize>,
}

impl Gen<'_> {
    fn index(&self, pos: usize) -> usize {
        self.ctx.len() - 1 - pos
    }

    fn vars(&self, want: &Ty) -> Vec<usize> {
        (0..self.ctx.len())
            .filter(|&i| matches!(&self.ctx[i], Bind::Data(t, l) if t == want && l.low()))
            .map(|i| self.index(i))
            .collect()
    }

    fn atom(&mut self, want: &Ty) -> E {
        let vs = self.vars(want);
        if !vs.is_empty() && self.rng.chance(2, 3) {
            return E::Var(vs[self.rng.below(vs.len())]);
        }
        E::Val(match want {
            Ty::Int => V::Int(self.rng.int(0, 100)),
            Ty::Bool => V::Bool(self.rng.chance(1, 2)),
            _ => V::Unit,
        })
    }

    fn result(&mut self, want: &Ty) -> E {
        match want {
            Ty::Unit => E::Val(V::Unit),
            Ty::Int if self.rng.chance(1, 2) => {
                let op = if self.rng.chance(1, 2) { Op::Add } else { Op::Sub };
                E::Prim(op, Box::new(self.atom(&Ty::Int)), Box::new(self.atom(&Ty::Int)))
            }
            Ty::Bool if self.rng.chance(1, 2) => {
                let op = if self.rng.chance(1, 2) { Op::Le } else { Op::Eq };
                E::Prim(op, Box::new(self.atom(&Ty::Int)), Box::new(self.atom(&Ty::Int)))
            }
            t => self.atom(t),
        }
    }

    fn any_ty(&mut self) -> Ty {
        [Ty::Int, Ty::Bool, Ty::Unit][self.rng.below(3)].clone()
    }

    /// Ferramentas que a capacidade da posição `pos` autoriza.
    fn tools_for(&self, pos: usize) -> Vec<usize> {
        let Bind::Cap(eff) = &self.ctx[pos] else { return vec![] };
        (0..self.tools.len()).filter(|&t| eff.contains(&self.tools[t].effect) && self.tools[t].key.is_none()).collect()
    }

    fn consume(&mut self, pos: usize, prefer_act: bool) -> E {
        // com chaves: metade das capacidades vai para a chamada com chave da sua ferramenta
        // (provedor `opaque` não entra em saga: o `athc` recusa, T0305)
        if let Some(base) = self.keyed {
            let opaque = self.tools[base + pos].key.as_ref().is_some_and(|k| k.provider == Prov::Opaque);
            if self.rng.chance(1, 2) && !(self.saga && opaque) {
                let k = self.index(pos);
                return E::Call(base + pos, k, Box::new(self.atom(&Ty::Int)));
            }
        }
        let mut ts = self.tools_for(pos);
        // no corpo de uma saga, de preferência uma ação compensável
        if self.saga && self.rng.chance(3, 4) {
            let comp: Vec<usize> = ts.iter().copied().filter(|&t| self.tools[t].compensable).collect();
            if !comp.is_empty() {
                ts = comp;
            }
        }
        let k = self.index(pos);
        if ts.is_empty() || !(prefer_act || self.saga) {
            return E::Drop(k);
        }
        let t = ts[self.rng.below(ts.len())];
        let arg = self.tools[t].arg.clone();
        E::Act(t, k, Box::new(self.atom(&arg)))
    }

    /// Um bloco: `let`s em sequência e um resultado do tipo `want`. Consome exatamente as
    /// capacidades `live` (posições absolutas em `ctx`) em todo caminho.
    fn block(&mut self, mut live: Vec<usize>, depth: usize, want: &Ty) -> E {
        let base = self.ctx.len();
        let tail = std::mem::take(&mut self.tail);
        let mut stmts = Vec::new();
        let n = self.rng.below(4) + usize::from(depth == 0) * 2;
        for _ in 0..n {
            let roll = self.rng.below(if self.nprobes > 0 { 13 } else { 10 });
            let (s, b) = match roll {
                0..=2 if self.npol > 0 => {
                    let prompt = self.atom(&Ty::Int);
                    let infer = E::Infer(0, Ty::Int, Box::new(prompt));
                    let d = E::Val(V::Int(self.rng.int(0, 100)));
                    (E::Endorse(self.rng.below(self.npol), Box::new(infer), Box::new(d)), Bind::Data(Ty::Int, Lab::E))
                }
                3 | 4 => {
                    let op = [Op::Add, Op::Sub, Op::Le, Op::Eq][self.rng.below(4)];
                    let (a, b) = (self.atom(&Ty::Int), self.atom(&Ty::Int));
                    let l = [&a, &b]
                        .iter()
                        .map(|x| match x {
                            E::Var(k) => match get(&self.ctx, *k) {
                                Some(Bind::Data(_, l)) => *l,
                                _ => Lab::T,
                            },
                            _ => Lab::T,
                        })
                        .fold(Lab::T, Lab::meet);
                    (E::Prim(op, Box::new(a), Box::new(b)), Bind::Data(op.ty(), l))
                }
                5 | 6 if !live.is_empty() => {
                    let pos = live.remove(self.rng.below(live.len()));
                    let prefer = self.rng.chance(4, 5);
                    (self.consume(pos, prefer), Bind::Data(Ty::Unit, Lab::T))
                }
                7 if depth < 3 => {
                    let c = self.atom(&Ty::Bool);
                    let lc = match &c {
                        E::Var(k) => match get(&self.ctx, *k) {
                            Some(Bind::Data(_, l)) => *l,
                            _ => Lab::T,
                        },
                        _ => Lab::T,
                    };
                    let mut inner = Vec::new();
                    live.retain(|&p| {
                        if self.rng.chance(1, 2) {
                            inner.push(p);
                            false
                        } else {
                            true
                        }
                    });
                    let t = self.any_ty();
                    let a = self.block(inner.clone(), depth + 1, &t);
                    let f = self.block(inner, depth + 1, &t);
                    (E::Ite(Box::new(c), Box::new(a), Box::new(f)), Bind::Data(t, lc))
                }
                8 if depth < 2 => {
                    let len = 1 + self.rng.below(3);
                    let xs: Vec<V> = (0..len).map(|_| V::Int(self.rng.int(0, 100))).collect();
                    self.ctx.push(Bind::Data(Ty::Int, Lab::T));
                    let t = self.any_ty();
                    let body = self.block(vec![], depth + 1, &t);
                    self.ctx.pop();
                    (E::For(len, Box::new(E::Val(V::List(xs))), Box::new(body)), Bind::Data(Ty::Unit, Lab::T))
                }
                10 if self.saga => (E::Await(self.rng.below(self.nprobes)), Bind::Data(Ty::Unit, Lab::T)),
                11 | 12 if depth < 2 => {
                    // a saga: o corpo consome uma parte das capacidades; os dois tratadores
                    // consomem outra, a mesma (a saída revoga o que o caminho não usou)
                    let (mut body, mut handlers) = (Vec::new(), Vec::new());
                    live.retain(|&p| match self.rng.below(4) {
                        0 | 1 => {
                            body.push(p);
                            false
                        }
                        2 => {
                            handlers.push(p);
                            false
                        }
                        _ => true,
                    });
                    let t = self.any_ty();
                    let outer = std::mem::replace(&mut self.saga, true);
                    self.tail = true;
                    let b = self.block(body, depth + 1, &t);
                    self.saga = outer;
                    let f = self.block(handlers.clone(), depth + 1, &t);
                    let g = self.block(handlers, depth + 1, &t);
                    let r = self.rng.below(self.nprobes);
                    (E::Atomic(Box::new(b), r, Box::new(f), Box::new(g)), Bind::Data(t, Lab::E))
                }
                _ => continue,
            };
            stmts.push(s);
            self.ctx.push(b);
        }
        // as capacidades que restam: cada uma por uma ação (de preferência) ou um `drop`
        while !live.is_empty() {
            let pos = live.remove(self.rng.below(live.len()));
            let prefer = self.rng.chance(3, 4);
            stmts.push(self.consume(pos, prefer));
            self.ctx.push(Bind::Data(Ty::Unit, Lab::T));
        }
        // o corpo de uma saga termina, em geral, observando uma pós-condição
        if tail && self.rng.chance(3, 4) {
            stmts.push(E::Await(self.rng.below(self.nprobes)));
            self.ctx.push(Bind::Data(Ty::Unit, Lab::T));
        }
        let mut e = self.result(want);
        self.ctx.truncate(base);
        for s in stmts.into_iter().rev() {
            e = E::Let(Box::new(s), Box::new(e));
        }
        e
    }
}

/// Teto estático de chamadas ao oráculo (o roteiro precisa cobrir o pior caminho).
fn calls(e: &E) -> usize {
    match e {
        E::Val(_) | E::Var(_) | E::Drop(_) => 0,
        E::Let(a, b) | E::Prim(_, a, b) => calls(a) + calls(b),
        E::Ite(c, t, f) => calls(c) + calls(t).max(calls(f)),
        E::Endorse(_, a, d) => calls(a) + calls(d),
        E::Act(_, _, a) => calls(a),
        E::For(n, xs, body) => calls(xs) + n * calls(body),
        E::Infer(_, _, a) => 1 + calls(a),
        E::Await(_) => 0,
        E::Atomic(body, _, f, g) => calls(body) + calls(f).max(calls(g)),
        E::Call(_, _, a) => calls(a),
        E::Transfer(_) => 0,
    }
}

/// O programa da semente `seed`, no fragmento sem sagas.
pub fn generate(seed: u64) -> Prog {
    generate_with(seed, false, false)
}

/// O programa da semente `seed`, com sagas: ferramentas de célula (algumas `compensable`,
/// algumas com compensação recusada), sondas de intervalo, `atomic` e `await`.
pub fn generate_saga(seed: u64) -> Prog {
    generate_with(seed, true, false)
}

/// O programa da semente `seed`, com sagas e chamadas com chave: cada capacidade tem uma
/// ferramenta com chave (provedor `idempotent`, `queryable` ou `opaque`, falhas injetadas na
/// chamada e nos reenvios, consultas inconclusivas), e metade das capacidades é consumida por
/// ela. O tratador de `TimeoutUnknown` reconcilia com o combustível do programa e, esgotado,
/// transfere a obrigação e encerra o evento.
pub fn generate_keyed(seed: u64) -> Prog {
    generate_with(seed, true, true)
}

fn key_spec(rng: &mut Rng, cap: usize) -> KeySpec {
    let provider = [Prov::Idempotent, Prov::Idempotent, Prov::Queryable, Prov::Queryable, Prov::Opaque][rng.below(5)];
    let mut faults = Vec::new();
    match rng.below(9) {
        0 | 1 => {}
        2 => faults.push((1, "error")),
        3..=5 => faults.push((1, "timeout_lost")),
        _ => faults.push((1, "timeout_applied")),
    }
    if provider == Prov::Idempotent {
        for n in 2..=4 {
            match rng.below(5) {
                0 => {}
                1 | 2 => faults.push((n, "timeout_lost")),
                3 => faults.push((n, "timeout_applied")),
                _ => faults.push((n, "error")),
            }
        }
    }
    let unknown = if provider == Prov::Queryable { rng.below(4) as u32 } else { 0 };
    KeySpec { cap, provider, faults, unknown }
}

fn generate_with(seed: u64, sagas: bool, keyed: bool) -> Prog {
    let mut rng = Rng::new(seed);
    let ncaps = 1 + rng.below(3) + usize::from(sagas && rng.chance(1, 2));
    let effects = ncaps + rng.below(2);
    let caps: Vec<Vec<usize>> = (0..ncaps)
        .map(|i| {
            let mut e: Vec<usize> = (0..effects).filter(|&x| x == i || rng.chance(1, 4)).collect();
            e.sort();
            e
        })
        .collect();
    let ntools = 1 + rng.below(4);
    let ncells = 1 + rng.below(2);
    let tools: Vec<Tool> = (0..ntools)
        .map(|_| {
            let effect = rng.below(effects);
            let arg = if rng.chance(3, 4) { Ty::Int } else { Ty::Bool };
            let mut t = Tool { effect, arg, cell: None, compensable: false, undo_fails: false, key: None };
            if sagas && t.arg == Ty::Int && rng.chance(3, 4) {
                t.cell = Some(rng.below(ncells));
                t.compensable = rng.chance(4, 5);
                t.undo_fails = t.compensable && rng.chance(1, 3);
            }
            t
        })
        .collect();
    let mut tools = tools;
    if sagas {
        // toda capacidade autoriza ao menos uma ferramenta compensável: o efeito `i` está
        // sempre em `caps[i]`
        for e in 0..ncaps {
            let cell = Some(rng.below(ncells));
            tools.push(Tool {
                effect: e,
                arg: Ty::Int,
                cell,
                compensable: true,
                undo_fails: rng.chance(1, 3),
                key: None,
            });
        }
    }
    // com chaves: a ferramenta com chave da capacidade `i` (efeito `i`), depois de todas as
    // outras, para não mudar a sequência pseudoaleatória dos outros fragmentos
    let keyed_base = tools.len();
    if keyed {
        for i in 0..ncaps {
            let spec = key_spec(&mut rng, i);
            let cell = rng.chance(3, 4).then(|| rng.below(ncells));
            let compensable = cell.is_some() && rng.chance(1, 2);
            let undo_fails = compensable && rng.chance(1, 3);
            tools.push(Tool { effect: i, arg: Ty::Int, cell, compensable, undo_fails, key: Some(spec) });
        }
    }
    let probes: Vec<Probe> = if sagas {
        (0..1 + rng.below(3))
            .map(|_| {
                let cell = rng.below(ncells);
                if rng.chance(1, 3) {
                    // a recuperação típica: a célula voltou a zero
                    Probe { cell, lo: 0, hi: 0 }
                } else {
                    let lo = rng.int(0, 80);
                    Probe { cell, lo, hi: rng.int(lo, 160) }
                }
            })
            .collect()
    } else {
        vec![]
    };
    let npol = 1 + rng.below(3);
    let policies: Vec<Policy> = (0..npol)
        .map(|_| {
            let lo = rng.int(0, 60);
            Policy { lo, hi: rng.int(lo, 100) }
        })
        .collect();
    let ret = [Ty::Int, Ty::Bool, Ty::Unit][rng.below(3)].clone();
    let mut g = Gen {
        rng: &mut rng,
        tools: &tools,
        npol,
        ctx: caps.iter().map(|e| Bind::Cap(e.clone())).collect(),
        nprobes: probes.len(),
        saga: false,
        tail: false,
        keyed: keyed.then_some(keyed_base),
    };
    let live: Vec<usize> = (0..ncaps).collect();
    let exp = g.block(live, 0, &ret);
    // metade das respostas cai na fronteira de alguma política (lo − 1, lo, hi, hi + 1): é ali
    // que um `≤` trocado por `<` em qualquer backend apareceria
    let oracle = (0..calls(&exp))
        .map(|_| {
            if rng.chance(1, 2) {
                let p = &policies[rng.below(policies.len())];
                [p.lo - 1, p.lo, p.hi, p.hi + 1][rng.below(4)].clamp(0, 100)
            } else {
                rng.int(0, 100)
            }
        })
        .collect();
    let (fuel, hval) = if keyed {
        let fuel = 1 + rng.below(3);
        let hval = match ret {
            Ty::Int => V::Int(rng.int(0, 100)),
            Ty::Bool => V::Bool(rng.chance(1, 2)),
            _ => V::Unit,
        };
        (fuel, hval)
    } else {
        (0, V::Unit)
    };
    Prog { seed, effects, tools, policies, caps, oracle, exp, ret, probes, fuel, hval }
}

// ------------------------------------------------------------------ JSON para o `ath-lean-eval`

fn ty_json(t: &Ty) -> Json {
    match t {
        Ty::Int => json!("int"),
        Ty::Bool => json!("bool"),
        Ty::Unit => json!("unit"),
        Ty::List(t, n) => json!({ "list": [ty_json(t), n] }),
    }
}

fn val_json(v: &V) -> Json {
    match v {
        V::Int(n) => json!({ "int": n }),
        V::Bool(b) => json!({ "bool": b }),
        V::Unit => json!("unit"),
        V::List(vs) => json!({ "list": vs.iter().map(val_json).collect::<Vec<_>>() }),
    }
}

fn exp_json(e: &E, h: &(usize, Json)) -> Json {
    let exp_json = |x: &E| exp_json(x, h);
    match e {
        E::Val(v) => json!({ "val": val_json(v) }),
        E::Var(k) => json!({ "var": k }),
        E::Let(a, b) => json!({ "let": [exp_json(a), exp_json(b)] }),
        E::Ite(c, t, f) => json!({ "ite": [exp_json(c), exp_json(t), exp_json(f)] }),
        E::Prim(op, a, b) => json!({ "prim": [op.name(), exp_json(a), exp_json(b)] }),
        E::Endorse(p, a, d) => json!({ "endorse": [p, exp_json(a), exp_json(d)] }),
        E::Act(t, k, a) => json!({ "act": [t, k, exp_json(a)] }),
        E::Drop(k) => json!({ "drop": k }),
        E::For(n, xs, b) => json!({ "for": [n, exp_json(xs), exp_json(b)] }),
        E::Infer(m, t, a) => json!({ "infer": [m, ty_json(t), exp_json(a)] }),
        E::Await(q) => json!({ "await": q }),
        E::Atomic(b, r, f, g) => json!({ "atomic": [exp_json(b), r, exp_json(f), exp_json(g)] }),
        E::Call(t, k, a) => json!({ "call": [t, k, exp_json(a), h.0, h.1] }),
        E::Transfer(p) => json!({ "transfer": p }),
    }
}

impl Prog {
    pub fn effect_name(i: usize) -> String {
        format!("Core.E{i}")
    }

    pub fn tool_names(&self) -> Vec<String> {
        (0..self.tools.len()).map(|t| format!("t{t}")).collect()
    }

    /// O programa no formato de `lean/Aletheia/Exec.lean`.
    pub fn to_json(&self) -> Json {
        json!({
            "effects": (0..self.effects).map(Prog::effect_name).collect::<Vec<_>>(),
            "tools": self.tools.iter().enumerate().map(|(i, t)| {
                let mut j = json!({
                    "name": format!("t{i}"), "effect": t.effect, "arg": ty_json(&t.arg),
                    "cell": t.cell, "compensable": t.compensable, "undo_fails": t.undo_fails,
                });
                if let Some(k) = &t.key {
                    j["keyed"] = json!(true);
                    j["provider"] = json!(k.provider.name());
                }
                j
            }).collect::<Vec<_>>(),
            "probes": self.probes.iter().enumerate().map(|(i, p)| json!({ "name": format!("p{i}"), "cell": p.cell, "lo": p.lo, "hi": p.hi })).collect::<Vec<_>>(),
            "policies": self.policies.iter().enumerate().map(|(i, p)| json!({ "name": format!("P{i}"), "lo": p.lo, "hi": p.hi })).collect::<Vec<_>>(),
            "grants": self.caps,
            // ρ inicial em ordem de de Bruijn: o índice 0 é a última capacidade
            "env": (0..self.caps.len()).rev().map(|id| json!({ "cap": id })).collect::<Vec<_>>(),
            "oracle": self.oracle,
            "exp": exp_json(&self.exp, &(self.fuel, exp_json(&self.handler(), &(0, Json::Null)))),
            "replies": self.key_runs().iter().map(|(_, cap, r)| json!({ "key": cap, "replies": r.replies })).collect::<Vec<_>>(),
            "applied": self.key_runs().iter().filter(|(_, _, r)| r.applied).map(|(_, cap, _)| cap).collect::<Vec<_>>(),
        })
    }

    /// O mundo dos runtimes, de latência zero: cada ferramenta sem célula é um sumidouro; as
    /// de célula somam o argumento, as compensações `u<i>` subtraem (ou são recusadas) e as
    /// sondas `p<q>` observam um intervalo.
    pub fn world(&self) -> Json {
        let names = self.tool_names();
        let mut tools = serde_json::Map::new();
        let mut lat = serde_json::Map::new();
        let mut sinks = Vec::new();
        for (i, t) in self.tools.iter().enumerate() {
            lat.insert(names[i].clone(), json!(0.0));
            match t.cell {
                Some(c) => {
                    tools.insert(names[i].clone(), json!({ "cell": c, "sign": 1 }));
                    if t.compensable {
                        tools.insert(format!("u{i}"), json!({ "cell": c, "sign": -1, "fails": t.undo_fails }));
                        lat.insert(format!("u{i}"), json!(0.0));
                    }
                }
                None => sinks.push(names[i].clone()),
            }
        }
        let probes: serde_json::Map<_, _> = self
            .probes
            .iter()
            .enumerate()
            .map(|(i, p)| (format!("p{i}"), json!({ "cell": p.cell, "lo": p.lo, "hi": p.hi })))
            .collect();
        for i in 0..self.probes.len() {
            lat.insert(format!("p{i}"), json!(0.0));
        }
        let mut faults = Vec::new();
        for (i, t) in self.tools.iter().enumerate() {
            if let Some(k) = &t.key {
                for (nth, kind) in &k.faults {
                    faults.push(json!({ "tool": names[i], "nth": nth, "kind": kind }));
                }
                if k.unknown > 0 {
                    faults.push(json!({ "tool": names[i], "kind": "unknown_status", "times": k.unknown }));
                }
            }
        }
        let mut w = json!({ "sinks": sinks, "latency": lat, "cells": { "tools": tools, "probes": probes } });
        if !faults.is_empty() {
            w["faults"] = json!(faults);
        }
        w
    }

    pub fn has_saga(&self) -> bool {
        fn go(e: &E) -> bool {
            match e {
                E::Atomic(..) => true,
                E::Val(_) | E::Var(_) | E::Drop(_) | E::Await(_) | E::Transfer(_) => false,
                E::Let(a, b) | E::Prim(_, a, b) | E::Endorse(_, a, b) => go(a) || go(b),
                E::Ite(c, t, f) => go(c) || go(t) || go(f),
                E::Act(_, _, a) | E::Infer(_, _, a) | E::Call(_, _, a) => go(a),
                E::For(_, xs, b) => go(xs) || go(b),
            }
        }
        go(&self.exp)
    }

    pub fn models(&self) -> Json {
        json!({ "m0": self.oracle })
    }
}

// ------------------------------------------------------------------ tradução para a superfície

fn surface_ty(t: &Ty) -> &'static str {
    match t {
        Ty::Int => "Int",
        Ty::Bool => "Bool",
        Ty::Unit => "Unit",
        Ty::List(..) => unreachable!("listas só aparecem como iteráveis literais"),
    }
}

fn caps_ty(eff: &[usize]) -> String {
    eff.iter().map(|&e| Prog::effect_name(e)).collect::<Vec<_>>().join(" + ")
}

struct Tr {
    fresh: usize,
    /// O nome de cada ligação de Γ (o último é o índice 0); `None` para valores `()`.
    names: Vec<Option<String>>,
}

impl Tr {
    fn name(&self, k: usize) -> Option<String> {
        self.names[self.names.len() - 1 - k].clone()
    }

    fn fresh(&mut self) -> String {
        self.fresh += 1;
        format!("x{}", self.fresh)
    }

    fn lit(v: &V) -> String {
        match v {
            V::Int(n) => n.to_string(),
            V::Bool(b) => b.to_string(),
            V::List(vs) => format!("[{}]", vs.iter().map(Tr::lit).collect::<Vec<_>>().join(", ")),
            V::Unit => unreachable!("`()` não tem literal na superfície"),
        }
    }

    fn atom(&self, e: &E) -> String {
        match e {
            E::Val(v) => Tr::lit(v),
            E::Var(k) => self.name(*k).expect("átomo de tipo `()`"),
            other => panic!("não é átomo: {other:?}"),
        }
    }

    /// Um bloco: linhas e, se o tipo não é `()`, a expressão final.
    fn block(&mut self, e: &E, ind: usize) -> (Vec<String>, Option<String>) {
        let base = self.names.len();
        let mut lines = Vec::new();
        let mut cur = e;
        while let E::Let(a, b) = cur {
            let n = self.stmt(a, ind, &mut lines);
            self.names.push(n);
            cur = b;
        }
        let fin = match cur {
            E::Val(V::Unit) => None,
            E::Val(_) | E::Var(_) => self.name_or_lit(cur),
            E::Prim(op, a, b) => Some(format!("{} {} {}", self.atom(a), op.surface(), self.atom(b))),
            other => self.stmt(other, ind, &mut lines),
        };
        self.names.truncate(base);
        (lines, fin)
    }

    fn name_or_lit(&self, e: &E) -> Option<String> {
        match e {
            E::Var(k) => self.name(*k),
            E::Val(V::Unit) => None,
            E::Val(v) => Some(Tr::lit(v)),
            _ => unreachable!(),
        }
    }

    /// O bloco entre chaves e se o seu valor é `()` (sem expressão final).
    fn braces(&mut self, e: &E, ind: usize) -> (String, bool) {
        let (lines, fin) = self.block(e, ind + 1);
        let unit = fin.is_none();
        let pad = "  ".repeat(ind + 1);
        let mut body: Vec<String> = lines;
        body.extend(fin.map(|f| format!("{pad}{f}")));
        let text =
            if body.is_empty() { "{ }".into() } else { format!("{{\n{}\n{}}}", body.join("\n"), "  ".repeat(ind)) };
        (text, unit)
    }

    /// Um termo em posição de `let`: emite as linhas e devolve o nome do valor (`None` = `()`).
    fn stmt(&mut self, e: &E, ind: usize, out: &mut Vec<String>) -> Option<String> {
        let pad = "  ".repeat(ind);
        match e {
            E::Endorse(p, a, d) => {
                let E::Infer(_, Ty::Int, prompt) = a.as_ref() else { panic!("endosso fora de `refine infer`") };
                let x = self.fresh();
                out.push(format!(
                    "{pad}let {x} = refine infer m0 -> Int[0..100] {{ q = {} }} by P{p} retry 1 else {{ {} }}",
                    self.atom(prompt),
                    self.atom(d)
                ));
                Some(x)
            }
            E::Prim(op, a, b) => {
                let x = self.fresh();
                out.push(format!("{pad}let {x} = {} {} {}", self.atom(a), op.surface(), self.atom(b)));
                Some(x)
            }
            E::Act(t, k, a) => {
                out.push(format!("{pad}t{t}({}, {})", self.name(*k).expect("capacidade"), self.atom(a)));
                None
            }
            // a chave é cunhada no ponto da chamada: a ordem de cunhagem é a de execução
            E::Call(t, k, a) => {
                out.push(format!("{pad}t{t}({}, {}, keys.mint())", self.name(*k).expect("capacidade"), self.atom(a)));
                None
            }
            // a superfície ainda não tem `drop`: o descarte explícito do núcleo não emite evento
            E::Drop(_) => None,
            E::Ite(c, t, f) => {
                let cond = self.atom(c);
                let ((bt, unit), (bf, _)) = (self.braces(t, ind), self.braces(f, ind));
                if unit {
                    out.push(format!("{pad}if {cond} {bt} else {bf}"));
                    None
                } else {
                    let x = self.fresh();
                    out.push(format!("{pad}let {x} = if {cond} {bt} else {bf}"));
                    Some(x)
                }
            }
            E::For(_, xs, body) => {
                let x = self.fresh();
                let it = self.atom(xs);
                self.names.push(Some(x.clone()));
                let (b, _) = self.braces(body, ind);
                self.names.pop();
                out.push(format!("{pad}for {x} in {it} {b}"));
                None
            }
            E::Await(q) => {
                out.push(format!("{pad}await p{q}() within 0.0"));
                None
            }
            E::Atomic(body, r, f, g) => {
                let (bb, unit) = self.braces(body, ind);
                let (bf, _) = self.braces(f, ind);
                let (bg, _) = self.braces(g, ind);
                let (e, cf) = (self.fresh(), self.fresh());
                let saga = format!("atomic recover p{r}() {bb} on failure({e}) {bf} on compensation_failed({cf}) {bg}");
                if unit {
                    out.push(format!("{pad}{saga}"));
                    None
                } else {
                    let x = self.fresh();
                    out.push(format!("{pad}let {x} = {saga}"));
                    Some(x)
                }
            }
            E::Val(V::Unit) => None,
            E::Val(_) | E::Var(_) => {
                let v = self.name_or_lit(e)?;
                let x = self.fresh();
                out.push(format!("{pad}let {x} = {v}"));
                Some(x)
            }
            E::Let(..) | E::Infer(..) | E::Transfer(_) => panic!("fora da forma A-normal: {e:?}"),
        }
    }
}

impl Prog {
    /// O programa em Aletheia de superfície: o agente `A`, o handler `go`, o modelo `m0`.
    pub fn to_ath(&self) -> String {
        let mut s = format!("module core.s{}\n\n", self.seed);
        s.push_str("model m0 { price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 64  reasoning = 0  ttft = 0.0  per_tok = 0.0 }\n\n");
        for (i, t) in self.tools.iter().enumerate() {
            let e = Prog::effect_name(t.effect);
            let a = surface_ty(&t.arg);
            if let Some(k) = &t.key {
                let pv = k.provider.name();
                if t.compensable {
                    s.push_str(&format!(
                        "tool t{i}(c: Cap[{e}], v: {a} ^ Endorsed, k: own IdemKey) -> Unit ! {{ {e} }} compensable by u{i} provider {pv} latency 0.0\n"
                    ));
                    s.push_str(&format!(
                        "tool u{i}(c: Cap[{e}], v: {a} ^ Endorsed, k: own IdemKey) -> Unit ! {{ {e} }} reversible provider idempotent latency 0.0\n"
                    ));
                } else {
                    s.push_str(&format!(
                        "tool t{i}(c: Cap[{e}], v: {a} ^ Endorsed, k: own IdemKey) -> Unit ! {{ {e} }} reversible provider {pv} latency 0.0\n"
                    ));
                }
                continue;
            }
            if t.compensable {
                s.push_str(&format!(
                    "tool t{i}(c: Cap[{e}], v: {a} ^ Endorsed) -> Unit ! {{ {e} }} compensable by u{i} latency 0.0\n"
                ));
                s.push_str(&format!(
                    "tool u{i}(c: Cap[{e}], v: {a} ^ Endorsed) -> Unit ! {{ {e} }} reversible latency 0.0\n"
                ));
            } else {
                s.push_str(&format!(
                    "tool t{i}(c: Cap[{e}], v: {a} ^ Endorsed) -> Unit ! {{ {e} }} reversible latency 0.0\n"
                ));
            }
        }
        for q in 0..self.probes.len() {
            s.push_str(&format!("tool p{q}() -> Bool ! {{ }} reversible latency 0.0\n"));
        }
        s.push('\n');
        for (i, p) in self.policies.iter().enumerate() {
            s.push_str(&format!("policy P{i}(x: Int[0..100]) -> Bool = {} <= x && x <= {}\n", p.lo, p.hi));
        }
        let mut params: Vec<String> =
            self.caps.iter().enumerate().map(|(i, e)| format!("c{i}: Cap[{}]", caps_ty(e))).collect();
        let calls = self.has_calls();
        if calls {
            params.push("keys: Cap[IdemKey.Mint]".into());
        }
        let mut row: Vec<String> = vec!["Infer".into()];
        let mut effs: Vec<usize> = self.tools.iter().map(|t| t.effect).collect();
        effs.sort();
        effs.dedup();
        row.extend(effs.into_iter().map(Prog::effect_name));
        if self.has_saga() {
            row.push("CompensationFailed".into());
        }
        if calls {
            row.push("TimeoutUnknown".into());
        }
        s.push_str(&format!(
            "\nagent A({}) budget [tok <= 100000000, usd <= 1.0, wall <= 1000.0] {{\n  on go() -> {} ! {{ {} }} ",
            params.join(", "),
            surface_ty(&self.ret),
            row.join(", ")
        ));
        let mut tr = Tr { fresh: 0, names: (0..self.caps.len()).map(|i| Some(format!("c{i}"))).collect() };
        s.push_str(&tr.braces(&self.exp, 1).0);
        if calls {
            // o tratador do núcleo (`Prog::handler`): reconcilia; esgotado, transfere e encerra
            let fin = match &self.hval {
                V::Unit => String::new(),
                v => format!("\n      {}", Tr::lit(v)),
            };
            s.push_str(&format!(
                "\n\n  handle timeout_unknown(p: own Pending[_]) -> {} {{\n    reconcile p fuel {} else {{\n      transfer p{fin}\n    }}\n  }}",
                surface_ty(&self.ret),
                self.fuel
            ));
        }
        s.push_str("\n}\n");
        s
    }
}
