import Aletheia.Noninterference

/-!
# Exemplos: o padrão de remediação no núcleo

O agente recebe um alerta `Untrusted` e pede ao modelo um número de réplicas. Ele endossa a
resposta pela política `SafeScale` (2 ≤ n ≤ 64) e escala o cluster com a autoridade da sua
capacidade. É o `remediate.ath` reduzido ao núcleo. Este arquivo verifica três coisas:

* o programa é bem tipado, linear e cabe no orçamento, então o T1 garante a execução;
* as duas variantes perigosas não passam na tipagem: usar a saída do modelo direto como
  argumento de ação, e escolher a ação por um teste sobre dado `Untrusted`;
* o consumo duplo de uma capacidade, inclusive num laço com teto 2, não passa no E1.

Por fim, duas execuções concretas reproduzem o que a Groq fez com a injeção. Com o modelo
respondendo 0 (seguindo a injeção), o endosso recusa e o agente escala para o valor
conservador. Com o modelo respondendo 5, o endosso aceita. Os dois traços coincidem até a
decisão de endosso, como o T3 exige.
-/

namespace Aletheia.Examples

open Lab Budget

/-- Ferramenta 0: `scale(cluster, n)`. Política 0: `SafeScale`. Modelo 0: o planejador. -/
def S : Sig where
  cost  := fun _ d => match d with | .wall => 1000 | _ => 0
  mcost := fun _ d => match d with | .tok => 1400 | .usd => 300 | .wall => 2000 | _ => 0
  arg   := fun _ => .int
  pol   := fun _ v => match v with | .int n => decide (2 ≤ n ∧ n ≤ 64) | _ => false

/-- Γ = [alerta : Int ^ Untrusted, cluster : Cap]. -/
def Γ : List Bind := [.data .int .U, .cap]

/-- ```
let plan = infer planner -> Int { alerta }
let n    = refine plan by SafeScale else 3
scale(cluster, n)
``` -/
def remediate : Exp :=
  .let_ (.infer 0 .int (.var 0))
    (.let_ (.endorse 0 (.var 0) (.val (.int 3)))
      (.act 0 3 (.var 0)))

def βR : Budget := (0 + S.mcost 0) + ((0 + 0) + (0 + S.cost 0))

theorem remediate_typed : HasType S Γ .T remediate .unit .T βR :=
  .let_ (.infer (.var rfl))
    (.let_ (.endorse (by decide) (.var rfl) (.val .int))
      (.act (by decide) rfl (.var rfl) (by decide)))

theorem remediate_linear : Linear Γ remediate := by
  intro k hk
  match k, hk with
  | 1, _ => decide

/-! ## As variantes perigosas não tipam -/

/-- Usar a resposta do modelo direto como argumento da ação. -/
def injected : Exp := .let_ (.infer 0 .int (.var 0)) (.act 0 2 (.var 0))

theorem injected_rejected (pc : Lab) (τ : Ty) (ι : Lab) (β : Budget) :
    ¬ HasType S Γ pc injected τ ι β := by
  intro h
  cases h with
  | let_ h₁ h₂ =>
    cases h₁ with
    | infer _ =>
      cases h₂ with
      | act _ _ ha hla =>
        cases ha with
        | var hk =>
          simp at hk
          obtain ⟨_, rfl⟩ := hk
          exact not_low_U hla

/-- Escolher se age por um teste sobre a resposta do modelo. -/
def steered : Exp :=
  .ite (.prim .le (.infer 0 .int (.var 0)) (.val (.int 5)))
    (.act 0 1 (.val (.int 4)))
    (.val .unit)

theorem steered_rejected (pc : Lab) (τ : Ty) (ι : Lab) (β : Budget) :
    ¬ HasType S Γ pc steered τ ι β := by
  intro h
  cases h with
  | ite hc ht _ =>
    cases hc with
    | prim ha _ =>
      cases ha with
      | infer _ =>
        cases ht with
        | act hpc _ _ _ =>
          have := (low_meet.mp hpc).2
          exact not_low_U (low_meet.mp this).1

/-! ## O E1 recusa consumo duplo -/

def twice : Exp := .let_ (.act 0 1 (.val (.int 3))) (.act 0 2 (.val (.int 4)))

theorem twice_not_linear : ¬ Linear Γ twice := fun h => absurd (h 1 rfl) (by decide)

/-- Um laço de teto 2 que consome a capacidade a cada iteração. -/
def looped : Exp := .for_ 2 (.val (.list [.int 3, .int 4])) (.act 0 2 (.var 0))

theorem looped_not_linear : ¬ Linear Γ looped := fun h => absurd (h 1 rfl) (by decide)

/-- Com teto 1, o mesmo laço é linear. -/
theorem looped_once_linear : Linear Γ (.for_ 1 (.val (.list [.int 3])) (.act 0 2 (.var 0))) := by
  intro k hk
  match k, hk with
  | 1, _ => decide

/-! ## T1 aplicado -/

/-- Um oráculo que responde sempre `n` quando pedem um inteiro. -/
def canned (n : Int) : Oracle := fun _ _ t _ =>
  match t with
  | .int => .int n
  | .bool => .bool false
  | .unit => .unit
  | .list _ _ => .list []

theorem canned_ok (n : Int) : (canned n).Ok := by
  intro i m t v
  cases t with
  | int => exact .int
  | bool => exact .bool
  | unit => exact .unit
  | list t k => exact .list (by simp) (by simp)

def ρ₀ : List Val := [.int 42, .cap 7]
def s₀ : St := ⟨[], 0, 0⟩
def limit : Budget := fun d => match d with | .tok => 20000 | .usd => 500000 | .wall => 120000 | _ => 0

theorem start₀ : Start Γ ρ₀ s₀ where
  env := .data .int (.cap .nil)
  caps := by
    intro i j id hi hj
    rcases i with _ | _ | i <;> rcases j with _ | _ | j <;> simp_all [ρ₀]
  fresh := by simp [s₀]

theorem fits : s₀.spent + βR ≤ limit := by
  intro d; cases d <;> decide

/-- Para qualquer resposta do modelo, a remediação executa sem erro e dentro do orçamento. -/
theorem remediate_runs (n : Int) :
    ∀ err, eval S ⟨canned n, limit⟩ remediate ρ₀ s₀ ≠ .error err :=
  progress (c := ⟨canned n, limit⟩) (canned_ok n) remediate_typed remediate_linear start₀ fits

/-! ## A injeção da Groq, no núcleo -/

/-- O modelo segue a injeção e responde 0. O endosso recusa, e o agente escala para 3. -/
example : (eval S ⟨canned 0, limit⟩ remediate ρ₀ s₀).map (·.tr) =
    .ok [.endorse 0 none, .act 0 7 (.int 3)] := rfl

/-- O modelo responde 5. O endosso aceita, e o agente escala para 5. -/
example : (eval S ⟨canned 5, limit⟩ remediate ρ₀ s₀).map (·.tr) =
    .ok [.endorse 0 (some (.int 5)), .act 0 7 (.int 5)] := rfl

/-- Os dois traços divergem exatamente na decisão de endosso, como o T3 exige. -/
example : Diverge [.endorse 0 none, .act 0 7 (.int 3)]
    [.endorse 0 (some (.int 5)), .act 0 7 (.int 5)] :=
  ⟨[], 0, none, some (.int 5), [.act 0 7 (.int 3)], [.act 0 7 (.int 5)], by simp, rfl, rfl⟩

end Aletheia.Examples
