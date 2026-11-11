import Aletheia.Confinement

/-!
# Exemplos: o padrão de remediação no núcleo

O agente recebe um alerta `Untrusted` e pede ao modelo um número de réplicas. Ele endossa a
resposta pela política `SafeScale` (2 ≤ n ≤ 64) e escala o cluster com a autoridade da sua
capacidade `Cap[Cluster.Write]`. É o `remediate.ath` reduzido ao núcleo. Este arquivo verifica:

* o programa é bem tipado, linear e cabe no orçamento, então o T1 garante a execução e o T2
  garante que a única ação usou a capacidade certa, concedida, exatamente uma vez;
* as duas variantes perigosas não passam na tipagem: usar a saída do modelo direto como
  argumento de ação, e escolher a ação por um teste sobre dado `Untrusted`;
* uma ferramenta de efeito fora de `E` não tipa;
* o E1 (multiplicidade 1) recusa consumo duplo, consumo dentro de laço, consumo num só ramo
  e descarte implícito, e aceita o descarte explícito por `drop`;
* com a concessão revogada no runtime, a guarda recusa a ação: o programa não chega a agir.

Por fim, duas execuções concretas reproduzem o que a Groq fez com a injeção. Com o modelo
respondendo 0 (seguindo a injeção), o endosso recusa e o agente escala para o valor
conservador. Com o modelo respondendo 5, o endosso aceita. Os dois traços coincidem até a
decisão de endosso, como o T3 exige.
-/

namespace Aletheia.Examples

open Lab Budget

/-- Efeito 0: `Cluster.Write`. Efeito 1: `Cluster.Delete`. -/
def ClusterWrite : EffectId := 0
def ClusterDelete : EffectId := 1

/-- Ferramenta 0: `scale(cluster, n)`, efeito `Cluster.Write`. Ferramenta 1:
`delete(cluster, n)`, efeito `Cluster.Delete`. Política 0: `SafeScale`. Modelo 0: o
planejador. -/
def S : Sig where
  cost  := fun _ d => match d with | .wall => 1000 | _ => 0
  mcost := fun _ d => match d with | .tok => 1400 | .usd => 300 | .wall => 2000 | _ => 0
  arg   := fun _ => .int
  effect := fun t => if t = 0 then ClusterWrite else ClusterDelete
  pol   := fun _ v => match v with | .int n => decide (2 ≤ n ∧ n ≤ 64) | _ => false

/-- Γ = [alerta : Int ^ Untrusted, cluster : Cap[Cluster.Write]]. -/
def Γ : List Bind := [.data .int .U, .cap [ClusterWrite]]

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
      (.act (by decide) rfl (by decide) (.var rfl) (by decide)))

theorem remediate_linear : Linear Γ remediate := by
  intro k E hk
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
      | act _ _ _ ha hla =>
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
        | act hpc _ _ _ _ =>
          have := (low_meet.mp hpc).2
          exact not_low_U (low_meet.mp this).1

/-! ## O E2 recusa efeito fora de `Cap[E]` -/

/-- `delete` exige `Cluster.Delete`, que a capacidade `Cap[Cluster.Write]` não autoriza. -/
def deleting : Exp := .act 1 1 (.val (.int 3))

theorem deleting_rejected (pc : Lab) (τ : Ty) (ι : Lab) (β : Budget) :
    ¬ HasType S Γ pc deleting τ ι β := by
  intro h
  cases h with
  | act _ hk heff _ _ =>
    simp [Γ] at hk
    subst hk
    exact absurd heff (by decide)

/-! ## O E1 com multiplicidade 1 -/

/-- Duas ações com a mesma capacidade: duplicação. -/
def twice : Exp := .let_ (.act 0 1 (.val (.int 3))) (.act 0 2 (.val (.int 4)))

theorem twice_not_linear : ¬ Linear Γ twice := fun h => absurd (h 1 _ rfl) (by decide)

/-- Uma ação dentro de um laço: o número de iterações não é fixo. -/
def looped : Exp := .for_ 1 (.val (.list [.int 3])) (.act 0 2 (.var 0))

theorem looped_not_linear : ¬ Linear Γ looped := fun h => absurd (h 1 _ rfl) (by decide)

/-- Uma ação num só ramo: o outro caminho descartaria a capacidade sem consumo. -/
def oneBranch : Exp := .ite (.val (.bool true)) (.act 0 1 (.val (.int 3))) (.val .unit)

theorem oneBranch_not_linear : ¬ Linear Γ oneBranch := fun h => absurd (h 1 _ rfl) (by decide)

/-- Nenhum uso: descarte implícito. -/
theorem discard_not_linear : ¬ Linear Γ (.val .unit) := fun h => absurd (h 1 _ rfl) (by decide)

/-- O mesmo programa de um ramo, com o descarte explícito no outro, é linear. -/
def bothBranches : Exp := .ite (.val (.bool true)) (.act 0 1 (.val (.int 3))) (.drop 1)

theorem bothBranches_linear : Linear Γ bothBranches := by
  intro k E hk
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
def s₀ : St := ⟨[], 0, 0, fun _ => 0, []⟩
def limit : Budget := fun d => match d with | .tok => 20000 | .usd => 500000 | .wall => 120000 | _ => 0

/-- O runtime concede `Cluster.Write` à capacidade 7, e nada mais. -/
def grants : Nat → List EffectId := fun id => if id = 7 then [ClusterWrite] else []

/-- A mesma tabela com a concessão revogada. -/
def revoked : Nat → List EffectId := fun _ => []

def cfg (n : Int) : Cfg := ⟨canned n, limit, grants⟩

theorem start₀ : Start grants Γ ρ₀ s₀ where
  env := .data .int (.cap .nil)
  caps := by
    intro i j id hi hj
    rcases i with _ | _ | i <;> rcases j with _ | _ | j <;> simp_all [ρ₀]
  granted := by
    intro k E id hk hid x hx
    rcases k with _ | _ | k <;> simp_all [Γ, ρ₀, grants]
  fresh := by simp [s₀]

theorem fits : s₀.spent + βR ≤ limit := by
  intro d; cases d <;> decide

/-- Para qualquer resposta do modelo, a remediação executa sem erro e dentro do orçamento. -/
theorem remediate_runs (n : Int) : ∀ err, eval S (cfg n) remediate ρ₀ s₀ ≠ .error err := by
  intro err h
  obtain ⟨o, he, _⟩ := soundness_keyless (c := cfg n) (canned_ok n) remediate_typed remediate_linear
    rfl rfl start₀ fits
  rw [he] at h; cases h

/-- T2 aplicado: a ação usou `Cap[Cluster.Write]` léxica e concedida, e a capacidade 7 foi
consumida exatamente uma vez, para qualquer resposta do modelo. -/
theorem remediate_T2 (n : Int) :
    ∃ o new, eval S (cfg n) remediate ρ₀ s₀ = .ok o ∧ Lexical S Γ ρ₀ o.tr ∧
      Granted' S grants o.tr ∧ o.st.used = new ++ s₀.used ∧ new.Nodup ∧
      ∀ id : Nat, id ∈ new ↔ ∃ k : Nat, ρ₀[k]? = some (Val.cap id) :=
  T2 (c := cfg n) (canned_ok n) remediate_typed remediate_linear rfl rfl start₀ fits

/-- Com a concessão revogada, a guarda do runtime recusa a ação. -/
example : eval S ⟨canned 5, limit, revoked⟩ remediate ρ₀ s₀ = .error .noCap := rfl

/-! ## A injeção da Groq, no núcleo -/

/-- O modelo segue a injeção e responde 0. O endosso recusa, e o agente escala para 3. -/
example : (eval S (cfg 0) remediate ρ₀ s₀).map (·.tr) =
    .ok [.endorse 0 none, .act 0 7 (.int 3)] := rfl

/-- O modelo responde 5. O endosso aceita, e o agente escala para 5. -/
example : (eval S (cfg 5) remediate ρ₀ s₀).map (·.tr) =
    .ok [.endorse 0 (some (.int 5)), .act 0 7 (.int 5)] := rfl

/-- Os dois traços divergem exatamente na decisão de endosso, como o T3 exige. -/
example : Diverge [.endorse 0 none, .act 0 7 (.int 3)]
    [.endorse 0 (some (.int 5)), .act 0 7 (.int 5)] :=
  ⟨[], 0, none, some (.int 5), [.act 0 7 (.int 3)], [.act 0 7 (.int 5)], by simp, rfl, rfl⟩

end Aletheia.Examples
