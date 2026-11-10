import Aletheia.Lattice
import Aletheia.Budget

/-!
# Sintaxe do núcleo λ_Ath

O subconjunto mecanizado corresponde ao que o `athc` verifica e o ARX executa no Marco Zero.
Ele tem dados de primeira ordem (inteiros, booleanos, unidade e listas com teto
`List[T, 0..n]`), capacidades lineares, o reticulado de integridade, orçamentos graduados,
inferência por um oráculo não confiável, endosso por política auditada e ações efetivas.
Variáveis usam índices de de Bruijn: o índice 0 é a ligação mais recente.

Uma capacidade não é um dado. Ela só existe no ambiente inicial (as capacidades do agente) e
tem o tipo `Cap[E]`, indexado pelo conjunto de efeitos `E` que autoriza. Não tem tipo de
valor: não pode ser copiada, guardada numa lista nem passada como argumento. Ela é linear
(multiplicidade 1): todo caminho de execução a consome exatamente uma vez, como a autoridade
de uma `act` ou por um `drop` explícito.
-/

namespace Aletheia

/-- Tipos de dados. `list t n` é `List[t, 0..n]`. -/
inductive Ty where
  | int
  | bool
  | unit
  | list (t : Ty) (n : Nat)
  deriving DecidableEq, Repr

/-- Valores de execução. `cap id` é a capacidade de identidade `id`. -/
inductive Val where
  | int (n : Int)
  | bool (b : Bool)
  | unit
  | list (vs : List Val)
  | cap (id : Nat)
  deriving Repr

/-- Tipagem de valores. Nenhum valor de dado é uma capacidade. -/
inductive HasTy : Val → Ty → Prop where
  | int  {n : Int} : HasTy (.int n) .int
  | bool {b : Bool} : HasTy (.bool b) .bool
  | unit : HasTy .unit .unit
  | list {vs : List Val} {t : Ty} {n : Nat} :
      (∀ v ∈ vs, HasTy v t) → vs.length ≤ n → HasTy (.list vs) (.list t n)

theorem HasTy.not_cap {v : Val} {t : Ty} (h : HasTy v t) (id : Nat) : v ≠ .cap id := by
  cases h <;> nofun

/-- Operações primitivas sobre inteiros. -/
inductive Op where
  | add | sub | le | eq
  deriving DecidableEq, Repr

def Op.ty : Op → Ty
  | .add | .sub => .int
  | .le | .eq => .bool

def Op.apply : Op → Int → Int → Val
  | .add, x, y => .int (x + y)
  | .sub, x, y => .int (x - y)
  | .le,  x, y => .bool (decide (x ≤ y))
  | .eq,  x, y => .bool (decide (x = y))

theorem Op.apply_ty (op : Op) (x y : Int) : HasTy (op.apply x y) op.ty := by
  cases op <;> constructor

/-- Expressões do núcleo. -/
inductive Exp where
  /-- literal escrito pelo autor do programa (Trusted) -/
  | val (v : Val)
  | var (k : Nat)
  /-- `let x = e₁ in e₂`; `e₂` vê `x` no índice 0 -/
  | let_ (e₁ e₂ : Exp)
  | ite (c t f : Exp)
  | prim (op : Op) (a b : Exp)
  /-- `refine e by p else d`: endosso de `e` pela política auditada `p` -/
  | endorse (p : Nat) (e d : Exp)
  /-- executa a ferramenta `t` com a autoridade da capacidade no índice `k`, consumindo-a -/
  | act (t : Nat) (k : Nat) (a : Exp)
  /-- descarta explicitamente a capacidade do índice `k`, sem efeito -/
  | drop (k : Nat)
  /-- `for x in xs { body }` sobre uma lista de no máximo `n` elementos -/
  | for_ (n : Nat) (xs body : Exp)
  /-- `infer m -> t { a }`: a saída do modelo é Untrusted -/
  | infer (m : Nat) (t : Ty) (a : Exp)
  deriving Repr

/-- Classe de efeito externo (identificador canônico atribuído pelo compilador, por exemplo
`Cluster.Write`). -/
abbrev EffectId := Nat

/-- Uma entrada do contexto de tipagem Γ: um dado com tipo e integridade, ou uma capacidade
linear `Cap[E]`. -/
inductive Bind where
  | data (t : Ty) (ι : Lab)
  | cap (E : List EffectId)
  deriving DecidableEq, Repr

/-- Eventos observáveis de uma execução. -/
inductive Ev where
  /-- a ferramenta `t` executou com a capacidade `id` e o argumento `v` -/
  | act (t : Nat) (id : Nat) (v : Val)
  /-- decisão de endosso pela política `p`: `some v` aceito, `none` recusado -/
  | endorse (p : Nat) (o : Option Val)
  deriving Repr

/-- A assinatura estática do programa: o contrato Δ das ferramentas e dos modelos, mais as
políticas auditadas. -/
structure Sig where
  /-- custo declarado de cada ferramenta -/
  cost  : Nat → Budget
  /-- custo máximo declarado de cada chamada de modelo -/
  mcost : Nat → Budget
  /-- tipo do argumento de cada ferramenta -/
  arg   : Nat → Ty
  /-- classe de efeito de cada ferramenta (a linha de efeitos da sua assinatura) -/
  effect : Nat → EffectId
  /-- políticas auditadas (decidíveis) -/
  pol   : Nat → Val → Bool

/-- O oráculo de inferência: `O i m t v` é a resposta da `i`-ésima chamada ao modelo `m`,
pedida no tipo `t`, com o prompt `v`. Ele é arbitrário e, portanto, adversarial. -/
def Oracle := Nat → Nat → Ty → Val → Val

/-- O ARX valida cada resposta contra o esquema do tipo pedido (decodificação restrita e
análise). Do lado formal, isso é a hipótese de que o oráculo devolve valores bem tipados. -/
def Oracle.Ok (O : Oracle) : Prop := ∀ i m t v, HasTy (O i m t v) t

/-- Configuração de uma execução: o oráculo, o teto de orçamento do agente e a tabela de
concessões do runtime. `grants id` lista os efeitos que a capacidade de identidade `id`
realmente concede. O host instala a tabela; nenhuma resposta de modelo a escreve. -/
structure Cfg where
  oracle : Oracle
  limit  : Budget
  grants : Nat → List EffectId

end Aletheia
