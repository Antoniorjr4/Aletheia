import Aletheia.Contracts

/-!
# M-I: o contrato levantado para o endosso (efeitos com prova)

`Contracts.lean` mostrou por que o contrato ficou na fronteira do efeito: o rótulo `Endorsed`
sobrevive à computação, o predicado da política não (`policyOk_but_not_contract`). A Rev13
devolve ao compilador a parte que dá para provar. Se o argumento de uma ação é **exatamente**
um valor endossado — a mesma variável, sem transformação no caminho — e a política do endosso
carrega a obrigação do contrato da ferramenta (a obrigação levantada: o runtime confere a
política e o contrato juntos no `refine`), então o contrato vale no efeito. É uma obrigação
**descarregada**: `política ∧ fatos ⇒ φ`, sem monitor.

`Disch` é a análise neste fragmento do λ_Ath (valores, variáveis, `let`, aritmética, `if`,
inferência, endosso, ação, `drop`); fora dele ela responde `false`, e o sítio fica com o monitor
dinâmico. `lifting_sound` é o teorema; `amplified_not_discharged` mostra que a análise recusa o
contraexemplo de `Contracts.lean`, e `lifted_example` que ela aceita a forma da Rev13 para
**toda** resposta do modelo.

A análise do `athc` (`crates/ath-syntax/src/obligations.rs`) faz o mesmo sobre a sintaxe de
superfície, com caminhos em ADTs e listas (`match`, `for`); essa extensão é verificada por
testes, não por este teorema.
-/

namespace Aletheia.Lifting

open Aletheia

variable (S : Sig) (φ : Nat → Val → Bool) (L : Nat → Nat → Bool)

/-- A variável ligada por esta expressão é um endosso pela política `p`, com um `else` literal
que a própria política aceita: o valor ligado satisfaz `p` em qualquer execução. -/
def slotOf : Exp → Option Nat
  | .endorse p _ (.val w) => if S.pol p w then some p else none
  | _ => none

/-- **A análise.** `Δ[j] = some p` diz que a variável `j` é um valor endossado pela política
`p`; `L t p` diz que `p` carrega a obrigação levantada do contrato de `t`. Uma ação está
descarregada se o argumento é uma dessas variáveis. -/
def Disch : List (Option Nat) → Exp → Bool
  | _, .val _ => true
  | _, .var _ => true
  | _, .drop _ => true
  | Δ, .let_ a b => Disch Δ a && Disch (slotOf S a :: Δ) b
  | Δ, .prim _ a b => Disch Δ a && Disch Δ b
  | Δ, .ite x t f => Disch Δ x && Disch Δ t && Disch Δ f
  | Δ, .infer _ _ a => Disch Δ a
  | Δ, .endorse _ e d => Disch Δ e && Disch Δ d
  | Δ, .act t _ (.var j) => match Δ[j]? with
    | some (some p) => L t p
    | _ => false
  | _, _ => false

/-- O ambiente honra a análise: cada variável marcada com `p` guarda um valor que `p` aceita. -/
def EnvOk (Δ : List (Option Nat)) (ρ : List Val) : Prop :=
  ∀ (j p : Nat), Δ[j]? = some (some p) → ∃ v, ρ[j]? = some v ∧ S.pol p v = true

/-- Todo efeito do traço satisfaz o contrato da sua ferramenta. -/
def ContractOk (tr : List Ev) : Prop := ∀ t id v, Ev.act t id v ∈ tr → φ t v = true

variable {S φ L}

theorem slotOf_pol {c : Cfg} {a : Exp} {p : Nat} (h : slotOf S a = some p) {ρ : List Val} {s : St} {o : Out}
    (he : eval S c a ρ s = .ok o) : S.pol p o.v = true := by
  cases a with
  | endorse p' e d =>
    cases d with
    | val w =>
      simp only [slotOf] at h
      by_cases hw : S.pol p' w = true
      · rw [if_pos hw] at h; injection h with h; subst h
        simp only [eval] at he
        cases h1 : eval S c e ρ s with
        | error x => rw [h1] at he; cases he
        | ok o₁ =>
          rw [h1] at he
          simp only [bnd_ok] at he
          by_cases hp : S.pol p' o₁.v = true
          · rw [if_pos hp] at he; simp only [pre_ok] at he; cases he; exact hp
          · rw [if_neg hp] at he
            simp only [eval, bnd_ok, pre_ok] at he
            cases he; exact hw
      · rw [if_neg hw] at h; cases h
    | _ => simp [slotOf] at h
  | _ => simp [slotOf] at h

theorem pre_err_ne_ok {α : Type} (tr : List Ev) (e : Err) (o : α) : pre tr (.error e) ≠ .ok o := by
  cases e <;> simp [pre]

theorem bnd_err {α β : Type} [HasTr α] (e : Err) (f : α → Except Err β) : bnd (.error e) f = .error e := rfl

theorem EnvOk.cons {Δ : List (Option Nat)} {ρ : List Val} (h : EnvOk S Δ ρ) (v : Val) :
    EnvOk S (none :: Δ) (v :: ρ) := by
  intro j p hj
  cases j with
  | zero => simp at hj
  | succ j => simpa using h j p (by simpa using hj)

theorem EnvOk.consP {Δ : List (Option Nat)} {ρ : List Val} (h : EnvOk S Δ ρ) {v : Val} {p : Nat}
    (hv : S.pol p v = true) : EnvOk S (some p :: Δ) (v :: ρ) := by
  intro j q hj
  cases j with
  | zero => simp at hj; subst hj; exact ⟨v, rfl, hv⟩
  | succ j => simpa using h j q (by simpa using hj)

theorem EnvOk.slot {Δ : List (Option Nat)} {ρ : List Val} (h : EnvOk S Δ ρ) {c : Cfg} {a : Exp} {s : St}
    {o : Out} (he : eval S c a ρ s = .ok o) : EnvOk S (slotOf S a :: Δ) (o.v :: ρ) := by
  cases hs : slotOf S a with
  | none => exact h.cons o.v
  | some p => exact h.consP (slotOf_pol hs he)

theorem contractOk_append {a b : List Ev} (ha : ContractOk φ a) (hb : ContractOk φ b) :
    ContractOk φ (a ++ b) := by
  intro t id v hm
  rcases List.mem_append.mp hm with h | h
  · exact ha t id v h
  · exact hb t id v h

theorem contractOk_nil : ContractOk φ [] := by intro _ _ _ h; simp at h

/-- **A obrigação levantada vale no efeito (`lifting_sound`).** Se cada política marcada por
`L` implica o contrato da ferramenta (é a política conferida junto com o contrato no `refine`),
todo programa do fragmento que a análise descarrega, avaliado num ambiente que a honra, só
executa efeitos que satisfazem o contrato — para qualquer resposta do modelo. -/
theorem lifting_sound (hL : ∀ t p v, L t p = true → S.pol p v = true → φ t v = true) (c : Cfg) :
    ∀ (e : Exp) (Δ : List (Option Nat)) (ρ : List Val) (s : St) (o : Out),
      Disch S L Δ e = true → EnvOk S Δ ρ → eval S c e ρ s = .ok o → ContractOk φ o.tr := by
  intro e
  induction e with
  | val v => intro Δ ρ s o _ _ he; simp only [eval] at he; cases he; exact contractOk_nil
  | var k =>
    intro Δ ρ s o _ _ he; simp only [eval] at he
    split at he
    · cases he; exact contractOk_nil
    · cases he
  | drop k =>
    intro Δ ρ s o _ _ he; simp only [eval, doDrop] at he
    split at he
    · split at he
      · cases he
      · cases he; exact contractOk_nil
    · cases he
  | let_ a b iha ihb =>
    intro Δ ρ s o hd henv he
    simp only [Disch, Bool.and_eq_true] at hd
    simp only [eval] at he
    cases h1 : eval S c a ρ s with
    | error x => rw [h1] at he; cases he
    | ok o₁ =>
      rw [h1] at he; simp only [bnd_ok] at he
      cases h2 : eval S c b (o₁.v :: ρ) o₁.st with
      | error x => rw [h2, bnd_err] at he; exact absurd he (pre_err_ne_ok _ _ _)
      | ok o₂ =>
        rw [h2] at he; simp only [bnd_ok, pre_ok] at he
        cases he
        exact contractOk_append (iha Δ ρ s o₁ hd.1 henv h1)
          (ihb _ _ _ o₂ hd.2 (henv.slot h1) h2)
  | prim op a b iha ihb =>
    intro Δ ρ s o hd henv he
    simp only [Disch, Bool.and_eq_true] at hd
    simp only [eval] at he
    cases h1 : eval S c a ρ s with
    | error x => rw [h1] at he; cases he
    | ok o₁ =>
      rw [h1] at he; simp only [bnd_ok] at he
      cases h2 : eval S c b ρ o₁.st with
      | error x => rw [h2, bnd_err] at he; exact absurd he (pre_err_ne_ok _ _ _)
      | ok o₂ =>
        rw [h2] at he; simp only [bnd_ok] at he
        split at he
        · simp only [pre_ok] at he; cases he
          exact contractOk_append (iha Δ ρ s o₁ hd.1 henv h1) (ihb Δ ρ _ o₂ hd.2 henv h2)
        · simp [pre] at he
  | ite x t f ihx iht ihf =>
    intro Δ ρ s o hd henv he
    simp only [Disch, Bool.and_eq_true] at hd
    simp only [eval] at he
    cases h1 : eval S c x ρ s with
    | error e => rw [h1] at he; cases he
    | ok o₁ =>
      rw [h1] at he; simp only [bnd_ok] at he
      split at he
      · rename_i b _
        cases hb : b
        · rw [hb] at he; simp only [cond] at he
          cases h2 : eval S c f ρ o₁.st with
          | error e => rw [h2, bnd_err] at he; exact absurd he (pre_err_ne_ok _ _ _)
          | ok o₂ =>
            rw [h2] at he; simp only [bnd_ok, pre_ok] at he; cases he
            exact contractOk_append (ihx Δ ρ s o₁ hd.1.1 henv h1) (ihf Δ ρ _ o₂ hd.2 henv h2)
        · rw [hb] at he; simp only [cond] at he
          cases h2 : eval S c t ρ o₁.st with
          | error e => rw [h2, bnd_err] at he; exact absurd he (pre_err_ne_ok _ _ _)
          | ok o₂ =>
            rw [h2] at he; simp only [bnd_ok, pre_ok] at he; cases he
            exact contractOk_append (ihx Δ ρ s o₁ hd.1.1 henv h1) (iht Δ ρ _ o₂ hd.1.2 henv h2)
      · simp [pre] at he
  | infer m t a iha =>
    intro Δ ρ s o hd henv he
    simp only [Disch] at hd
    simp only [eval] at he
    cases h1 : eval S c a ρ s with
    | error e => rw [h1] at he; cases he
    | ok o₁ =>
      rw [h1] at he; simp only [bnd_ok, doInfer] at he
      split at he
      · simp only [pre_ok] at he; cases he; exact iha Δ ρ s o₁ hd henv h1
      · simp [pre] at he
  | endorse p e d ihe ihd =>
    intro Δ ρ s o hd henv he
    simp only [Disch, Bool.and_eq_true] at hd
    simp only [eval] at he
    cases h1 : eval S c e ρ s with
    | error x => rw [h1] at he; cases he
    | ok o₁ =>
      rw [h1] at he; simp only [bnd_ok] at he
      split at he
      · simp only [pre_ok] at he; cases he
        refine contractOk_append (ihe Δ ρ s o₁ hd.1 henv h1) ?_
        intro t id v hm; simp at hm
      · cases h2 : eval S c d ρ o₁.st with
        | error x => rw [h2, bnd_err] at he; cases x <;> simp [pre] at he
        | ok o₂ =>
          rw [h2] at he; simp only [bnd_ok, pre_ok] at he; cases he
          refine contractOk_append (ihe Δ ρ s o₁ hd.1 henv h1) ?_
          intro t id v hm
          simp only [List.mem_cons] at hm
          rcases hm with hm | hm
          · cases hm
          · exact ihd Δ ρ _ o₂ hd.2 henv h2 t id v hm
  | act t k a iha =>
    intro Δ ρ s o hd henv he
    cases a with
    | var j =>
      simp only [Disch] at hd
      split at hd
      · rename_i p hp
        obtain ⟨v, hv, hpol⟩ := henv j p hp
        simp only [eval, hv, bnd_ok, doAct] at he
        split at he
        · split at he
          · split at he
            · cases he
            · split at he
              · simp only [pre_ok] at he; cases he
                intro t' id' v' hm
                simp [actEvs] at hm
                rcases hm with ⟨rfl, -, rfl⟩
                exact hL _ p _ hd hpol
              · simp [pre] at he
          · simp [pre] at he
        · simp [pre] at he
      · cases hd
    | _ => simp [Disch] at hd
  | _ => intro Δ ρ s o hd; simp [Disch] at hd

/-! ## Os dois lados -/

open Aletheia.Examples in
/-- **A análise recusa o contraexemplo.** Em `amplified` o argumento da ação é `n + 64000`, um
cálculo sobre o valor endossado: a obrigação não é demonstrável e o monitor do efeito fica. -/
theorem amplified_not_discharged (L : Nat → Nat → Bool) :
    Disch S L [] ContractsExample.amplified = false := by
  simp [ContractsExample.amplified, Disch, slotOf]

/-- `let n = infer m(…); let v = endorse p n else 3; act t c v` — a forma da Rev13: o argumento
é o valor endossado. Se `p` carrega o contrato de `t` (`L t p`) e o `else` literal o satisfaz,
a análise descarrega a ação. -/
def liftedProg (p t k : Nat) : Exp :=
  .let_ (.infer 0 .int (.var 0)) (.let_ (.endorse p (.var 0) (.val (.int 3))) (.act t (k + 2) (.var 0)))

theorem lifted_discharged {p t k : Nat} (hp : S.pol p (.int 3) = true) (hl : L t p = true) :
    Disch S L [none] (liftedProg p t k) = true := by
  simp [liftedProg, Disch, slotOf, hp, hl]

/-- **Para toda resposta do modelo**, o efeito do programa da Rev13 satisfaz o contrato. -/
theorem lifted_example {p t k : Nat} (hL : ∀ t p v, L t p = true → S.pol p v = true → φ t v = true)
    (hp : S.pol p (.int 3) = true) (hl : L t p = true) (c : Cfg) (ρ : List Val) (s : St) (o : Out)
    (he : eval S c (liftedProg p t k) ρ s = .ok o) : ContractOk φ o.tr :=
  lifting_sound hL c _ [none] ρ s o (lifted_discharged hp hl) (fun j q h => by
    cases j with
    | zero => simp at h
    | succ j => simp at h) he

end Aletheia.Lifting
