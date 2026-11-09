import Aletheia.Eval

/-!
# Teorema T1: preservação e progresso

Para um avaliador total de passo grande, a solidez de tipos tem duas metades.

* **Progresso**: um programa bem tipado não chega a nenhum erro de execução. Ele não fica
  preso, não executa efeito sem capacidade, não reusa capacidade linear e não ultrapassa o
  orçamento.
* **Preservação**: o valor final tem o tipo estático, e o gasto real fica dentro do
  orçamento estático β em cada uma das seis dimensões. O laço, em particular, gasta no
  máximo `n • β(corpo)`.

As hipóteses sobre o ponto de partida são estas: o ambiente realiza Γ; as capacidades do
agente têm identidades distintas e ainda não foram consumidas; o programa passa no E1
(`Linear`); o gasto inicial mais β cabe no teto; e o oráculo devolve valores bem tipados,
que é a validação de esquema do ARX.
-/

namespace Aletheia

open Lab Budget

/-! ## Invariantes de capacidade -/

/-- As capacidades do ambiente têm identidades distintas. -/
def CapsDistinct (ρ : List Val) : Prop :=
  ∀ i j id : Nat, ρ[i]? = some (Val.cap id) → ρ[j]? = some (Val.cap id) → i = j

/-- Linearidade relativa ao ambiente: cada capacidade presente é usada no máximo uma vez. -/
def LinRho (ρ : List Val) (f : Nat → Nat) : Prop :=
  ∀ k id : Nat, ρ[k]? = some (Val.cap id) → f k ≤ 1

/-- Toda capacidade que a expressão ainda pode usar está livre (fora de `u`). -/
def Avail (ρ : List Val) (u : List Nat) (f : Nat → Nat) : Prop :=
  ∀ k id : Nat, 1 ≤ f k → ρ[k]? = some (Val.cap id) → id ∉ u

/-- Pegada: a avaliação só acrescenta a `u` identidades de capacidades que ela pode usar. -/
def Footprint (ρ : List Val) (f : Nat → Nat) (u u' : List Nat) : Prop :=
  ∃ new : List Nat, u' = new ++ u ∧ ∀ id ∈ new, ∃ k, 1 ≤ f k ∧ ρ[k]? = some (Val.cap id)

theorem CapsDistinct.cons {ρ : List Val} {v : Val} (hv : ∀ id, v ≠ .cap id)
    (h : CapsDistinct ρ) : CapsDistinct (v :: ρ) := by
  intro i j id hi hj
  cases i with
  | zero => simp at hi; exact absurd hi (hv id)
  | succ i =>
    cases j with
    | zero => simp at hj; exact absurd hj (hv id)
    | succ j => simp at hi hj; exact congrArg (· + 1) (h i j id hi hj)

theorem LinRho.mono {ρ : List Val} {f g : Nat → Nat} (h : LinRho ρ g) (hfg : ∀ k, f k ≤ g k) :
    LinRho ρ f := fun k id hk => Nat.le_trans (hfg k) (h k id hk)

theorem LinRho.cons {ρ : List Val} {v : Val} {f : Nat → Nat} (hv : ∀ id, v ≠ .cap id)
    (h : LinRho ρ fun k => f (k + 1)) : LinRho (v :: ρ) f := by
  intro k id hk
  cases k with
  | zero => simp at hk; exact absurd hk (hv id)
  | succ k => exact h k id (by simpa using hk)

theorem Avail.mono {ρ : List Val} {u : List Nat} {f g : Nat → Nat} (h : Avail ρ u g)
    (hfg : ∀ k, f k ≤ g k) : Avail ρ u f := fun k id hk hid => h k id (Nat.le_trans hk (hfg k)) hid

theorem Avail.cons {ρ : List Val} {u : List Nat} {v : Val} {f : Nat → Nat}
    (hv : ∀ id, v ≠ .cap id) (h : Avail ρ u fun k => f (k + 1)) : Avail (v :: ρ) u f := by
  intro k id hk hid
  cases k with
  | zero => simp at hid; exact absurd hid (hv id)
  | succ k => exact h k id hk (by simpa using hid)

theorem Footprint.refl {ρ : List Val} {f : Nat → Nat} {u : List Nat} : Footprint ρ f u u :=
  ⟨[], rfl, by simp⟩

theorem Footprint.mono {ρ : List Val} {f g : Nat → Nat} {u u' : List Nat} (h : Footprint ρ f u u')
    (hfg : ∀ k, f k ≤ g k) : Footprint ρ g u u' := by
  obtain ⟨new, rfl, hn⟩ := h
  refine ⟨new, rfl, fun id hid => ?_⟩
  obtain ⟨k, hk, hk'⟩ := hn id hid
  exact ⟨k, Nat.le_trans hk (hfg k), hk'⟩

theorem Footprint.trans {ρ : List Val} {f : Nat → Nat} {u₁ u₂ u₃ : List Nat}
    (h₁ : Footprint ρ f u₁ u₂) (h₂ : Footprint ρ f u₂ u₃) : Footprint ρ f u₁ u₃ := by
  obtain ⟨n₁, rfl, hn₁⟩ := h₁
  obtain ⟨n₂, rfl, hn₂⟩ := h₂
  refine ⟨n₂ ++ n₁, by simp, fun id hid => ?_⟩
  rcases List.mem_append.mp hid with h | h
  · exact hn₂ id h
  · exact hn₁ id h

theorem Footprint.uncons {ρ : List Val} {v : Val} {f : Nat → Nat} {u u' : List Nat}
    (hv : ∀ id, v ≠ .cap id) (h : Footprint (v :: ρ) f u u') :
    Footprint ρ (fun k => f (k + 1)) u u' := by
  obtain ⟨new, rfl, hn⟩ := h
  refine ⟨new, rfl, fun id hid => ?_⟩
  obtain ⟨k, hk, hk'⟩ := hn id hid
  cases k with
  | zero => simp at hk'; exact absurd hk' (hv id)
  | succ k => exact ⟨k, hk, by simpa using hk'⟩

/-- Composição sequencial: o que a segunda parte usa continua livre depois da primeira,
porque nenhuma capacidade é usada pelas duas (E1) e capacidades distintas têm identidades
distintas. -/
theorem avail_seq {ρ : List Val} {u u₁ : List Nat} {f₁ f₂ g : Nat → Nat}
    (hg : ∀ k, f₁ k + f₂ k ≤ g k) (hL : LinRho ρ g) (hD : CapsDistinct ρ) (hA : Avail ρ u g)
    (hF : Footprint ρ f₁ u u₁) : Avail ρ u₁ f₂ := by
  intro k id hk hid hmem
  obtain ⟨new, rfl, hn⟩ := hF
  rcases List.mem_append.mp hmem with h | h
  · obtain ⟨k', hk', hid'⟩ := hn id h
    have := hD k k' id hid hid'
    subst this
    have := hL k id hid
    have := hg k
    omega
  · exact hA k id (by have := hg k; omega) hid h

/-! ## Inversão da tipagem de valores -/

theorem HasTy.bool_inv {v : Val} (h : HasTy v .bool) : ∃ b, v = .bool b := by
  cases h; exact ⟨_, rfl⟩

theorem HasTy.int_inv {v : Val} (h : HasTy v .int) : ∃ n, v = .int n := by
  cases h; exact ⟨_, rfl⟩

theorem HasTy.list_inv {v : Val} {t : Ty} {n : Nat} (h : HasTy v (.list t n)) :
    ∃ vs, v = .list vs ∧ (∀ x ∈ vs, HasTy x t) ∧ vs.length ≤ n := by
  cases h with
  | list hall hlen => exact ⟨_, rfl, hall, hlen⟩

theorem Within.step (s a : Budget) : Within s (s + a) a := fun d => by simp

/-! ## O laço -/

theorem loop_sound {f : Val → St → Except Err Out} {ρ : List Val} {g : Nat → Nat} {τ : Ty}
    {βb L : Budget}
    (hf : ∀ v (s : St), HasTy v τ → Avail ρ s.used g → s.spent + βb ≤ L →
      ∃ o, f v s = .ok o ∧ Within s.spent o.st.spent βb ∧ Footprint ρ g s.used o.st.used) :
    ∀ (n : Nat) (vs : List Val) (s : St), vs.length ≤ n → (∀ v ∈ vs, HasTy v τ) →
      s.spent + n • βb ≤ L →
      (∀ k id, 1 ≤ g k → ρ[k]? = some (.cap id) → n ≤ 1 ∧ (vs ≠ [] → id ∉ s.used)) →
      ∃ s' tr, loop f vs s = .ok (s', tr) ∧ Within s.spent s'.spent (n • βb) ∧
        Footprint ρ (fun k => n * g k) s.used s'.used
  | n, [], s, _, _, _, _ => ⟨s, [], rfl, Within.refl _ _, Footprint.refl⟩
  | n, v :: vs, s, hlen, hall, hB, hinv => by
    obtain ⟨n', rfl⟩ : ∃ n', n = n' + 1 := ⟨n - 1, by simp at hlen; omega⟩
    have hav : Avail ρ s.used g := fun k id hk hid => (hinv k id hk hid).2 (List.cons_ne_nil _ _)
    have hB₁ : s.spent + βb ≤ L := fun d => by
      have := hB d; simp only [add_apply, scale_apply, Nat.add_mul, Nat.one_mul] at this ⊢; omega
    obtain ⟨o, he, hw, hfp⟩ := hf v s (hall v (List.mem_cons_self ..)) hav hB₁
    have hB₂ : o.st.spent + n' • βb ≤ L := fun d => by
      have := hB d; have := (hw d).2
      simp only [add_apply, scale_apply, Nat.add_mul, Nat.one_mul] at *; omega
    have hinv' : ∀ k id, 1 ≤ g k → ρ[k]? = some (.cap id) → n' ≤ 1 ∧ (vs ≠ [] → id ∉ o.st.used) := by
      intro k id hk hid
      have := (hinv k id hk hid).1
      refine ⟨by omega, fun hne => ?_⟩
      have : vs.length = 0 := by simp at hlen; omega
      exact absurd (List.eq_nil_of_length_eq_zero this) hne
    obtain ⟨s', tr, hl, hw', hfp'⟩ :=
      loop_sound hf n' vs o.st (by simp at hlen; omega) (fun x hx => hall x (List.mem_cons_of_mem _ hx))
        hB₂ hinv'
    refine ⟨s', o.tr ++ tr, by simp [loop, he, hl], ?_, ?_⟩
    · refine (hw.trans hw').mono fun d => ?_
      simp only [add_apply, scale_apply, Nat.add_mul, Nat.one_mul]; omega
    · refine (hfp.mono fun k => ?_).trans (hfp'.mono fun k => ?_)
      · simp only [Nat.add_mul, Nat.one_mul]; omega
      · simp only [Nat.add_mul, Nat.one_mul]; omega

/-! ## O teorema -/

section
variable {S : Sig} {c : Cfg}

theorem sound (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} (h : HasType S Γ pc e τ ι β) :
    ∀ {ρ : List Val} {s : St}, EnvOk Γ ρ → CapsDistinct ρ → LinRho ρ (uses e) →
      Avail ρ s.used (uses e) → s.spent + β ≤ c.limit →
      ∃ o, eval S c e ρ s = .ok o ∧ HasTy o.v τ ∧ Within s.spent o.st.spent β ∧
        Footprint ρ (uses e) s.used o.st.used := by
  induction h with
  | val hv =>
    intro ρ s _ _ _ _ _
    exact ⟨⟨_, s, []⟩, rfl, hv, Within.refl _ _, Footprint.refl⟩
  | var hk =>
    intro ρ s hE _ _ _ _
    obtain ⟨v, hρ, hv⟩ := hE.data_lookup hk
    exact ⟨⟨v, s, []⟩, by simp [eval, hρ], hv, Within.refl _ _, Footprint.refl⟩
  | @let_ Γ pc e₁ e₂ τ₁ τ₂ ι₁ ι₂ β₁ β₂ _ _ ih₁ ih₂ =>
    intro ρ s hE hD hL hA hB
    have hg : ∀ k, uses e₁ k + uses e₂ (k + 1) ≤ uses (.let_ e₁ e₂) k := fun _ => Nat.le_refl _
    obtain ⟨o₁, he₁, hv₁, hw₁, hf₁⟩ :=
      ih₁ hE hD (hL.mono fun k => by have := hg k; omega) (hA.mono fun k => by have := hg k; omega)
        (fun d => by have := hB d; simp at this ⊢; omega)
    have hvc := hv₁.not_cap
    obtain ⟨o₂, he₂, hv₂, hw₂, hf₂⟩ :=
      ih₂ (.data hv₁ hE) (hD.cons hvc) (LinRho.cons hvc (hL.mono fun k => by have := hg k; omega))
        (Avail.cons hvc (avail_seq hg hL hD hA hf₁))
        (fun d => by have := hB d; have := (hw₁ d).2; simp at *; omega)
    refine ⟨⟨o₂.v, o₂.st, o₁.tr ++ o₂.tr⟩, by simp [eval, he₁, he₂], hv₂, hw₁.trans hw₂, ?_⟩
    exact (hf₁.mono fun k => by have := hg k; omega).trans
      ((hf₂.uncons hvc).mono fun k => by have := hg k; omega)
  | @ite Γ pc cnd t f τ ιc ιt ιf βc βt βf _ _ _ ihc iht ihf =>
    intro ρ s hE hD hL hA hB
    have hgt : ∀ k, uses cnd k + uses t k ≤ uses (.ite cnd t f) k := fun k => by
      simp only [uses]; omega
    have hgf : ∀ k, uses cnd k + uses f k ≤ uses (.ite cnd t f) k := fun k => by
      simp only [uses]; omega
    obtain ⟨oc, hec, hvc, hwc, hfc⟩ :=
      ihc hE hD (hL.mono fun k => by have := hgt k; omega) (hA.mono fun k => by have := hgt k; omega)
        (fun d => by have := hB d; simp at this ⊢; omega)
    obtain ⟨b, hb⟩ := hvc.bool_inv
    cases b with
    | true =>
      obtain ⟨o', he', hv', hw', hf'⟩ :=
        iht hE hD (hL.mono fun k => by have := hgt k; omega) (avail_seq hgt hL hD hA hfc)
          (fun d => by have := hB d; have := (hwc d).2; simp at *; omega)
      refine ⟨⟨o'.v, o'.st, oc.tr ++ o'.tr⟩, by simp [eval, hec, hb, he'], hv', ?_, ?_⟩
      · exact (hwc.trans hw').mono fun d => by simp; omega
      · exact (hfc.mono fun k => by have := hgt k; omega).trans
          (hf'.mono fun k => by have := hgt k; omega)
    | false =>
      obtain ⟨o', he', hv', hw', hf'⟩ :=
        ihf hE hD (hL.mono fun k => by have := hgf k; omega) (avail_seq hgf hL hD hA hfc)
          (fun d => by have := hB d; have := (hwc d).2; simp at *; omega)
      refine ⟨⟨o'.v, o'.st, oc.tr ++ o'.tr⟩, by simp [eval, hec, hb, he'], hv', ?_, ?_⟩
      · exact (hwc.trans hw').mono fun d => by simp; omega
      · exact (hfc.mono fun k => by have := hgf k; omega).trans
          (hf'.mono fun k => by have := hgf k; omega)
  | @prim Γ pc op a b ιa ιb βa βb _ _ iha ihb =>
    intro ρ s hE hD hL hA hB
    have hg : ∀ k, uses a k + uses b k ≤ uses (.prim op a b) k := fun _ => Nat.le_refl _
    obtain ⟨oa, hea, hva, hwa, hfa⟩ :=
      iha hE hD (hL.mono fun k => by have := hg k; omega) (hA.mono fun k => by have := hg k; omega)
        (fun d => by have := hB d; simp at this ⊢; omega)
    obtain ⟨ob, heb, hvb, hwb, hfb⟩ :=
      ihb hE hD (hL.mono fun k => by have := hg k; omega) (avail_seq hg hL hD hA hfa)
        (fun d => by have := hB d; have := (hwa d).2; simp at *; omega)
    obtain ⟨x, hx⟩ := hva.int_inv
    obtain ⟨y, hy⟩ := hvb.int_inv
    refine ⟨⟨op.apply x y, ob.st, oa.tr ++ ob.tr⟩, by simp [eval, hea, heb, hx, hy],
      op.apply_ty x y, hwa.trans hwb, ?_⟩
    exact (hfa.mono fun k => by have := hg k; omega).trans (hfb.mono fun k => by have := hg k; omega)
  | @endorse Γ pc p e d τ ιe ιd βe βd _ _ _ ihe ihd =>
    intro ρ s hE hD hL hA hB
    have hg : ∀ k, uses e k + uses d k ≤ uses (.endorse p e d) k := fun _ => Nat.le_refl _
    obtain ⟨oe, hee, hve, hwe, hfe⟩ :=
      ihe hE hD (hL.mono fun k => by have := hg k; omega) (hA.mono fun k => by have := hg k; omega)
        (fun d => by have := hB d; simp at this ⊢; omega)
    by_cases hp : S.pol p oe.v = true
    · refine ⟨⟨oe.v, oe.st, oe.tr ++ [.endorse p (some oe.v)]⟩, by simp [eval, hee, hp], hve,
        hwe.mono fun d => by simp, hfe.mono fun k => by have := hg k; omega⟩
    · obtain ⟨od, hed, hvd, hwd, hfd⟩ :=
        ihd hE hD (hL.mono fun k => by have := hg k; omega) (avail_seq hg hL hD hA hfe)
          (fun d => by have := hB d; have := (hwe d).2; simp at *; omega)
      refine ⟨⟨od.v, od.st, oe.tr ++ .endorse p none :: od.tr⟩, by simp [eval, hee, hp, hed], hvd,
        hwe.trans hwd, ?_⟩
      exact (hfe.mono fun k => by have := hg k; omega).trans
        (hfd.mono fun k => by have := hg k; omega)
  | @act Γ pc t j a ιa βa _ hk _ _ iha =>
    intro ρ s hE hD hL hA hB
    have hg : ∀ k, uses a k + (if j = k then 1 else 0) ≤ uses (.act t j a) k :=
      fun _ => Nat.le_refl _
    obtain ⟨oa, hea, _, hwa, hfa⟩ :=
      iha hE hD (hL.mono fun k => by have := hg k; omega) (hA.mono fun k => by have := hg k; omega)
        (fun d => by have := hB d; simp at this ⊢; omega)
    obtain ⟨id, hid⟩ := hE.cap_lookup hk
    have hfree : id ∉ oa.st.used := avail_seq hg hL hD hA hfa j id (by simp) hid
    have hble : ble (oa.st.spent + S.cost t) c.limit = true :=
      ble_iff.mpr fun d => by have := hB d; have := (hwa d).2; simp at *; omega
    refine ⟨⟨.unit, ⟨id :: oa.st.used, oa.st.spent + S.cost t, oa.st.calls⟩, oa.tr ++ [.act t id oa.v]⟩,
      by simp [eval, hea, doAct, hid, hfree, hble], .unit, hwa.trans (Within.step _ _), ?_⟩
    obtain ⟨new, hnew, hn⟩ := hfa
    refine ⟨id :: new, by simp [hnew], fun x hx => ?_⟩
    rcases List.mem_cons.mp hx with rfl | hx
    · exact ⟨j, by have := hg j; simp at this; omega, hid⟩
    · obtain ⟨k, hk', hk''⟩ := hn x hx
      exact ⟨k, by have := hg k; omega, hk''⟩
  | @for_ Γ pc n xs body τ τb ιx ιb βx βb _ _ ihx ihb =>
    intro ρ s hE hD hL hA hB
    have hg : ∀ k, uses xs k + n * uses body (k + 1) ≤ uses (.for_ n xs body) k :=
      fun _ => Nat.le_refl _
    obtain ⟨ox, hex, hvx, hwx, hfx⟩ :=
      ihx hE hD (hL.mono fun k => by have := hg k; omega) (hA.mono fun k => by have := hg k; omega)
        (fun d => by have := hB d; simp only [add_apply] at this ⊢; omega)
    obtain ⟨vs, hvs, hall, hlen⟩ := hvx.list_inv
    rcases Nat.eq_zero_or_pos n with hn | hn
    · subst hn
      have : vs = [] := List.eq_nil_of_length_eq_zero (by omega)
      subst this
      refine ⟨⟨.unit, ox.st, ox.tr ++ []⟩, by simp [eval, hex, hvs, loop], .unit,
        hwx.mono fun d => by simp, hfx.mono fun k => by have := hg k; omega⟩
    · have hf : ∀ v (s' : St), HasTy v τ → Avail ρ s'.used (fun k => uses body (k + 1)) →
          s'.spent + βb ≤ c.limit →
          ∃ o, eval S c body (v :: ρ) s' = .ok o ∧ Within s'.spent o.st.spent βb ∧
            Footprint ρ (fun k => uses body (k + 1)) s'.used o.st.used := by
        intro v s' hv hav hb
        have hvc := hv.not_cap
        obtain ⟨o, he, _, hw, hfp⟩ :=
          ihb (.data hv hE) (hD.cons hvc)
            (LinRho.cons hvc (hL.mono fun k => by
              have := hg k; have := Nat.mul_le_mul hn (Nat.le_refl (uses body (k + 1))); omega))
            (Avail.cons hvc hav) hb
        exact ⟨o, he, hw, hfp.uncons hvc⟩
      have hinv : ∀ k id, 1 ≤ uses body (k + 1) → ρ[k]? = some (.cap id) →
          n ≤ 1 ∧ (vs ≠ [] → id ∉ ox.st.used) := by
        intro k id hk hid
        have hle := hL k id hid
        have h1 : n * 1 ≤ n * uses body (k + 1) := Nat.mul_le_mul (Nat.le_refl n) hk
        refine ⟨by have := hg k; omega, fun _ => ?_⟩
        exact avail_seq hg hL hD hA hfx k id
          (by have := Nat.mul_le_mul hn hk; omega) hid
      obtain ⟨s', tr, hl, hwl, hfl⟩ :=
        loop_sound hf n vs ox.st hlen hall
          (fun d => by have := hB d; have := (hwx d).2; simp only [add_apply, scale_apply] at *; omega)
          hinv
      refine ⟨⟨.unit, s', ox.tr ++ tr⟩, by simp [eval, hex, hvs, hl], .unit, hwx.trans hwl, ?_⟩
      exact (hfx.mono fun k => by have := hg k; omega).trans (hfl.mono fun k => by have := hg k; omega)
  | @infer Γ pc m t a τa ιa βa _ iha =>
    intro ρ s hE hD hL hA hB
    obtain ⟨oa, hea, _, hwa, hfa⟩ := iha hE hD hL hA (fun d => by have := hB d; simp at this ⊢; omega)
    have hble : ble (oa.st.spent + S.mcost m) c.limit = true :=
      ble_iff.mpr fun d => by have := hB d; have := (hwa d).2; simp at *; omega
    refine ⟨⟨c.oracle oa.st.calls m t oa.v, ⟨oa.st.used, oa.st.spent + S.mcost m, oa.st.calls + 1⟩, oa.tr⟩,
      by simp [eval, hea, doInfer, hble], hO _ _ _ _, hwa.trans (Within.step _ _), hfa⟩

/-! ## Enunciados -/

/-- O ponto de partida de uma execução do agente: o ambiente realiza Γ, as capacidades têm
identidades distintas e nenhuma foi consumida ainda. -/
structure Start (Γ : List Bind) (ρ : List Val) (s : St) : Prop where
  env   : EnvOk Γ ρ
  caps  : CapsDistinct ρ
  fresh : ∀ k id : Nat, ρ[k]? = some (Val.cap id) → id ∉ s.used

theorem linear_rho {Γ : List Bind} {ρ : List Val} {e : Exp} (hE : EnvOk Γ ρ) (h : Linear Γ e) :
    LinRho ρ (uses e) := fun k _ hk => h k (hE.cap_pos hk)

/-- **Teorema T1 (solidez).** Um programa bem tipado, que passa no E1 e cujo orçamento
estático cabe no teto, executa sem erro. O valor final tem o tipo estático, e o gasto real
fica dentro de β em cada dimensão. Cada identidade acrescentada ao conjunto consumido
pertence a uma capacidade que o programa nomeia. -/
theorem soundness (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} {ρ : List Val} {s : St} (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (h0 : Start Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    ∃ o, eval S c e ρ s = .ok o ∧ HasTy o.v τ ∧ Within s.spent o.st.spent β ∧
      Footprint ρ (uses e) s.used o.st.used :=
  sound hO h h0.env h0.caps (linear_rho h0.env hlin)
    (fun k id _ hk => h0.fresh k id hk) hB

/-- **T1, progresso.** Nenhum dos quatro erros acontece: `stuck`, efeito sem capacidade
(`noCap`), reuso de capacidade linear (`reuse`) e estouro de orçamento (`budget`). -/
theorem progress (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} {ρ : List Val} {s : St} (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (h0 : Start Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    ∀ err, eval S c e ρ s ≠ .error err := by
  intro err herr
  obtain ⟨o, ho, _⟩ := soundness hO h hlin h0 hB
  rw [ho] at herr
  cases herr

/-- **T1, preservação.** Se a execução termina, o valor tem o tipo estático, o gasto real
fica dentro de β e, portanto, dentro do teto do agente. -/
theorem preservation (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} {ρ : List Val} {s : St} {o : Out} (h : HasType S Γ pc e τ ι β)
    (hlin : Linear Γ e) (h0 : Start Γ ρ s) (hB : s.spent + β ≤ c.limit)
    (he : eval S c e ρ s = .ok o) :
    HasTy o.v τ ∧ Within s.spent o.st.spent β ∧ o.st.spent ≤ c.limit := by
  obtain ⟨o', ho', hv, hw, _⟩ := soundness hO h hlin h0 hB
  rw [ho'] at he
  cases he
  exact ⟨hv, hw, fun d => by have h1 := hB d; have h2 := (hw d).2; simp at h1; omega⟩

/-! ## Capacidades lineares: nenhuma identidade é consumida duas vezes -/

theorem loop_nodup {f : Val → St → Except Err Out}
    (hf : ∀ v (s : St) o, f v s = .ok o → s.used.Nodup → o.st.used.Nodup) :
    ∀ (vs : List Val) (s : St) r, loop f vs s = .ok r → s.used.Nodup → r.1.used.Nodup
  | [], s, r, h, hn => by simp [loop] at h; subst h; exact hn
  | v :: vs, s, r, h, hn => by
    simp only [loop, bnd_eq_ok] at h
    obtain ⟨o, ho, r', hr', h⟩ := h
    cases h
    exact loop_nodup hf vs o.st r' hr' (hf v s o ho hn)

/-- O conjunto de capacidades consumidas nunca repete uma identidade. O avaliador recusa o
reuso, e o T1 garante que um programa bem tipado nunca é recusado. Juntos, os dois dão o
consumo linear. -/
theorem eval_nodup : ∀ (e : Exp) {ρ : List Val} {s : St} {o : Out},
    eval S c e ρ s = .ok o → s.used.Nodup → o.st.used.Nodup
  | .val _, ρ, s, o, h, hn => by simp [eval] at h; subst h; exact hn
  | .var k, ρ, s, o, h, hn => by
    simp only [eval] at h
    split at h
    · cases h; exact hn
    · cases h
  | .let_ e₁ e₂, ρ, s, o, h, hn => by
    simp only [eval, bnd_eq_ok] at h
    obtain ⟨o₁, h₁, o₂, h₂, h⟩ := h
    cases h
    have := eval_nodup e₂ h₂ (eval_nodup e₁ h₁ hn); exact this
  | .ite cnd t f, ρ, s, o, h, hn => by
    simp only [eval, bnd_eq_ok] at h
    obtain ⟨oc, hc, h⟩ := h
    split at h
    · obtain ⟨o', h', h⟩ := bnd_eq_ok.mp h
      cases h
      have hn' := eval_nodup cnd hc hn
      rcases cond_ok h' with ⟨_, h'⟩ | ⟨_, h'⟩
      · have := eval_nodup t h' hn'; exact this
      · have := eval_nodup f h' hn'; exact this
    · cases h
  | .prim op a b, ρ, s, o, h, hn => by
    simp only [eval, bnd_eq_ok] at h
    obtain ⟨o₁, h₁, o₂, h₂, h⟩ := h
    split at h
    · cases h; have := eval_nodup b h₂ (eval_nodup a h₁ hn); exact this
    · cases h
  | .endorse p e d, ρ, s, o, h, hn => by
    simp only [eval, bnd_eq_ok] at h
    obtain ⟨oe, he, h⟩ := h
    split at h
    · cases h; have := eval_nodup e he hn; exact this
    · obtain ⟨od, hd, h⟩ := bnd_eq_ok.mp h
      cases h; have := eval_nodup d hd (eval_nodup e he hn); exact this
  | .act t k a, ρ, s, o, h, hn => by
    simp only [eval, bnd_eq_ok] at h
    obtain ⟨oa, ha, h⟩ := h
    obtain ⟨id, _, hfree, _, rfl⟩ := doAct_ok h
    have := eval_nodup a ha hn
    exact List.nodup_cons.mpr ⟨hfree, this⟩
  | .for_ n xs body, ρ, s, o, h, hn => by
    simp only [eval, bnd_eq_ok] at h
    obtain ⟨ox, hx, h⟩ := h
    split at h
    · obtain ⟨r, hr, h⟩ := bnd_eq_ok.mp h
      cases h
      have hx' := eval_nodup xs hx hn
      have := loop_nodup (fun v s o ho hn => eval_nodup body ho hn) _ _ r hr hx'
      exact this
    · cases h
  | .infer m t a, ρ, s, o, h, hn => by
    simp only [eval, bnd_eq_ok] at h
    obtain ⟨oa, ha, h⟩ := h
    obtain ⟨_, rfl⟩ := doInfer_ok h
    have := eval_nodup a ha hn; exact this

end

end Aletheia
