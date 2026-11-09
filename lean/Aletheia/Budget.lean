/-!
# Orçamentos graduados

β é um vetor sobre as seis dimensões do julgamento de tipagem: tokens, USD, tempo,
energia, bytes transferidos e risco. Cada dimensão é medida em unidades inteiras (micro-USD,
milissegundos…). A soma e a ordem são ponto a ponto. A junção `⊔` é o máximo ponto a ponto
e gradua os ramos de um `if`. A multiplicação escalar `n • β` gradua um laço de no máximo
`n` iterações: é a regra E6, β(for) ⪯ n · β(corpo).
-/

namespace Aletheia

inductive Dim where
  | tok | usd | wall | energy | bytes | risk
  deriving DecidableEq, Repr

namespace Dim

def all : List Dim := [tok, usd, wall, energy, bytes, risk]

theorem mem_all (d : Dim) : d ∈ all := by cases d <;> simp [all]

end Dim

/-- Um orçamento: uma quantidade por dimensão. -/
def Budget := Dim → Nat

namespace Budget

instance : Add Budget := ⟨fun a b d => a d + b d⟩
instance : OfNat Budget 0 := ⟨fun _ => 0⟩
instance : LE Budget := ⟨fun a b => ∀ d, a d ≤ b d⟩

/-- Graduação de um laço: `n • β` é `β` repetido `n` vezes. -/
def scale (n : Nat) (a : Budget) : Budget := fun d => n * a d

infixr:73 " • " => scale

/-- Junção (máximo ponto a ponto). -/
def join (a b : Budget) : Budget := fun d => max (a d) (b d)

infixl:65 " ⊔ " => join

@[simp] theorem add_apply (a b : Budget) (d : Dim) : (a + b) d = a d + b d := rfl
@[simp] theorem zero_apply (d : Dim) : (0 : Budget) d = 0 := rfl
@[simp] theorem scale_apply (n : Nat) (a : Budget) (d : Dim) : (n • a) d = n * a d := rfl
@[simp] theorem join_apply (a b : Budget) (d : Dim) : (a ⊔ b) d = max (a d) (b d) := rfl

theorem le_def {a b : Budget} : a ≤ b ↔ ∀ d, a d ≤ b d := Iff.rfl

theorem le_refl (a : Budget) : a ≤ a := fun _ => Nat.le_refl _

theorem le_trans {a b c : Budget} (h₁ : a ≤ b) (h₂ : b ≤ c) : a ≤ c :=
  fun d => Nat.le_trans (h₁ d) (h₂ d)

theorem le_join_left (a b : Budget) : a ≤ a ⊔ b := fun d => by simp; omega
theorem le_join_right (a b : Budget) : b ≤ a ⊔ b := fun d => by simp; omega

/-- Verificação executável usada pelo avaliador antes de cada efeito (a reserva do ARX). -/
def ble (a b : Budget) : Bool := Dim.all.all fun d => decide (a d ≤ b d)

theorem ble_iff {a b : Budget} : ble a b = true ↔ a ≤ b := by
  simp only [ble, List.all_eq_true, decide_eq_true_eq]
  exact ⟨fun h d => h d (Dim.mem_all d), fun h d _ => h d⟩

/-- `Within s s' β`: o gasto passou de `s` para `s'` sem ultrapassar `β` em nenhuma dimensão. -/
def Within (s s' β : Budget) : Prop := ∀ d, s d ≤ s' d ∧ s' d ≤ s d + β d

theorem Within.refl (s β : Budget) : Within s s β := fun _ => ⟨Nat.le_refl _, by omega⟩

theorem Within.mono {s s' β β' : Budget} (h : Within s s' β) (hb : β ≤ β') : Within s s' β' :=
  fun d => ⟨(h d).1, by have := (h d).2; have := hb d; omega⟩

theorem Within.trans {s₁ s₂ s₃ β₁ β₂ : Budget} (h₁ : Within s₁ s₂ β₁) (h₂ : Within s₂ s₃ β₂) :
    Within s₁ s₃ (β₁ + β₂) := fun d => by
  have := h₁ d; have := h₂ d; simp; omega

end Budget

end Aletheia
