import Lean.Data.Json
import Aletheia.Eval
import Aletheia.Blake3

/-!
# O avaliador formal como backend executável

Este módulo liga o avaliador canônico `eval` (o mesmo sobre o qual T1, T2 e T3 estão provados)
a um programa do núcleo descrito em JSON e escreve o **journal do núcleo**: os eventos
observáveis da execução (`act`, `endorse`) e o valor final (`end`), encadeados por BLAKE3 com a
mesma serialização canônica do ARX. O binário `ath-lean-eval` (`Main.lean`) expõe isso na
linha de comando, e `crates/athc/tests/differential.rs` compara o resultado, byte a byte, com a
vista do núcleo dos journals do ARX (Rust) e do `aletheia_runtime` (Python).

Nada aqui reimplementa a semântica: a execução é `eval S c e ρ s₀`, e o journal é uma função
pura do traço `Out.tr` e do valor `Out.v` que ela devolve.

## O formato do programa

```json
{ "effects":  ["Core.E0", ...],                       // nomes das classes de efeito
  "tools":    [{"name": "t0", "effect": 0, "arg": "int"}, ...],
  "policies": [{"name": "P0", "lo": 2, "hi": 64}, ...], // p(v) = lo ≤ v ≤ hi
  "grants":   [[0, 1], [1], ...],                      // efeitos concedidos à capacidade id
  "env":      [{"cap": 1}, {"cap": 0}],                // ρ inicial; índice 0 = mais recente
  "oracle":   [5, 1, 70, ...],                         // respostas do modelo, na ordem das chamadas
  "probes":   [{"name": "p0", "cell": 0, "lo": 1, "hi": 50}, ...],  // sondas: lo ≤ w[cell] ≤ hi
  "exp":      ... }
```

O mundo é `World := Nat → Int`, zerado no início. Uma ferramenta com `"cell": i` soma o
argumento à célula `i`; com `"compensable": true` ela entra no registro da saga, e a
compensação subtrai o argumento, ou é recusada se `"undo_fails": true`.

Expressões: `{"val": V}`, `{"var": k}`, `{"let": [e₁, e₂]}`, `{"ite": [c, t, f]}`,
`{"prim": ["add"|"sub"|"le"|"eq", a, b]}`, `{"endorse": [p, e, d]}`, `{"act": [t, k, a]}`,
`{"drop": k}`, `{"for": [n, xs, body]}`, `{"infer": [m, T, a]}`, `{"await": q}`,
`{"atomic": [corpo, r, f, g]}`. Valores: `{"int": n}`,
`{"bool": b}`, `"unit"`, `{"list": [V, ...]}`. Tipos: `"int"`, `"bool"`, `"unit"`,
`{"list": [T, n]}`.

## O journal do núcleo

Cada entrada é `{"detail", "hash", "kind", "prev", "seq"}`, com `hash = BLAKE3(corpo)` e o
corpo serializado com chaves ordenadas e sem espaços, como o `serde_json` do ARX:

* `act {args: [v], cap: [efeitos concedidos], tool}`;
* `endorse {accepted: true, policy, value: v}` ou `{accepted: false, policy, value: null}`;
* `fail {probe}`, `undo {args: [v], ok, tool}` e `recover {ok, probe}`, os eventos de saga;
* `end {value}`.
-/

namespace Aletheia.Exec

open Lean (Json)

/-! ## JSON canônico -/

/-- JSON de saída. Os objetos são renderizados com as chaves em ordem, como o `serde_json`
(`BTreeMap`) e o `canon.dumps` do runtime Python. -/
inductive J where
  | null
  | bool (b : Bool)
  | int (n : Int)
  | str (s : String)
  | arr (xs : List J)
  | obj (kvs : List (String × J))
  deriving Inhabited

def hex2 (n : Nat) : String :=
  let d := "0123456789abcdef"
  String.mk [d.get ⟨n / 16⟩, d.get ⟨n % 16⟩]

/-- O escape de strings do `serde_json`: aspas, barra invertida e caracteres de controle. -/
def escape (s : String) : String :=
  s.foldl (init := "") fun acc c =>
    match c with
    | '"' => acc ++ "\\\""
    | '\\' => acc ++ "\\\\"
    | '\n' => acc ++ "\\n"
    | '\r' => acc ++ "\\r"
    | '\t' => acc ++ "\\t"
    | c =>
      if c.toNat == 8 then acc ++ "\\b"
      else if c.toNat == 12 then acc ++ "\\f"
      else if c.toNat < 0x20 then acc ++ "\\u00" ++ hex2 c.toNat
      else acc.push c

def insertKey (kv : String × J) : List (String × J) → List (String × J)
  | [] => [kv]
  | x :: xs => if kv.1 < x.1 then kv :: x :: xs else x :: insertKey kv xs

def sortKeys (kvs : List (String × J)) : List (String × J) := kvs.foldr insertKey []

partial def render : J → String
  | .null => "null"
  | .bool b => if b then "true" else "false"
  | .int n => toString n
  | .str s => "\"" ++ escape s ++ "\""
  | .arr xs => "[" ++ ",".intercalate (xs.map render) ++ "]"
  | .obj kvs =>
    "{" ++ ",".intercalate ((sortKeys kvs).map fun (k, v) => "\"" ++ escape k ++ "\":" ++ render v) ++ "}"

/-! ## O programa -/

structure Tool where
  name : String
  effect : Nat
  arg : Ty
  /-- a célula do mundo que a ferramenta altera (`w[cell] += v`), se alguma -/
  cell : Option Nat := none
  /-- entra no registro de compensação da saga (`compensable by …`) -/
  compensable : Bool := false
  /-- o provedor recusa a compensação -/
  undoFails : Bool := false

/-- Sonda de célula: `lo ≤ w[cell] ≤ hi`. -/
structure Probe where
  name : String
  cell : Nat
  lo : Int
  hi : Int

structure Policy where
  name : String
  lo : Int
  hi : Int

structure Prog where
  effects : Array String
  tools : Array Tool
  policies : Array Policy
  grants : Array (List Nat)
  env : List Val
  oracle : Array Int
  exp : Exp
  probes : Array Probe := #[]

abbrev P := Except String

def field (j : Json) (k : String) : P Json := j.getObjVal? k

def nat (j : Json) : P Nat := j.getNat?
def int (j : Json) : P Int := j.getInt?
def str (j : Json) : P String := j.getStr?
def arr (j : Json) : P (Array Json) := j.getArr?

/-- O único par chave-valor de um objeto de forma `{"tag": payload}`. -/
def tagged (j : Json) : P (String × Json) :=
  match j with
  | .obj kvs =>
    match kvs.toArray with
    | #[⟨k, v⟩] => pure (k, v)
    | _ => throw s!"esperado um objeto de uma chave: {j.compress}"
  | _ => throw s!"esperado um objeto: {j.compress}"

partial def ty (j : Json) : P Ty := do
  match j with
  | .str "int" => pure .int
  | .str "bool" => pure .bool
  | .str "unit" => pure .unit
  | _ =>
    let (k, v) ← tagged j
    if k == "list" then
      let xs ← arr v
      if h : xs.size = 2 then return .list (← ty xs[0]) (← nat xs[1])
    throw s!"tipo inválido: {j.compress}"

partial def val (j : Json) : P Val := do
  if j == .str "unit" then return .unit
  let (k, v) ← tagged j
  match k with
  | "int" => return .int (← int v)
  | "bool" =>
    match v with
    | .bool b => return .bool b
    | _ => throw "booleano inválido"
  | "list" => return .list (← (← arr v).toList.mapM val)
  | "cap" => return .cap (← nat v)
  | _ => throw s!"valor inválido: {j.compress}"

def op (j : Json) : P Op := do
  match ← str j with
  | "add" => pure .add
  | "sub" => pure .sub
  | "le" => pure .le
  | "eq" => pure .eq
  | o => throw s!"operação inválida: {o}"

def args (v : Json) (n : Nat) : P (Array Json) := do
  let xs ← arr v
  if xs.size != n then throw s!"esperados {n} argumentos: {v.compress}"
  return xs

partial def exp (j : Json) : P Exp := do
  let (k, v) ← tagged j
  match k with
  | "val" => return .val (← val v)
  | "var" => return .var (← nat v)
  | "let" => let a ← args v 2; return .let_ (← exp a[0]!) (← exp a[1]!)
  | "ite" => let a ← args v 3; return .ite (← exp a[0]!) (← exp a[1]!) (← exp a[2]!)
  | "prim" => let a ← args v 3; return .prim (← op a[0]!) (← exp a[1]!) (← exp a[2]!)
  | "endorse" => let a ← args v 3; return .endorse (← nat a[0]!) (← exp a[1]!) (← exp a[2]!)
  | "act" => let a ← args v 3; return .act (← nat a[0]!) (← nat a[1]!) (← exp a[2]!)
  | "drop" => return .drop (← nat v)
  | "for" => let a ← args v 3; return .for_ (← nat a[0]!) (← exp a[1]!) (← exp a[2]!)
  | "infer" => let a ← args v 3; return .infer (← nat a[0]!) (← ty a[1]!) (← exp a[2]!)
  | "await" => return .await (← nat v)
  | "atomic" =>
    let a ← args v 4; return .atomic (← exp a[0]!) (← nat a[1]!) (← exp a[2]!) (← exp a[3]!)
  | _ => throw s!"expressão inválida: {k}"

def prog (j : Json) : P Prog := do
  let effects ← (← arr (← field j "effects")).mapM str
  let flag (t : Json) (k : String) : Bool :=
    match t.getObjVal? k with
    | .ok (.bool b) => b
    | _ => false
  let tools ← (← arr (← field j "tools")).mapM fun t => do
    let cell := match t.getObjVal? "cell" with
      | .ok c => (c.getNat?).toOption
      | .error _ => none
    return { name := ← str (← field t "name"), effect := ← nat (← field t "effect"),
             arg := ← ty (← field t "arg"), cell, compensable := flag t "compensable",
             undoFails := flag t "undo_fails" : Tool }
  let probes ← match j.getObjVal? "probes" with
    | .ok ps => (← arr ps).mapM fun q => do
        return { name := ← str (← field q "name"), cell := ← nat (← field q "cell"),
                 lo := ← int (← field q "lo"), hi := ← int (← field q "hi") : Probe }
    | .error _ => pure #[]
  let policies ← (← arr (← field j "policies")).mapM fun p => do
    return { name := ← str (← field p "name"), lo := ← int (← field p "lo"), hi := ← int (← field p "hi") }
  let grants ← (← arr (← field j "grants")).mapM fun g => do return (← (← arr g).mapM nat).toList
  let env ← (← arr (← field j "env")).mapM val
  let oracle ← (← arr (← field j "oracle")).mapM int
  return { effects, tools, policies, grants, env := env.toList, oracle, exp := ← exp (← field j "exp"),
           probes }

/-! ## A assinatura, o oráculo e a configuração -/

/-- A assinatura Δ do programa. Os custos são zero: o diferencial compara eventos e journal,
não as unidades de orçamento, que o ARX mede em tokens, dólares e segundos. -/
def Prog.sig (p : Prog) : Sig where
  cost := fun _ => 0
  mcost := fun _ => 0
  arg := fun t => (p.tools[t]?.map (·.arg)).getD .unit
  effect := fun t => (p.tools[t]?.map (·.effect)).getD 0
  pol := fun i v =>
    match p.policies[i]?, v with
    | some q, .int n => decide (q.lo ≤ n ∧ n ≤ q.hi)
    | _, _ => false
  step := fun t v w =>
    match (p.tools[t]?).bind (·.cell), v with
    | some i, .int n => fun j => if j = i then w j + n else w j
    | _, _ => w
  compensable := fun t => (p.tools[t]?.map (·.compensable)).getD false
  undo := fun t v w =>
    match p.tools[t]? with
    | some tl =>
      if tl.undoFails then none
      else
        match tl.cell, v with
        | some i, .int n => some fun j => if j = i then w j - n else w j
        | _, _ => some w
    | none => some w
  probe := fun q w =>
    match p.probes[q]? with
    | some pr => decide (pr.lo ≤ w pr.cell ∧ w pr.cell ≤ pr.hi)
    | none => false

/-- O oráculo roteirizado: a `i`-ésima chamada recebe a `i`-ésima resposta, no tipo pedido. -/
def Prog.oracle' (p : Prog) : Oracle := fun i _ t _ =>
  match t with
  | .int => .int (p.oracle[i]?.getD 0)
  | .bool => .bool ((p.oracle[i]?.getD 0) != 0)
  | .unit => .unit
  | .list _ _ => .list []

def Prog.cfg (p : Prog) : Cfg where
  oracle := p.oracle'
  limit := 0
  grants := fun id => p.grants[id]?.getD []

def St.init : St := ⟨[], 0, 0, fun _ => 0, []⟩

/-- A execução: exatamente o avaliador canônico. -/
def Prog.run (p : Prog) : Except Err Out := eval p.sig p.cfg p.exp p.env St.init

/-- Nenhuma camada entre o binário e a semântica: `run` é, por definição, o `eval` dos
teoremas, na assinatura, na configuração e no ambiente que o programa declara. -/
theorem Prog.run_eq (p : Prog) : p.run = eval p.sig p.cfg p.exp p.env St.init := rfl

/-! ## O journal do núcleo -/

partial def valJson : Val → J
  | .int n => .int n
  | .bool b => .bool b
  | .unit => .null
  | .list vs => .arr (vs.map valJson)
  | .cap id => .obj [("cap", .int id)]

def Prog.name (xs : Array String) (i : Nat) : J := .str (xs[i]?.getD s!"#{i}")

def sortStrs (xs : List String) : List String :=
  xs.foldr (fun x acc => insert x acc) []
where
  insert (x : String) : List String → List String
    | [] => [x]
    | y :: ys => if x < y then x :: y :: ys else y :: insert x ys

/-- O detalhe de cada evento do traço. `cap` é o conjunto de efeitos que o runtime concede à
capacidade usada, em ordem. -/
def Prog.event (p : Prog) : Ev → String × J
  | .act t id v =>
    let caps := sortStrs ((p.cfg.grants id).map fun e => p.effects[e]?.getD s!"#{e}")
    ("act", .obj [("args", .arr [valJson v]), ("cap", .arr (caps.map .str)),
      ("tool", Prog.name (p.tools.map (·.name)) t)])
  | .endorse q o =>
    ("endorse", .obj [("accepted", .bool o.isSome), ("policy", Prog.name (p.policies.map (·.name)) q),
      ("value", (o.map valJson).getD .null)])
  -- os eventos de saga, como a vista do núcleo do ARX os projeta
  | .fail q => ("fail", .obj [("probe", Prog.name (p.probes.map (·.name)) q)])
  | .undo t v ok =>
    ("undo", .obj [("args", .arr [valJson v]), ("ok", .bool ok), ("tool", Prog.name (p.tools.map (·.name)) t)])
  | .recover r ok => ("recover", .obj [("ok", .bool ok), ("probe", Prog.name (p.probes.map (·.name)) r)])
  -- o protocolo de chaves (ainda fora do tripé: o formato do programa não declara chaves)
  | .pending t k v =>
    ("pending", .obj [("args", .arr [valJson v]), ("key", .int k), ("tool", Prog.name (p.tools.map (·.name)) t)])
  | .query k i r =>
    ("query", .obj [("attempt", .int i), ("key", .int k), ("status", .str (match r with
      | .done => "done" | .failed => "failed" | .unknown => "unknown"))])
  | .settle k e =>
    ("settle", .obj [("key", .int k), ("status", .str (match e with
      | .done => "done" | .failed => "failed" | .transferred => "transferred"))])

def GENESIS : String := "".pushn '0' 64

/-- Encadeia as entradas: `hash = BLAKE3(corpo)`, `prev` = hash anterior. -/
def chain (evs : List (String × J)) : List String := Id.run do
  let mut prev := GENESIS
  let mut out := #[]
  for (kind, detail) in evs do
    let body := [("detail", detail), ("kind", .str kind), ("prev", .str prev), ("seq", .int out.size)]
    let h := Blake3.hashString (render (.obj body))
    out := out.push (render (.obj (("hash", .str h) :: body)))
    prev := h
  return out.toList

/-- O journal do núcleo de uma execução bem-sucedida. -/
def Prog.journal (p : Prog) (o : Out) : List String :=
  chain (o.tr.map p.event ++ [("end", .obj [("value", valJson o.v)])])

end Aletheia.Exec
