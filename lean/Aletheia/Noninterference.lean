import Aletheia.Soundness

/-!
# Teorema T3: não-interferência com endosso robusto

Integridade é o dual da confidencialidade: o que se protege é o dado confiável contra a
influência do não confiável. O ataque é este. Duas execuções começam de ambientes que
concordam em tudo o que é `⊒ Endorsed` e nas capacidades, mas podem diferir arbitrariamente
em toda entrada `Untrusted`. Os dois oráculos de inferência também são arbitrários e
independentes, porque um modelo é tão adversarial quanto um log contaminado.

O teorema central (`noninterference`) diz que os traços de eventos das duas execuções são
iguais, e os resultados `⊒ Endorsed` também, ou então que os traços coincidem até uma
**decisão de endosso** e divergem nela. Não há terceira possibilidade. Antes da primeira
decisão de endosso diferente, as duas execuções fizeram exatamente as mesmas ações, com a
mesma autoridade e os mesmos argumentos.

As consequências (`robust_endorsement`) são estas:

* Se as decisões de endosso coincidem, os traços coincidem por inteiro. Logo a entrada
  `Untrusted` só alcança uma ação passando por um endosso.
* Todo valor aceito por um endosso satisfaz a política auditada (`endorsed_sound`).
* O endosso só acontece com `pc ⊒ Endorsed` (regra de tipagem). É a condição de robustez: a
  própria decisão de endossar não está sob controle de dado não confiável. O lema de
  confinamento (`confine`) fecha o argumento: sob `pc` `Untrusted`, nenhuma ação e nenhum
  endosso acontecem.

Ou seja: entradas `Untrusted` não desviam o fluxo de execução para ações fora de políticas
comprovadas.
-/

namespace Aletheia

open Lab Budget Classical

/-- Equivalência baixa: os ambientes concordam nas posições `⊒ Endorsed` e nas capacidades. -/
inductive LowEq : List Bind → List Val → List Val → Prop where
  | nil : LowEq [] [] []
  | data {Γ ρ₁ ρ₂ v₁ v₂ τ ι} :
      (low ι → v₁ = v₂) → LowEq Γ ρ₁ ρ₂ → LowEq (.data τ ι :: Γ) (v₁ :: ρ₁) (v₂ :: ρ₂)
  | cap {Γ ρ₁ ρ₂ v E} : LowEq Γ ρ₁ ρ₂ → LowEq (.cap E :: Γ) (v :: ρ₁) (v :: ρ₂)

theorem LowEq.data_eq {Γ : List Bind} {ρ₁ ρ₂ : List Val} (h : LowEq Γ ρ₁ ρ₂) :
    ∀ {k : Nat} {τ : Ty} {ι : Lab}, Γ[k]? = some (Bind.data τ ι) → low ι → ρ₁[k]? = ρ₂[k]? := by
  induction h with
  | nil => intro k τ ι hk; simp at hk
  | data hv _ ih =>
    intro k τ ι hk hl
    cases k with
    | zero => simp at hk ⊢; obtain ⟨rfl, rfl⟩ := hk; exact hv hl
    | succ k => simpa using ih (by simpa using hk) hl
  | cap _ ih =>
    intro k τ ι hk hl
    cases k with
    | zero => simp at hk
    | succ k => simpa using ih (by simpa using hk) hl

theorem LowEq.cap_eq {Γ : List Bind} {ρ₁ ρ₂ : List Val} (h : LowEq Γ ρ₁ ρ₂) :
    ∀ {k : Nat} {E : List EffectId}, Γ[k]? = some (Bind.cap E) → ρ₁[k]? = ρ₂[k]? := by
  induction h with
  | nil => intro k E hk; simp at hk
  | data _ _ ih =>
    intro k E hk
    cases k with
    | zero => simp at hk
    | succ k => simpa using ih (by simpa using hk)
  | cap _ ih =>
    intro k E hk
    cases k with
    | zero => rfl
    | succ k => simpa using ih (by simpa using hk)

/-! ## Divergência em endosso -/

/-- Os traços coincidem até uma decisão de endosso e divergem nela. -/
def Diverge (a b : List Ev) : Prop :=
  ∃ (pre : List Ev) (p : Nat) (o₁ o₂ : Option Val) (t₁ t₂ : List Ev),
    o₁ ≠ o₂ ∧ a = pre ++ Ev.endorse p o₁ :: t₁ ∧ b = pre ++ Ev.endorse p o₂ :: t₂

theorem Diverge.append {a b : List Ev} (h : Diverge a b) (x y : List Ev) :
    Diverge (a ++ x) (b ++ y) := by
  obtain ⟨pre, p, o₁, o₂, t₁, t₂, hne, rfl, rfl⟩ := h
  exact ⟨pre, p, o₁, o₂, t₁ ++ x, t₂ ++ y, hne, by simp, by simp⟩

theorem Diverge.prepend {a b : List Ev} (h : Diverge a b) (x : List Ev) :
    Diverge (x ++ a) (x ++ b) := by
  obtain ⟨pre, p, o₁, o₂, t₁, t₂, hne, rfl, rfl⟩ := h
  exact ⟨x ++ pre, p, o₁, o₂, t₁, t₂, hne, by simp, by simp⟩

/-- A conclusão da não-interferência para saídas de integridade `ι`. -/
def NI (ι : Lab) (o₁ o₂ : Out) : Prop :=
  (o₁.tr = o₂.tr ∧ (low ι → o₁.v = o₂.v)) ∨ Diverge o₁.tr o₂.tr

/-- Composição sequencial: se o prefixo concorda ou já divergiu, a concatenação também. -/
theorem seq_agree {a₁ b₁ a₂ b₂ : List Ev} {P : Prop} (h₁ : a₁ = b₁ ∨ Diverge a₁ b₁)
    (h₂ : a₁ = b₁ → (a₂ = b₂ ∧ P) ∨ Diverge a₂ b₂) :
    (a₁ ++ a₂ = b₁ ++ b₂ ∧ P) ∨ Diverge (a₁ ++ a₂) (b₁ ++ b₂) := by
  rcases h₁ with rfl | hd
  · rcases h₂ rfl with ⟨rfl, hp⟩ | hd
    · exact .inl ⟨rfl, hp⟩
    · exact .inr (hd.prepend _)
  · exact .inr (hd.append _ _)

theorem NI.tr {ι : Lab} {o₁ o₂ : Out} (h : NI ι o₁ o₂) : o₁.tr = o₂.tr ∨ Diverge o₁.tr o₂.tr := by
  rcases h with ⟨h, _⟩ | h
  · exact .inl h
  · exact .inr h

/-! ## Laços -/

theorem loop_tr_nil {f : Val → St → Except Err Out}
    (hf : ∀ v (s : St) o, f v s = .ok o → o.tr = []) :
    ∀ (vs : List Val) (s : St) r, loop f vs s = .ok r → r.2 = []
  | [], s, r, h => by simp [loop] at h; subst h; rfl
  | v :: vs, s, r, h => by
    simp only [loop, bnd_eq_ok] at h
    obtain ⟨o, ho, r', hr', h⟩ := h
    cases h
    simp [hf v s o ho, loop_tr_nil hf vs o.st r' hr']

theorem loop_agree {f₁ f₂ : Val → St → Except Err Out}
    (hf : ∀ v (s₁ s₂ : St) o₁ o₂, f₁ v s₁ = .ok o₁ → f₂ v s₂ = .ok o₂ →
      o₁.tr = o₂.tr ∨ Diverge o₁.tr o₂.tr) :
    ∀ (vs : List Val) (s₁ s₂ : St) r₁ r₂, loop f₁ vs s₁ = .ok r₁ → loop f₂ vs s₂ = .ok r₂ →
      r₁.2 = r₂.2 ∨ Diverge r₁.2 r₂.2
  | [], s₁, s₂, r₁, r₂, h₁, h₂ => by
    simp [loop] at h₁ h₂; subst h₁; subst h₂; exact .inl rfl
  | v :: vs, s₁, s₂, r₁, r₂, h₁, h₂ => by
    simp only [loop, bnd_eq_ok] at h₁ h₂
    obtain ⟨o₁, ho₁, r₁', hr₁', h₁⟩ := h₁
    obtain ⟨o₂, ho₂, r₂', hr₂', h₂⟩ := h₂
    cases h₁; cases h₂
    have := seq_agree (P := True) (hf v s₁ s₂ o₁ o₂ ho₁ ho₂) fun _ => by
      rcases loop_agree hf vs o₁.st o₂.st r₁' r₂' hr₁' hr₂' with h | h
      · exact .inl ⟨h, trivial⟩
      · exact .inr h
    rcases this with ⟨h, _⟩ | h
    · exact .inl h
    · exact .inr h

/-! ## Confinamento -/

section
variable {S : Sig} {c : Cfg}

/-- Sob `pc` `Untrusted` não há eventos: nenhuma ação e nenhum endosso. -/
theorem confine {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab} {β : Budget}
    (h : HasType S Γ pc e τ ι β) :
    ¬ low pc → ∀ {ρ : List Val} {s : St} {o : Out}, eval S c e ρ s = .ok o → o.tr = [] := by
  induction h with
  | val _ => intro _ ρ s o he; simp [eval] at he; subst he; rfl
  | var _ =>
    intro _ ρ s o he
    simp only [eval] at he
    split at he
    · cases he; rfl
    · cases he
  | let_ _ _ ih₁ ih₂ =>
    intro hpc ρ s o he
    simp only [eval, bnd_eq_ok] at he
    obtain ⟨o₁, h₁, o₂, h₂, he⟩ := he
    cases he
    simp [ih₁ hpc h₁, ih₂ hpc h₂]
  | @ite Γ pc cnd t f τ ιc ιt ιf βc βt βf _ _ _ ihc iht ihf =>
    intro hpc ρ s o he
    have hpc' : ¬ low (pc ⊓ ιc) := fun h => hpc (low_meet.mp h).1
    simp only [eval, bnd_eq_ok] at he
    obtain ⟨oc, hc, he⟩ := he
    split at he
    · obtain ⟨o', h', he⟩ := bnd_eq_ok.mp he
      cases he
      rcases cond_ok h' with ⟨_, h'⟩ | ⟨_, h'⟩
      · simp [ihc hpc hc, iht hpc' h']
      · simp [ihc hpc hc, ihf hpc' h']
    · cases he
  | prim _ _ iha ihb =>
    intro hpc ρ s o he
    simp only [eval, bnd_eq_ok] at he
    obtain ⟨o₁, h₁, o₂, h₂, he⟩ := he
    split at he
    · cases he; simp [iha hpc h₁, ihb hpc h₂]
    · cases he
  | endorse hl _ _ _ _ => intro hpc; exact absurd hl hpc
  | act hl _ _ _ _ _ => intro hpc; exact absurd hl hpc
  | drop _ =>
    intro _ ρ s o he
    simp only [eval] at he
    obtain ⟨_, _, _, rfl⟩ := doDrop_ok he
    rfl
  | @for_ Γ pc n xs body τ τb ιx ιb βx βb _ _ ihx ihb =>
    intro hpc ρ s o he
    have hpc' : ¬ low (pc ⊓ ιx) := fun h => hpc (low_meet.mp h).1
    simp only [eval, bnd_eq_ok] at he
    obtain ⟨ox, hx, he⟩ := he
    split at he
    · obtain ⟨r, hr, he⟩ := bnd_eq_ok.mp he
      cases he
      simp [ihx hpc hx, loop_tr_nil (fun v s o ho => ihb hpc' ho) _ _ r hr]
    · cases he
  | infer _ iha =>
    intro hpc ρ s o he
    simp only [eval, bnd_eq_ok] at he
    obtain ⟨oa, ha, he⟩ := he
    obtain ⟨_, rfl⟩ := doInfer_ok he
    have := iha hpc ha; exact this
  | await hl => intro hpc; exact absurd hl hpc
  | atomic hl _ _ _ _ _ _ => intro hpc; exact absurd hl hpc

/-! ## O teorema -/

/-- **Teorema T3 (não-interferência com endosso robusto).** Duas execuções bem-sucedidas do
mesmo programa bem tipado e sem sagas, a partir de ambientes baixo-equivalentes e com oráculos e tetos
arbitrários, produzem o mesmo traço e o mesmo resultado `⊒ Endorsed`, ou então divergem
numa decisão de endosso. -/
theorem noninterference {c₁ c₂ : Cfg} {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} (h : HasType S Γ pc e τ ι β) :
    sagaFree e = true → ∀ {ρ₁ ρ₂ : List Val} {s₁ s₂ : St} {o₁ o₂ : Out}, LowEq Γ ρ₁ ρ₂ →
      eval S c₁ e ρ₁ s₁ = .ok o₁ → eval S c₂ e ρ₂ s₂ = .ok o₂ → NI ι o₁ o₂ := by
  induction h with
  | val _ =>
    intro hsf ρ₁ ρ₂ s₁ s₂ o₁ o₂ _ h₁ h₂
    simp [eval] at h₁ h₂; subst h₁; subst h₂
    exact .inl ⟨rfl, fun _ => rfl⟩
  | var hk =>
    intro hsf ρ₁ ρ₂ s₁ s₂ o₁ o₂ hl h₁ h₂
    simp only [eval] at h₁ h₂
    split at h₁
    next v₁ hρ₁ =>
      split at h₂
      next v₂ hρ₂ =>
        cases h₁; cases h₂
        refine .inl ⟨rfl, fun hι => ?_⟩
        have := hl.data_eq hk hι
        rw [hρ₁, hρ₂] at this
        exact Option.some.inj this
      next => cases h₂
    next => cases h₁
  | @let_ Γ pc x y τ₁ τ₂ ι₁ ι₂ β₁ β₂ _ _ ih₁ ih₂ =>
    intro hsf ρ₁ ρ₂ s₁ s₂ o₁ o₂ hl h₁ h₂
    simp only [eval, bnd_eq_ok] at h₁ h₂
    obtain ⟨a₁, ha₁, b₁, hb₁, h₁⟩ := h₁
    obtain ⟨a₂, ha₂, b₂, hb₂, h₂⟩ := h₂
    cases h₁; cases h₂
    simp only [sagaFree, Bool.and_eq_true] at hsf
    have hA := ih₁ hsf.1 hl ha₁ ha₂
    refine seq_agree hA.tr fun htr => ?_
    have hv : low ι₁ → a₁.v = a₂.v := by
      rcases hA with ⟨_, hv⟩ | ⟨pre, p, _, _, t₁, t₂, hne, e₁, e₂⟩
      · exact hv
      · rw [htr] at e₁; rw [e₁] at e₂; simp at e₂; exact absurd e₂.1 hne
    exact ih₂ hsf.2 (.data hv hl) hb₁ hb₂
  | @ite Γ pc cnd t f τ ιc ιt ιf βc βt βf _ ht hf ihc iht ihf =>
    intro hsf ρ₁ ρ₂ s₁ s₂ o₁ o₂ hl h₁ h₂
    simp only [eval, bnd_eq_ok] at h₁ h₂
    obtain ⟨c₁', hc₁, h₁⟩ := h₁
    obtain ⟨c₂', hc₂, h₂⟩ := h₂
    simp only [sagaFree, Bool.and_eq_true] at hsf
    have hC := ihc hsf.1.1 hl hc₁ hc₂
    split at h₁
    next b₁ hb₁ =>
      split at h₂
      next b₂ hb₂ =>
        obtain ⟨x₁, hx₁, h₁⟩ := bnd_eq_ok.mp h₁
        obtain ⟨x₂, hx₂, h₂⟩ := bnd_eq_ok.mp h₂
        cases h₁; cases h₂
        refine seq_agree hC.tr fun htr => ?_
        by_cases hlc : low ιc
        · -- condição confiável: as duas execuções tomam o mesmo ramo
          have hv : c₁'.v = c₂'.v := by
            rcases hC with ⟨_, hv⟩ | ⟨pre, p, _, _, t₁, t₂, hne, e₁, e₂⟩
            · exact hv hlc
            · rw [htr] at e₁; rw [e₁] at e₂; simp at e₂; exact absurd e₂.1 hne
          rw [hb₁, hb₂] at hv
          cases hv
          rcases cond_ok hx₁ with ⟨rfl, hx₁⟩ | ⟨rfl, hx₁⟩ <;> simp only [cond] at hx₂
          · rcases iht hsf.1.2 hl hx₁ hx₂ with ⟨h, hv'⟩ | h
            · exact .inl ⟨h, fun hι => hv' (low_meet.mp (low_meet.mp hι).2).1⟩
            · exact .inr h
          · rcases ihf hsf.2 hl hx₁ hx₂ with ⟨h, hv'⟩ | h
            · exact .inl ⟨h, fun hι => hv' (low_meet.mp (low_meet.mp hι).2).2⟩
            · exact .inr h
        · -- condição não confiável: os ramos rodam sob pc Untrusted e não produzem eventos
          have hpc : ¬ low (pc ⊓ ιc) := fun h => hlc (low_meet.mp h).2
          have e₁ : x₁.tr = [] := by
            rcases cond_ok hx₁ with ⟨_, h⟩ | ⟨_, h⟩
            · exact confine ht hpc h
            · exact confine hf hpc h
          have e₂ : x₂.tr = [] := by
            rcases cond_ok hx₂ with ⟨_, h⟩ | ⟨_, h⟩
            · exact confine ht hpc h
            · exact confine hf hpc h
          exact .inl ⟨by rw [e₁, e₂], fun hι => absurd (low_meet.mp hι).1 hlc⟩
      next => cases h₂
    next => cases h₁
  | prim _ _ iha ihb =>
    intro hsf ρ₁ ρ₂ s₁ s₂ o₁ o₂ hl h₁ h₂
    simp only [eval, bnd_eq_ok] at h₁ h₂
    obtain ⟨a₁, ha₁, b₁, hb₁, h₁⟩ := h₁
    obtain ⟨a₂, ha₂, b₂, hb₂, h₂⟩ := h₂
    simp only [sagaFree, Bool.and_eq_true] at hsf
    have hA := iha hsf.1 hl ha₁ ha₂
    have hB := ihb hsf.2 hl hb₁ hb₂
    split at h₁
    next x₁ y₁ hx₁ hy₁ =>
      split at h₂
      next x₂ y₂ hx₂ hy₂ =>
        cases h₁; cases h₂
        refine seq_agree hA.tr fun htr => ?_
        rcases hB with ⟨hb, hvb⟩ | hd
        · refine .inl ⟨hb, fun hι => ?_⟩
          have hva : a₁.v = a₂.v := by
            rcases hA with ⟨_, hv⟩ | ⟨pre, p, _, _, t₁, t₂, hne, e₁, e₂⟩
            · exact hv (low_meet.mp hι).1
            · rw [htr] at e₁; rw [e₁] at e₂; simp at e₂; exact absurd e₂.1 hne
          have hvb := hvb (low_meet.mp hι).2
          rw [hx₁, hx₂] at hva; rw [hy₁, hy₂] at hvb
          cases hva; cases hvb; rfl
        · exact .inr hd
      next => cases h₂
    next => cases h₁
  | @endorse Γ pc p e d τ ιe ιd βe βd _ _ _ ihe ihd =>
    intro hsf ρ₁ ρ₂ s₁ s₂ o₁ o₂ hl h₁ h₂
    simp only [eval, bnd_eq_ok] at h₁ h₂
    obtain ⟨e₁, he₁, h₁⟩ := h₁
    obtain ⟨e₂, he₂, h₂⟩ := h₂
    simp only [sagaFree, Bool.and_eq_true] at hsf
    have hE := (ihe hsf.1 hl he₁ he₂).tr
    by_cases hp₁ : S.pol p e₁.v = true <;> by_cases hp₂ : S.pol p e₂.v = true <;>
      simp only [hp₁, hp₂, if_true, if_false, Bool.false_eq_true] at h₁ h₂
    · -- os dois aceitam
      cases h₁; cases h₂
      rcases hE with htr | hd
      · by_cases hv : e₁.v = e₂.v
        · exact .inl ⟨by simp [htr, hv], fun _ => hv⟩
        · exact .inr ⟨e₁.tr, p, some e₁.v, some e₂.v, [], [], by simpa using hv, rfl, by simp [htr]⟩
      · exact .inr (hd.append _ _)
    · -- só o primeiro aceita
      obtain ⟨x₂, _, h₂⟩ := bnd_eq_ok.mp (pre_eq_ok.mp h₂)
      cases h₁; cases h₂
      rcases hE with htr | hd
      · exact .inr ⟨e₁.tr, p, some e₁.v, none, [], x₂.tr, by simp, rfl, by simp [htr]⟩
      · exact .inr (hd.append _ _)
    · -- só o segundo aceita
      obtain ⟨x₁, _, h₁⟩ := bnd_eq_ok.mp (pre_eq_ok.mp h₁)
      cases h₁; cases h₂
      rcases hE with htr | hd
      · exact .inr ⟨e₁.tr, p, none, some e₂.v, x₁.tr, [], by simp, rfl, by simp [htr]⟩
      · exact .inr (hd.append _ _)
    · -- os dois recusam: as duas execuções seguem pelo ramo `else`
      obtain ⟨x₁, hx₁, h₁⟩ := bnd_eq_ok.mp (pre_eq_ok.mp h₁)
      obtain ⟨x₂, hx₂, h₂⟩ := bnd_eq_ok.mp (pre_eq_ok.mp h₂)
      cases h₁; cases h₂
      have := seq_agree (a₁ := e₁.tr ++ [Ev.endorse p none]) (b₁ := e₂.tr ++ [Ev.endorse p none])
        (P := low (Lab.E ⊓ ιd) → x₁.v = x₂.v)
        (by rcases hE with htr | hd
            · exact .inl (by rw [htr])
            · exact .inr (hd.append _ _)) fun _ => by
          rcases ihd hsf.2 hl hx₁ hx₂ with ⟨h, hv⟩ | h
          · exact .inl ⟨h, fun hι => hv (low_meet.mp hι).2⟩
          · exact .inr h
      simpa using this
  | @act Γ pc t k a ιa βa E _ hk _ _ hla iha =>
    intro hsf ρ₁ ρ₂ s₁ s₂ o₁ o₂ hl h₁ h₂
    simp only [eval, bnd_eq_ok] at h₁ h₂
    obtain ⟨a₁, ha₁, h₁⟩ := h₁
    obtain ⟨a₂, ha₂, h₂⟩ := h₂
    obtain ⟨id₁, hid₁, _, _, _, rfl⟩ := doAct_ok h₁
    obtain ⟨id₂, hid₂, _, _, _, rfl⟩ := doAct_ok h₂
    have hcap := hl.cap_eq hk
    rw [hid₁, hid₂] at hcap
    cases hcap
    rcases iha hsf hl ha₁ ha₂ with ⟨htr, hv⟩ | hd
    · exact .inl ⟨by simp [htr, hv hla], fun _ => rfl⟩
    · exact .inr (hd.append _ _)
  | @for_ Γ pc n xs body τ τb ιx ιb βx βb _ hb ihx ihb =>
    intro hsf ρ₁ ρ₂ s₁ s₂ o₁ o₂ hl h₁ h₂
    simp only [eval, bnd_eq_ok] at h₁ h₂
    obtain ⟨x₁, hx₁, h₁⟩ := h₁
    obtain ⟨x₂, hx₂, h₂⟩ := h₂
    simp only [sagaFree, Bool.and_eq_true] at hsf
    have hX := ihx hsf.1 hl hx₁ hx₂
    split at h₁
    next vs₁ hvs₁ =>
      split at h₂
      next vs₂ hvs₂ =>
        obtain ⟨r₁, hr₁, h₁⟩ := bnd_eq_ok.mp h₁
        obtain ⟨r₂, hr₂, h₂⟩ := bnd_eq_ok.mp h₂
        cases h₁; cases h₂
        refine seq_agree hX.tr fun htr => ?_
        by_cases hlx : low ιx
        · -- lista confiável: as duas execuções iteram a mesma lista
          have hv : x₁.v = x₂.v := by
            rcases hX with ⟨_, hv⟩ | ⟨pre, p, _, _, t₁, t₂, hne, e₁, e₂⟩
            · exact hv hlx
            · rw [htr] at e₁; rw [e₁] at e₂; simp at e₂; exact absurd e₂.1 hne
          rw [hvs₁, hvs₂] at hv
          cases hv
          rcases loop_agree (f₁ := fun v s' => eval S c₁ body (v :: ρ₁) s')
              (f₂ := fun v s' => eval S c₂ body (v :: ρ₂) s') (fun v s₁ s₂ o₁ o₂ h₁ h₂ =>
              (ihb hsf.2 (.data (fun _ => rfl) hl) h₁ h₂).tr) vs₁ _ _ r₁ r₂ hr₁ hr₂ with h | h
          · exact .inl ⟨h, fun _ => rfl⟩
          · exact .inr h
        · -- lista não confiável: o corpo roda sob pc Untrusted e não produz eventos
          have hpc : ¬ low (pc ⊓ ιx) := fun h => hlx (low_meet.mp h).2
          have e₁ := loop_tr_nil (fun v s o ho => confine hb hpc ho) vs₁ _ r₁ hr₁
          have e₂ := loop_tr_nil (fun v s o ho => confine hb hpc ho) vs₂ _ r₂ hr₂
          exact .inl ⟨by rw [e₁, e₂], fun _ => rfl⟩
      next => cases h₂
    next => cases h₁
  | drop _ =>
    intro hsf ρ₁ ρ₂ s₁ s₂ o₁ o₂ _ h₁ h₂
    simp only [eval] at h₁ h₂
    obtain ⟨_, _, _, rfl⟩ := doDrop_ok h₁
    obtain ⟨_, _, _, rfl⟩ := doDrop_ok h₂
    exact .inl ⟨rfl, fun _ => rfl⟩
  | infer _ iha =>
    intro hsf ρ₁ ρ₂ s₁ s₂ o₁ o₂ hl h₁ h₂
    simp only [eval, bnd_eq_ok] at h₁ h₂
    obtain ⟨a₁, ha₁, h₁⟩ := h₁
    obtain ⟨a₂, ha₂, h₂⟩ := h₂
    obtain ⟨_, rfl⟩ := doInfer_ok h₁
    obtain ⟨_, rfl⟩ := doInfer_ok h₂
    rcases (iha hsf hl ha₁ ha₂).tr with h | h
    · exact .inl ⟨h, fun hι => absurd hι not_low_U⟩
    · exact .inr h
  | await _ =>
    intro _ ρ₁ ρ₂ s₁ s₂ o₁ o₂ _ h₁ h₂
    simp only [eval] at h₁ h₂
    split at h₁ <;> split at h₂ <;> cases h₁ <;> cases h₂
    exact .inl ⟨rfl, fun _ => rfl⟩
  | atomic _ _ _ _ _ _ _ => intro hsf; simp [sagaFree] at hsf

/-! ## Consequências -/

/-- As decisões de endosso de um traço, em ordem. -/
def endorsements (tr : List Ev) : List (Nat × Option Val) :=
  tr.filterMap fun
    | .endorse p o => some (p, o)
    | _ => none

/-- As ações de um traço: ferramenta, capacidade e argumento, em ordem. -/
def actions (tr : List Ev) : List (Nat × Nat × Val) :=
  tr.filterMap fun
    | .act t id v => some (t, id, v)
    | _ => none

theorem Diverge.endorsements_ne {a b : List Ev} (h : Diverge a b) :
    endorsements a ≠ endorsements b := by
  obtain ⟨pre, p, o₁, o₂, t₁, t₂, hne, rfl, rfl⟩ := h
  intro heq
  simp only [endorsements, List.filterMap_append, List.filterMap_cons] at heq
  have := List.append_cancel_left heq
  simp at this
  exact hne this.1

/-- **T3, endosso robusto.** Mesmo programa bem tipado e entradas confiáveis iguais. Se as
decisões de endosso coincidem, as duas execuções fazem exatamente as mesmas ações, com as
mesmas capacidades e os mesmos argumentos, e devolvem o mesmo resultado `⊒ Endorsed`. Tudo
isso vale por mais que as entradas `Untrusted` e as respostas dos modelos difiram. -/
theorem robust_endorsement {c₁ c₂ : Cfg} {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty}
    {ι : Lab} {β : Budget} {ρ₁ ρ₂ : List Val} {s₁ s₂ : St} {o₁ o₂ : Out}
    (h : HasType S Γ pc e τ ι β) (hsf : sagaFree e = true) (hl : LowEq Γ ρ₁ ρ₂)
    (h₁ : eval S c₁ e ρ₁ s₁ = .ok o₁) (h₂ : eval S c₂ e ρ₂ s₂ = .ok o₂)
    (hend : endorsements o₁.tr = endorsements o₂.tr) :
    o₁.tr = o₂.tr ∧ actions o₁.tr = actions o₂.tr ∧ (low ι → o₁.v = o₂.v) := by
  rcases noninterference h hsf hl h₁ h₂ with ⟨htr, hv⟩ | hd
  · exact ⟨htr, by rw [htr], hv⟩
  · exact absurd hend hd.endorsements_ne

/-- Antes da primeira decisão de endosso divergente, as ações são as mesmas: a parte comum
do traço é idêntica nas duas execuções. -/
theorem actions_agree_until_endorsement {c₁ c₂ : Cfg} {Γ : List Bind} {pc : Lab} {e : Exp}
    {τ : Ty} {ι : Lab} {β : Budget} {ρ₁ ρ₂ : List Val} {s₁ s₂ : St} {o₁ o₂ : Out}
    (h : HasType S Γ pc e τ ι β) (hsf : sagaFree e = true) (hl : LowEq Γ ρ₁ ρ₂)
    (h₁ : eval S c₁ e ρ₁ s₁ = .ok o₁) (h₂ : eval S c₂ e ρ₂ s₂ = .ok o₂) :
    o₁.tr = o₂.tr ∨ ∃ pre p r₁ r₂ t₁ t₂, r₁ ≠ r₂ ∧
      o₁.tr = pre ++ Ev.endorse p r₁ :: t₁ ∧ o₂.tr = pre ++ Ev.endorse p r₂ :: t₂ := by
  rcases noninterference h hsf hl h₁ h₂ with ⟨htr, _⟩ | hd
  · exact .inl htr
  · exact .inr hd

/-- **T1 + T3.** A não-interferência não depende de as duas execuções terminarem bem: para um
programa bem tipado e linear, com o orçamento estático dentro dos dois tetos, o T1 garante as
duas execuções. A conclusão do T3 vale, portanto, sem hipótese sobre o resultado. -/
theorem noninterference_total {c₁ c₂ : Cfg} (hO₁ : c₁.oracle.Ok) (hO₂ : c₂.oracle.Ok)
    {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab} {β : Budget}
    {ρ₁ ρ₂ : List Val} {s₁ s₂ : St} (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (hsf : sagaFree e = true) (hg : Guarded e) (h0₁ : Start c₁.grants Γ ρ₁ s₁) (h0₂ : Start c₂.grants Γ ρ₂ s₂) (hB₁ : s₁.spent + β ≤ c₁.limit)
    (hB₂ : s₂.spent + β ≤ c₂.limit) (hl : LowEq Γ ρ₁ ρ₂) :
    ∃ o₁ o₂, eval S c₁ e ρ₁ s₁ = .ok o₁ ∧ eval S c₂ e ρ₂ s₂ = .ok o₂ ∧ NI ι o₁ o₂ := by
  obtain ⟨o₁, h₁, _⟩ := soundness hO₁ h hlin hg h0₁ hB₁
  obtain ⟨o₂, h₂, _⟩ := soundness hO₂ h hlin hg h0₂ hB₂
  exact ⟨o₁, o₂, h₁, h₂, noninterference h hsf hl h₁ h₂⟩

/-- Todo endosso aceito num traço satisfaz a política auditada que o aceitou. -/
def PolicyOk (S : Sig) (tr : List Ev) : Prop := ∀ p v, Ev.endorse p (some v) ∈ tr → S.pol p v = true

theorem PolicyOk.nil : PolicyOk S [] := by intro p v h; simp at h

theorem PolicyOk.append {a b : List Ev} (ha : PolicyOk S a) (hb : PolicyOk S b) :
    PolicyOk S (a ++ b) := by
  intro p v h
  rcases List.mem_append.mp h with h | h
  · exact ha p v h
  · exact hb p v h

theorem loop_policy {f : Val → St → Except Err Out}
    (hf : ∀ v (s : St) o, f v s = .ok o → PolicyOk S o.tr) :
    ∀ (vs : List Val) (s : St) r, loop f vs s = .ok r → PolicyOk S r.2
  | [], s, r, h => by simp [loop] at h; subst h; exact PolicyOk.nil
  | v :: vs, s, r, h => by
    simp only [loop, bnd_eq_ok] at h
    obtain ⟨o, ho, r', hr', h⟩ := h
    cases h
    exact (hf v s o ho).append (loop_policy hf vs o.st r' hr')

/-- Endossos pela política valem nos dois desfechos, inclusive no traço de uma saga que
falha (as compensações e as sondas não são endossos). -/
theorem policy_preserves : Preserves S c True (fun _ _ tr => PolicyOk S tr) where
  refl _ := PolicyOk.nil
  trans h₁ h₂ := h₁.append h₂
  endorse hp := by intro p' v hv; simp at hv; obtain ⟨rfl, rfl⟩ := hv; exact hp
  reject := fun p' v hv => by rcases List.mem_singleton.mp hv with ⟨⟩
  act _ _ := by intro p v hv; exact absurd hv endorse_not_mem_actEvs
  drop _ := PolicyOk.nil
  infer := PolicyOk.nil
  commit _ := fun h => h
  revoke _ := fun _ _ _ h => h
  abort _ := fun q r p h => by
    rw [List.append_assoc]
    refine h.append ?_
    intro p' v hv
    simp only [List.cons_append, List.mem_cons, List.mem_append, List.mem_singleton, reduceCtorEq,
      false_or, or_false] at hv
    rcases hv with hv | hv
    · obtain ⟨_, _, _, hu⟩ := compensate_undo S _ _ _ hv
      cases hu
    · simp at hv

/-- **T3, solidez do endosso.** Em qualquer execução, de qualquer programa, todo valor aceito
por `refine … by p` satisfaz `p`. -/
theorem endorsed_sound (e : Exp) {ρ : List Val} {s : St} {o : Out}
    (h : eval S c e ρ s = .ok o) : PolicyOk S o.tr :=
  (eval_preserves policy_preserves e (.inl trivial)).1 h

end

end Aletheia
