/-!
# Reticulado de integridade

`Untrusted ⊑ Endorsed ⊑ Trusted`, o reticulado do Marco Zero (`ath_syntax::ast::Label`).
O encontro `⊓` é o ínfimo; `low ℓ` (lê-se "ℓ ⊒ Endorsed") é a propriedade que ações e
endossos exigem.
-/

namespace Aletheia

inductive Lab where
  | U  -- Untrusted
  | E  -- Endorsed
  | T  -- Trusted
  deriving DecidableEq, Repr

namespace Lab

def rank : Lab → Nat
  | U => 0
  | E => 1
  | T => 2

/-- Ordem do reticulado: `a ⊑ b`. -/
def le (a b : Lab) : Prop := a.rank ≤ b.rank

instance : LE Lab := ⟨le⟩

instance (a b : Lab) : Decidable (a ≤ b) := inferInstanceAs (Decidable (a.rank ≤ b.rank))

/-- Encontro (ínfimo). -/
def meet : Lab → Lab → Lab
  | U, _ => U
  | _, U => U
  | E, _ => E
  | _, E => E
  | T, T => T

infixl:70 " ⊓ " => meet

/-- `ℓ ⊒ Endorsed`: integridade suficiente para decidir ações. -/
def low (ℓ : Lab) : Prop := ℓ ≠ U

theorem le_refl (a : Lab) : a ≤ a := Nat.le_refl _

theorem le_trans {a b c : Lab} (h₁ : a ≤ b) (h₂ : b ≤ c) : a ≤ c := Nat.le_trans h₁ h₂

theorem le_antisymm {a b : Lab} (h₁ : a ≤ b) (h₂ : b ≤ a) : a = b := by
  revert h₁ h₂; cases a <;> cases b <;> decide

theorem meet_le_left (a b : Lab) : a ⊓ b ≤ a := by
  cases a <;> cases b <;> decide

theorem meet_le_right (a b : Lab) : a ⊓ b ≤ b := by
  cases a <;> cases b <;> decide

/-- `⊓` é o ínfimo: a maior cota inferior. -/
theorem le_meet {a b c : Lab} (h₁ : c ≤ a) (h₂ : c ≤ b) : c ≤ a ⊓ b := by
  revert h₁ h₂; cases a <;> cases b <;> cases c <;> decide

theorem U_le (a : Lab) : U ≤ a := by cases a <;> decide

instance (a : Lab) : Decidable (low a) := inferInstanceAs (Decidable (a ≠ U))

theorem low_iff_E_le (a : Lab) : low a ↔ E ≤ a := by
  cases a <;> decide

theorem low_meet {a b : Lab} : low (a ⊓ b) ↔ low a ∧ low b := by
  cases a <;> cases b <;> decide

theorem not_low_U : ¬ low U := by decide

theorem low_E : low E := by decide

theorem low_T : low T := by decide

theorem meet_U_left (a : Lab) : U ⊓ a = U := by cases a <;> rfl

end Lab

end Aletheia
