import Aletheia.Syntax

/-!
# Julgamento de tipagem

`HasType S Γ pc e τ ι β` lê-se Γ; pc ⊢ e : τ ^ ι ▷ β. O valor de `e` tem tipo `τ` e
integridade `ι`; em contexto de controle de integridade `pc`, `e` gasta no máximo `β`.

Estes são os estratos do `athc` que o núcleo cobre:

* **E3 (integridade)**: o resultado de uma operação recebe o encontro dos rótulos. Ramificar
  ou iterar sobre um dado rebaixa `pc`. `act` e `endorse` exigem `pc ⊒ Endorsed`, e `act`
  exige também o argumento `⊒ Endorsed`. A saída de `infer` é sempre `Untrusted`.
* **E6 (orçamento)**: os custos somam em sequência e fazem junção nos ramos, e o laço
  multiplica: β(for) = β(xs) + n • β(corpo).
* **E1 (linearidade)**: `uses e k` conta quantas vezes `e` pode consumir a capacidade do
  índice `k`, com o laço multiplicando pelo teto `n`. `Linear Γ e` exige no máximo um
  consumo por capacidade.
-/

namespace Aletheia

open Lab Budget

inductive HasType (S : Sig) : List Bind → Lab → Exp → Ty → Lab → Budget → Prop where
  | val {Γ pc v τ} :
      HasTy v τ →
      HasType S Γ pc (.val v) τ .T 0
  | var {Γ pc k τ ι} :
      Γ[k]? = some (.data τ ι) →
      HasType S Γ pc (.var k) τ ι 0
  | let_ {Γ pc e₁ e₂ τ₁ τ₂ ι₁ ι₂ β₁ β₂} :
      HasType S Γ pc e₁ τ₁ ι₁ β₁ →
      HasType S (.data τ₁ ι₁ :: Γ) pc e₂ τ₂ ι₂ β₂ →
      HasType S Γ pc (.let_ e₁ e₂) τ₂ ι₂ (β₁ + β₂)
  | ite {Γ pc c t f τ ιc ιt ιf βc βt βf} :
      HasType S Γ pc c .bool ιc βc →
      HasType S Γ (pc ⊓ ιc) t τ ιt βt →
      HasType S Γ (pc ⊓ ιc) f τ ιf βf →
      HasType S Γ pc (.ite c t f) τ (ιc ⊓ (ιt ⊓ ιf)) (βc + (βt ⊔ βf))
  | prim {Γ pc op a b ιa ιb βa βb} :
      HasType S Γ pc a .int ιa βa →
      HasType S Γ pc b .int ιb βb →
      HasType S Γ pc (.prim op a b) op.ty (ιa ⊓ ιb) (βa + βb)
  | endorse {Γ pc p e d τ ιe ιd βe βd} :
      low pc →
      HasType S Γ pc e τ ιe βe →
      HasType S Γ pc d τ ιd βd →
      HasType S Γ pc (.endorse p e d) τ (.E ⊓ ιd) (βe + βd)
  | act {Γ pc t k a ιa βa} :
      low pc →
      Γ[k]? = some .cap →
      HasType S Γ pc a (S.arg t) ιa βa →
      low ιa →
      HasType S Γ pc (.act t k a) .unit .T (βa + S.cost t)
  | for_ {Γ pc n xs body τ τb ιx ιb βx βb} :
      HasType S Γ pc xs (.list τ n) ιx βx →
      HasType S (.data τ ιx :: Γ) (pc ⊓ ιx) body τb ιb βb →
      HasType S Γ pc (.for_ n xs body) .unit .T (βx + n • βb)
  | infer {Γ pc m t a τa ιa βa} :
      HasType S Γ pc a τa ιa βa →
      HasType S Γ pc (.infer m t a) t .U (βa + S.mcost m)

/-- Quantas vezes `e` pode consumir a capacidade do índice `k`. -/
def uses : Exp → Nat → Nat
  | .val _, _ => 0
  | .var _, _ => 0
  | .let_ e₁ e₂, k => uses e₁ k + uses e₂ (k + 1)
  | .ite c t f, k => uses c k + max (uses t k) (uses f k)
  | .prim _ a b, k => uses a k + uses b k
  | .endorse _ e d, k => uses e k + uses d k
  | .act _ j a, k => uses a k + (if j = k then 1 else 0)
  | .for_ n xs body, k => uses xs k + n * uses body (k + 1)
  | .infer _ _ a, k => uses a k

/-- E1: cada capacidade em Γ é consumida no máximo uma vez. -/
def Linear (Γ : List Bind) (e : Exp) : Prop := ∀ k, Γ[k]? = some .cap → uses e k ≤ 1

/-- O ambiente realiza o contexto: dados bem tipados nas posições de dado e capacidades nas
posições de capacidade. -/
inductive EnvOk : List Bind → List Val → Prop where
  | nil : EnvOk [] []
  | data {Γ ρ v τ ι} : HasTy v τ → EnvOk Γ ρ → EnvOk (.data τ ι :: Γ) (v :: ρ)
  | cap {Γ ρ id} : EnvOk Γ ρ → EnvOk (.cap :: Γ) (.cap id :: ρ)

theorem EnvOk.data_lookup {Γ : List Bind} {ρ : List Val} (h : EnvOk Γ ρ) :
    ∀ {k : Nat} {τ : Ty} {ι : Lab}, Γ[k]? = some (Bind.data τ ι) → ∃ v, ρ[k]? = some v ∧ HasTy v τ := by
  induction h with
  | nil => intro k τ ι hk; simp at hk
  | @data Γ ρ v τ' ι' hv _ ih =>
    intro k τ ι hk
    cases k with
    | zero => simp at hk; obtain ⟨rfl, rfl⟩ := hk; exact ⟨v, rfl, hv⟩
    | succ k => simpa using ih (by simpa using hk)
  | cap _ ih =>
    intro k τ ι hk
    cases k with
    | zero => simp at hk
    | succ k => simpa using ih (by simpa using hk)

theorem EnvOk.cap_lookup {Γ : List Bind} {ρ : List Val} (h : EnvOk Γ ρ) :
    ∀ {k : Nat}, Γ[k]? = some Bind.cap → ∃ id, ρ[k]? = some (Val.cap id) := by
  induction h with
  | nil => intro k hk; simp at hk
  | data _ _ ih =>
    intro k hk
    cases k with
    | zero => simp at hk
    | succ k => simpa using ih (by simpa using hk)
  | @cap Γ ρ id _ ih =>
    intro k hk
    cases k with
    | zero => exact ⟨id, rfl⟩
    | succ k => simpa using ih (by simpa using hk)

/-- Uma capacidade no ambiente está numa posição de capacidade de Γ. -/
theorem EnvOk.cap_pos {Γ : List Bind} {ρ : List Val} (h : EnvOk Γ ρ) :
    ∀ {k id : Nat}, ρ[k]? = some (Val.cap id) → Γ[k]? = some Bind.cap := by
  induction h with
  | nil => intro k id hk; simp at hk
  | data hv _ ih =>
    intro k id hk
    cases k with
    | zero => simp at hk; exact absurd hk (hv.not_cap id)
    | succ k => simpa using ih (by simpa using hk)
  | cap _ ih =>
    intro k id hk
    cases k with
    | zero => rfl
    | succ k => simpa using ih (by simpa using hk)

end Aletheia
