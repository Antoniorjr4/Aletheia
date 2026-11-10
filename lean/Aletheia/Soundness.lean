import Aletheia.Eval

/-!
# Teorema T1: preservação e progresso

Para um avaliador total de passo grande, a solidez de tipos tem duas metades.

* **Progresso**: um programa bem tipado não chega a nenhum erro de execução. Ele não fica
  preso, não executa efeito sem capacidade nem sem concessão do runtime, não reusa
  capacidade linear e não ultrapassa o orçamento.
* **Preservação**: o valor final tem o tipo estático, e o gasto real fica dentro do
  orçamento estático β em cada uma das seis dimensões. O laço, em particular, gasta no
  máximo `n • β(corpo)`.

Este arquivo prova ainda o **consumo exato** de capacidades: o que a execução acrescenta ao
conjunto consumido é exatamente o conjunto de capacidades que `consumes` conta como
consumidas, sem repetição. Com `Linear`, são todas as capacidades do agente, cada uma uma
vez.

As hipóteses sobre o ponto de partida (`Start`) são estas: o ambiente realiza Γ; as
capacidades do agente têm identidades distintas, ainda não foram consumidas e são
concedidas pelo runtime para pelo menos os efeitos do seu tipo `Cap[E]`. Além disso, o
programa passa no E1 (`Linear`), o gasto inicial mais β cabe no teto, e o oráculo devolve
valores bem tipados, que é a validação de esquema do ARX.
-/

namespace Aletheia

open Lab Budget

/-! ## Invariantes de capacidade -/

/-- As capacidades do ambiente têm identidades distintas. -/
def CapsDistinct (ρ : List Val) : Prop :=
  ∀ i j id : Nat, ρ[i]? = some (Val.cap id) → ρ[j]? = some (Val.cap id) → i = j

/-- Cada capacidade do ambiente é consumida 0 ou 1 vez pela expressão (contagem `f`); se
for consumida, ainda está livre em `u`. -/
def Lin (ρ : List Val) (u : List Nat) (f : Nat → Option Nat) : Prop :=
  ∀ k id : Nat, ρ[k]? = some (Val.cap id) → ∃ m, f k = some m ∧ m ≤ 1 ∧ (m = 1 → id ∉ u)

/-- Consumo exato: a avaliação acrescenta a `u` exatamente as capacidades com contagem 1. -/
def Consumed (ρ : List Val) (f : Nat → Option Nat) (u u' : List Nat) : Prop :=
  ∃ new : List Nat, u' = new ++ u ∧
    ∀ id, id ∈ new ↔ ∃ k, ρ[k]? = some (Val.cap id) ∧ f k = some 1

/-- Decomposição de uma contagem em duas partes sequenciais. -/
def Splits (f f₁ f₂ : Nat → Option Nat) : Prop :=
  ∀ k m, f k = some m → ∃ x y, f₁ k = some x ∧ f₂ k = some y ∧ m = x + y

theorem CapsDistinct.cons {ρ : List Val} {v : Val} (hv : ∀ id, v ≠ .cap id)
    (h : CapsDistinct ρ) : CapsDistinct (v :: ρ) := by
  intro i j id hi hj
  cases i with
  | zero => simp at hi; exact absurd hi (hv id)
  | succ i =>
    cases j with
    | zero => simp at hj; exact absurd hj (hv id)
    | succ j => simp at hi hj; exact congrArg (· + 1) (h i j id hi hj)

theorem Lin.cons {ρ : List Val} {u : List Nat} {v : Val} {f : Nat → Option Nat}
    (hv : ∀ id, v ≠ .cap id) (h : Lin ρ u fun k => f (k + 1)) : Lin (v :: ρ) u f := by
  intro k id hk
  cases k with
  | zero => simp at hk; exact absurd hk (hv id)
  | succ k => exact h k id (by simpa using hk)

/-- A primeira parte de uma decomposição herda o invariante. -/
theorem Lin.left {ρ : List Val} {u : List Nat} {f f₁ f₂ : Nat → Option Nat}
    (hs : Splits f f₁ f₂) (h : Lin ρ u f) : Lin ρ u f₁ := by
  intro k id hk
  obtain ⟨m, hm, hle, hfree⟩ := h k id hk
  obtain ⟨x, y, hx, _, rfl⟩ := hs k m hm
  exact ⟨x, hx, by omega, fun h1 => hfree (by omega)⟩

/-- O que a segunda parte consome continua livre depois da primeira: nenhuma capacidade é
consumida pelas duas (E1) e capacidades distintas têm identidades distintas. -/
theorem Lin.seq {ρ : List Val} {u u₁ : List Nat} {f f₁ f₂ : Nat → Option Nat}
    (hs : Splits f f₁ f₂) (hD : CapsDistinct ρ) (h : Lin ρ u f) (h₁ : Consumed ρ f₁ u u₁) :
    Lin ρ u₁ f₂ := by
  intro k id hk
  obtain ⟨m, hm, hle, hfree⟩ := h k id hk
  obtain ⟨x, y, hx, hy, rfl⟩ := hs k m hm
  refine ⟨y, hy, by omega, fun hy1 => ?_⟩
  obtain ⟨new, rfl, hnew⟩ := h₁
  intro hmem
  rcases List.mem_append.mp hmem with hin | hin
  · obtain ⟨k', hk', hx'⟩ := (hnew id).mp hin
    have := hD k k' id hk hk'
    subst this
    rw [hx] at hx'
    cases hx'
    omega
  · exact hfree (by omega) hin

theorem Consumed.nil {ρ : List Val} {f : Nat → Option Nat} {u : List Nat}
    (hf : ∀ k id, ρ[k]? = some (Val.cap id) → f k ≠ some 1) : Consumed ρ f u u := by
  refine ⟨[], rfl, fun id => ⟨fun h => by simp at h, fun ⟨k, hk, h1⟩ => absurd h1 (hf k id hk)⟩⟩

/-- Uma parte sem consumo não altera o conjunto consumido. -/
theorem Consumed.unchanged {ρ : List Val} {f : Nat → Option Nat} {u u' : List Nat}
    (h : Consumed ρ f u u') (hf : ∀ k id, ρ[k]? = some (Val.cap id) → f k ≠ some 1) :
    u' = u := by
  obtain ⟨new, rfl, hnew⟩ := h
  cases new with
  | nil => rfl
  | cons x xs =>
    obtain ⟨k, hk, h1⟩ := (hnew x).mp (List.mem_cons_self ..)
    exact absurd h1 (hf k x hk)

theorem Consumed.single {ρ : List Val} {j id : Nat} {u : List Nat}
    (hid : ρ[j]? = some (Val.cap id)) :
    Consumed ρ (fun k => some (if j = k then 1 else 0)) u (id :: u) := by
  refine ⟨[id], rfl, fun x => ⟨fun h => ?_, fun ⟨k, hk, h1⟩ => ?_⟩⟩
  · simp at h; subst h; exact ⟨j, hid, by simp⟩
  · by_cases hjk : j = k
    · subst hjk; rw [hid] at hk; cases hk; simp
    · simp [hjk] at h1

theorem Consumed.uncons {ρ : List Val} {v : Val} {f : Nat → Option Nat} {u u' : List Nat}
    (hv : ∀ id, v ≠ .cap id) (h : Consumed (v :: ρ) f u u') :
    Consumed ρ (fun k => f (k + 1)) u u' := by
  obtain ⟨new, rfl, hnew⟩ := h
  refine ⟨new, rfl, fun id => ⟨fun hin => ?_, fun ⟨k, hk, h1⟩ => (hnew id).mpr ⟨k + 1, by simpa using hk, h1⟩⟩⟩
  obtain ⟨k, hk, h1⟩ := (hnew id).mp hin
  cases k with
  | zero => simp at hk; exact absurd hk (hv id)
  | succ k => exact ⟨k, by simpa using hk, h1⟩

/-- Composição sequencial do consumo exato. -/
theorem Consumed.seq {ρ : List Val} {u u₁ u₂ : List Nat} {f f₁ f₂ : Nat → Option Nat}
    (hs : Splits f f₁ f₂) (h : Lin ρ u f) (h₁ : Consumed ρ f₁ u u₁) (h₂ : Consumed ρ f₂ u₁ u₂) :
    Consumed ρ f u u₂ := by
  obtain ⟨n₁, rfl, hn₁⟩ := h₁
  obtain ⟨n₂, rfl, hn₂⟩ := h₂
  refine ⟨n₂ ++ n₁, by simp, fun id => ⟨fun hin => ?_, fun ⟨k, hk, h1⟩ => ?_⟩⟩
  · rcases List.mem_append.mp hin with hin | hin
    · obtain ⟨k, hk, hy⟩ := (hn₂ id).mp hin
      obtain ⟨m, hm, hle, _⟩ := h k id hk
      obtain ⟨x, y, _, hy', rfl⟩ := hs k m hm
      rw [hy] at hy'; cases hy'
      exact ⟨k, hk, by rw [hm]; congr 1; omega⟩
    · obtain ⟨k, hk, hx⟩ := (hn₁ id).mp hin
      obtain ⟨m, hm, hle, _⟩ := h k id hk
      obtain ⟨x, y, hx', _, rfl⟩ := hs k m hm
      rw [hx] at hx'; cases hx'
      exact ⟨k, hk, by rw [hm]; congr 1; omega⟩
  · obtain ⟨x, y, hx, hy, hxy⟩ := hs k 1 h1
    rcases Nat.eq_zero_or_pos x with hx0 | hx0
    · exact List.mem_append_left _ ((hn₂ id).mpr ⟨k, hk, by rw [hy]; congr 1; omega⟩)
    · exact List.mem_append_right _ ((hn₁ id).mpr ⟨k, hk, by rw [hx]; congr 1; omega⟩)

/-! ## Decomposições das regras -/

theorem splits_let (e₁ e₂ : Exp) :
    Splits (consumes (.let_ e₁ e₂)) (consumes e₁) (fun k => consumes e₂ (k + 1)) :=
  fun _ _ h => Count.add_eq h

theorem splits_prim (op : Op) (a b : Exp) :
    Splits (consumes (.prim op a b)) (consumes a) (consumes b) :=
  fun _ _ h => Count.add_eq h

theorem splits_ite_t (c t f : Exp) : Splits (consumes (.ite c t f)) (consumes c) (consumes t) := by
  intro k m h
  obtain ⟨x, y, hx, hy, rfl⟩ := Count.add_eq h
  exact ⟨x, y, hx, (Count.same_eq hy).1, rfl⟩

theorem splits_ite_f (c t f : Exp) : Splits (consumes (.ite c t f)) (consumes c) (consumes f) := by
  intro k m h
  obtain ⟨x, y, hx, hy, rfl⟩ := Count.add_eq h
  exact ⟨x, y, hx, (Count.same_eq hy).2, rfl⟩

theorem splits_endorse_d (p : Nat) (e d : Exp) :
    Splits (consumes (.endorse p e d)) (consumes e) (consumes d) := by
  intro k m h
  obtain ⟨x, y, hx, hy, rfl⟩ := Count.add_eq h
  obtain ⟨hd, rfl⟩ := Count.zero_eq hy
  exact ⟨x, 0, hx, hd, rfl⟩

theorem splits_endorse_e (p : Nat) (e d : Exp) :
    Splits (consumes (.endorse p e d)) (consumes e) (fun _ => some 0) := by
  intro k m h
  obtain ⟨x, y, hx, hy, rfl⟩ := Count.add_eq h
  obtain ⟨_, rfl⟩ := Count.zero_eq hy
  exact ⟨x, 0, hx, rfl, rfl⟩

theorem splits_act (t j : Nat) (a : Exp) :
    Splits (consumes (.act t j a)) (consumes a) (fun k => some (if j = k then 1 else 0)) :=
  fun _ _ h => Count.add_eq h

theorem splits_for (n : Nat) (xs body : Exp) :
    Splits (consumes (.for_ n xs body)) (consumes xs) (fun _ => some 0) := by
  intro k m h
  obtain ⟨x, y, hx, hy, rfl⟩ := Count.add_eq h
  obtain ⟨_, rfl⟩ := Count.zero_eq hy
  exact ⟨x, 0, hx, rfl, rfl⟩

/-- O corpo de um laço não consome capacidade de fora. -/
theorem for_body_zero {n : Nat} {xs body : Exp} {ρ : List Val} {u : List Nat}
    (h : Lin ρ u (consumes (.for_ n xs body))) :
    ∀ k id, ρ[k]? = some (Val.cap id) → consumes body (k + 1) = some 0 := by
  intro k id hk
  obtain ⟨m, hm, _, _⟩ := h k id hk
  obtain ⟨_, y, _, hy, _⟩ := Count.add_eq hm
  exact (Count.zero_eq hy).1

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

/-- Um laço cujo corpo não consome capacidade de fora gasta no máximo `n • β(corpo)` e não
altera o conjunto consumido. -/
theorem loop_sound {f : Val → St → Except Err Out} {τ : Ty} {βb L : Budget}
    (hf : ∀ v (s : St), HasTy v τ → s.spent + βb ≤ L →
      ∃ o, f v s = .ok o ∧ Within s.spent o.st.spent βb ∧ o.st.used = s.used) :
    ∀ (n : Nat) (vs : List Val) (s : St), vs.length ≤ n → (∀ v ∈ vs, HasTy v τ) →
      s.spent + n • βb ≤ L →
      ∃ s' tr, loop f vs s = .ok (s', tr) ∧ Within s.spent s'.spent (n • βb) ∧ s'.used = s.used
  | n, [], s, _, _, _ => ⟨s, [], rfl, Within.refl _ _, rfl⟩
  | n, v :: vs, s, hlen, hall, hB => by
    obtain ⟨n', rfl⟩ : ∃ n', n = n' + 1 := ⟨n - 1, by simp at hlen; omega⟩
    have hB₁ : s.spent + βb ≤ L := fun d => by
      have := hB d; simp only [add_apply, scale_apply, Nat.add_mul, Nat.one_mul] at this ⊢; omega
    obtain ⟨o, he, hw, hu⟩ := hf v s (hall v (List.mem_cons_self ..)) hB₁
    have hB₂ : o.st.spent + n' • βb ≤ L := fun d => by
      have := hB d; have := (hw d).2
      simp only [add_apply, scale_apply, Nat.add_mul, Nat.one_mul] at *; omega
    obtain ⟨s', tr, hl, hw', hu'⟩ :=
      loop_sound hf n' vs o.st (by simp at hlen; omega) (fun x hx => hall x (List.mem_cons_of_mem _ hx)) hB₂
    refine ⟨s', o.tr ++ tr, by simp [loop, he, hl], ?_, by rw [hu', hu]⟩
    refine (hw.trans hw').mono fun d => ?_
    simp only [add_apply, scale_apply, Nat.add_mul, Nat.one_mul]; omega

/-! ## O teorema -/

section
variable {S : Sig} {c : Cfg}

theorem sound (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} (h : HasType S Γ pc e τ ι β) :
    ∀ {ρ : List Val} {s : St}, EnvOk Γ ρ → CapsDistinct ρ → Granted c.grants Γ ρ →
      Lin ρ s.used (consumes e) → s.spent + β ≤ c.limit →
      ∃ o, eval S c e ρ s = .ok o ∧ HasTy o.v τ ∧ Within s.spent o.st.spent β ∧
        Consumed ρ (consumes e) s.used o.st.used := by
  induction h with
  | val hv =>
    intro ρ s _ _ _ _ _
    exact ⟨⟨_, s, []⟩, rfl, hv, Within.refl _ _, Consumed.nil fun _ _ _ => by simp [consumes]⟩
  | var hk =>
    intro ρ s hE _ _ _ _
    obtain ⟨v, hρ, hv⟩ := hE.data_lookup hk
    exact ⟨⟨v, s, []⟩, by simp [eval, hρ], hv, Within.refl _ _,
      Consumed.nil fun _ _ _ => by simp [consumes]⟩
  | @let_ Γ pc e₁ e₂ τ₁ τ₂ ι₁ ι₂ β₁ β₂ _ _ ih₁ ih₂ =>
    intro ρ s hE hD hG hL hB
    have hs := splits_let e₁ e₂
    obtain ⟨o₁, he₁, hv₁, hw₁, hc₁⟩ :=
      ih₁ hE hD hG (hL.left hs) (fun d => by have := hB d; simp at this ⊢; omega)
    have hvc := hv₁.not_cap
    obtain ⟨o₂, he₂, hv₂, hw₂, hc₂⟩ :=
      ih₂ (.data hv₁ hE) (hD.cons hvc) hG.cons (Lin.cons hvc (hL.seq hs hD hc₁))
        (fun d => by have := hB d; have := (hw₁ d).2; simp at *; omega)
    exact ⟨⟨o₂.v, o₂.st, o₁.tr ++ o₂.tr⟩, by simp [eval, he₁, he₂], hv₂, hw₁.trans hw₂,
      Consumed.seq hs hL hc₁ (hc₂.uncons hvc)⟩
  | @ite Γ pc cnd t f τ ιc ιt ιf βc βt βf _ _ _ ihc iht ihf =>
    intro ρ s hE hD hG hL hB
    have hst := splits_ite_t cnd t f
    have hsf := splits_ite_f cnd t f
    obtain ⟨oc, hec, hvc, hwc, hcc⟩ :=
      ihc hE hD hG (hL.left hst) (fun d => by have := hB d; simp at this ⊢; omega)
    obtain ⟨b, hb⟩ := hvc.bool_inv
    cases b with
    | true =>
      obtain ⟨o', he', hv', hw', hc'⟩ :=
        iht hE hD hG (hL.seq hst hD hcc)
          (fun d => by have := hB d; have := (hwc d).2; simp at *; omega)
      refine ⟨⟨o'.v, o'.st, oc.tr ++ o'.tr⟩, by simp [eval, hec, hb, he'], hv', ?_,
        Consumed.seq hst hL hcc hc'⟩
      exact (hwc.trans hw').mono fun d => by simp; omega
    | false =>
      obtain ⟨o', he', hv', hw', hc'⟩ :=
        ihf hE hD hG (hL.seq hsf hD hcc)
          (fun d => by have := hB d; have := (hwc d).2; simp at *; omega)
      refine ⟨⟨o'.v, o'.st, oc.tr ++ o'.tr⟩, by simp [eval, hec, hb, he'], hv', ?_,
        Consumed.seq hsf hL hcc hc'⟩
      exact (hwc.trans hw').mono fun d => by simp; omega
  | @prim Γ pc op a b ιa ιb βa βb _ _ iha ihb =>
    intro ρ s hE hD hG hL hB
    have hs := splits_prim op a b
    obtain ⟨oa, hea, hva, hwa, hca⟩ :=
      iha hE hD hG (hL.left hs) (fun d => by have := hB d; simp at this ⊢; omega)
    obtain ⟨ob, heb, hvb, hwb, hcb⟩ :=
      ihb hE hD hG (hL.seq hs hD hca)
        (fun d => by have := hB d; have := (hwa d).2; simp at *; omega)
    obtain ⟨x, hx⟩ := hva.int_inv
    obtain ⟨y, hy⟩ := hvb.int_inv
    exact ⟨⟨op.apply x y, ob.st, oa.tr ++ ob.tr⟩, by simp [eval, hea, heb, hx, hy],
      op.apply_ty x y, hwa.trans hwb, Consumed.seq hs hL hca hcb⟩
  | @endorse Γ pc p e d τ ιe ιd βe βd _ _ _ ihe ihd =>
    intro ρ s hE hD hG hL hB
    have hsd := splits_endorse_d p e d
    have hse := splits_endorse_e p e d
    obtain ⟨oe, hee, hve, hwe, hce⟩ :=
      ihe hE hD hG (hL.left hsd) (fun d => by have := hB d; simp at this ⊢; omega)
    by_cases hp : S.pol p oe.v = true
    · exact ⟨⟨oe.v, oe.st, oe.tr ++ [.endorse p (some oe.v)]⟩, by simp [eval, hee, hp], hve,
        hwe.mono fun d => by simp,
        Consumed.seq hse hL hce (Consumed.nil fun _ _ _ => by simp)⟩
    · obtain ⟨od, hed, hvd, hwd, hcd⟩ :=
        ihd hE hD hG (hL.seq hsd hD hce)
          (fun d => by have := hB d; have := (hwe d).2; simp at *; omega)
      exact ⟨⟨od.v, od.st, oe.tr ++ .endorse p none :: od.tr⟩, by simp [eval, hee, hp, hed], hvd,
        hwe.trans hwd, Consumed.seq hsd hL hce hcd⟩
  | @act Γ pc t j a ιa βa E _ hk heff _ _ iha =>
    intro ρ s hE hD hG hL hB
    have hs := splits_act t j a
    obtain ⟨oa, hea, _, hwa, hca⟩ :=
      iha hE hD hG (hL.left hs) (fun d => by have := hB d; simp at this ⊢; omega)
    obtain ⟨id, hid⟩ := hE.cap_lookup hk
    have hgrant : S.effect t ∈ c.grants id := hG j E id hk hid _ heff
    have hfree : id ∉ oa.st.used := by
      obtain ⟨m, hm, _, hm1⟩ := hL.seq hs hD hca j id hid
      simp at hm; subst hm; exact hm1 rfl
    have hble : ble (oa.st.spent + S.cost t) c.limit = true :=
      ble_iff.mpr fun d => by have := hB d; have := (hwa d).2; simp at *; omega
    exact ⟨⟨.unit, ⟨id :: oa.st.used, oa.st.spent + S.cost t, oa.st.calls, S.step t oa.v oa.st.w,
      (t, oa.v) :: oa.st.comp⟩, oa.tr ++ [.act t id oa.v]⟩,
      by simp [eval, hea, doAct, hid, hgrant, hfree, hble], .unit, hwa.trans (Within.step _ _),
      Consumed.seq hs hL hca (Consumed.single hid)⟩
  | @drop Γ pc j E hk =>
    intro ρ s hE _ _ hL _
    obtain ⟨id, hid⟩ := hE.cap_lookup hk
    have hfree : id ∉ s.used := by
      obtain ⟨m, hm, _, hm1⟩ := hL j id hid
      simp [consumes] at hm; subst hm; exact hm1 rfl
    exact ⟨⟨.unit, ⟨id :: s.used, s.spent, s.calls, s.w, s.comp⟩, []⟩, by simp [eval, doDrop, hid, hfree], .unit,
      Within.refl _ _, Consumed.single hid⟩
  | @for_ Γ pc n xs body τ τb ιx ιb βx βb _ _ ihx ihb =>
    intro ρ s hE hD hG hL hB
    have hs := splits_for n xs body
    obtain ⟨ox, hex, hvx, hwx, hcx⟩ :=
      ihx hE hD hG (hL.left hs) (fun d => by have := hB d; simp only [add_apply] at this ⊢; omega)
    obtain ⟨vs, hvs, hall, hlen⟩ := hvx.list_inv
    have hzero := for_body_zero hL
    have hf : ∀ v (s' : St), HasTy v τ → s'.spent + βb ≤ c.limit →
        ∃ o, eval S c body (v :: ρ) s' = .ok o ∧ Within s'.spent o.st.spent βb ∧
          o.st.used = s'.used := by
      intro v s' hv hb
      have hvc := hv.not_cap
      have hLb : Lin (v :: ρ) s'.used (consumes body) :=
        Lin.cons hvc fun k id hk => ⟨0, hzero k id hk, by omega, by omega⟩
      obtain ⟨o, he, _, hw, hcb⟩ := ihb (.data hv hE) (hD.cons hvc) hG.cons hLb hb
      refine ⟨o, he, hw, (hcb.uncons hvc).unchanged fun k id hk => ?_⟩
      simp [hzero k id hk]
    obtain ⟨s', tr, hl, hwl, hul⟩ :=
      loop_sound hf n vs ox.st hlen hall
        (fun d => by have := hB d; have := (hwx d).2; simp only [add_apply, scale_apply] at *; omega)
    refine ⟨⟨.unit, s', ox.tr ++ tr⟩, by simp [eval, hex, hvs, hl], .unit, hwx.trans hwl, ?_⟩
    exact Consumed.seq hs hL hcx (by rw [hul]; exact Consumed.nil fun _ _ _ => by simp)
  | @infer Γ pc m t a τa ιa βa _ iha =>
    intro ρ s hE hD hG hL hB
    obtain ⟨oa, hea, _, hwa, hca⟩ := iha hE hD hG hL (fun d => by have := hB d; simp at this ⊢; omega)
    have hble : ble (oa.st.spent + S.mcost m) c.limit = true :=
      ble_iff.mpr fun d => by have := hB d; have := (hwa d).2; simp at *; omega
    exact ⟨⟨c.oracle oa.st.calls m t oa.v, ⟨oa.st.used, oa.st.spent + S.mcost m, oa.st.calls + 1, oa.st.w,
      oa.st.comp⟩, oa.tr⟩,
      by simp [eval, hea, doInfer, hble], hO _ _ _ _, hwa.trans (Within.step _ _), hca⟩

/-! ## Enunciados -/

/-- O ponto de partida de uma execução do agente: o ambiente realiza Γ; as capacidades têm
identidades distintas, nenhuma foi consumida e o runtime concede a cada uma pelo menos os
efeitos do seu tipo `Cap[E]`. -/
structure Start (G : Nat → List EffectId) (Γ : List Bind) (ρ : List Val) (s : St) : Prop where
  env     : EnvOk Γ ρ
  caps    : CapsDistinct ρ
  granted : Granted G Γ ρ
  fresh   : ∀ k id : Nat, ρ[k]? = some (Val.cap id) → id ∉ s.used

theorem linear_lin {Γ : List Bind} {ρ : List Val} {s : St} {e : Exp} (hE : EnvOk Γ ρ)
    (hfresh : ∀ k id : Nat, ρ[k]? = some (Val.cap id) → id ∉ s.used) (h : Linear Γ e) :
    Lin ρ s.used (consumes e) := by
  intro k id hk
  obtain ⟨E, hE'⟩ := hE.cap_pos hk
  exact ⟨1, h k E hE', Nat.le_refl _, fun _ => hfresh k id hk⟩

/-- **Teorema T1 (solidez).** Um programa bem tipado e linear, cujo orçamento estático cabe
no teto, executa sem erro a partir de um ponto de partida válido. O valor final tem o tipo
estático, o gasto real fica dentro de β em cada dimensão, e o conjunto consumido ganha
exatamente as capacidades do agente. -/
theorem soundness (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} {ρ : List Val} {s : St} (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    ∃ o, eval S c e ρ s = .ok o ∧ HasTy o.v τ ∧ Within s.spent o.st.spent β ∧
      ∃ new : List Nat, o.st.used = new ++ s.used ∧
        ∀ id : Nat, id ∈ new ↔ ∃ k : Nat, ρ[k]? = some (Val.cap id) := by
  obtain ⟨o, he, hv, hw, new, hnew, hmem⟩ :=
    sound hO h h0.env h0.caps h0.granted (linear_lin h0.env h0.fresh hlin) hB
  refine ⟨o, he, hv, hw, new, hnew, fun id => ⟨fun hin => ?_, fun ⟨k, hk⟩ => ?_⟩⟩
  · obtain ⟨k, hk, _⟩ := (hmem id).mp hin
    exact ⟨k, hk⟩
  · obtain ⟨E, hE'⟩ := h0.env.cap_pos hk
    exact (hmem id).mpr ⟨k, hk, hlin k E hE'⟩

/-- **T1, progresso.** Nenhum dos quatro erros acontece: `stuck`, efeito sem capacidade ou
sem concessão (`noCap`), reuso de capacidade linear (`reuse`) e estouro de orçamento
(`budget`). -/
theorem progress (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} {ρ : List Val} {s : St} (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    ∀ err, eval S c e ρ s ≠ .error err := by
  intro err herr
  obtain ⟨o, ho, _⟩ := soundness hO h hlin h0 hB
  rw [ho] at herr
  cases herr

/-- **T1, preservação.** Se a execução termina, o valor tem o tipo estático, o gasto real
fica dentro de β e, portanto, dentro do teto do agente. -/
theorem preservation (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} {ρ : List Val} {s : St} {o : Out} (h : HasType S Γ pc e τ ι β)
    (hlin : Linear Γ e) (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit)
    (he : eval S c e ρ s = .ok o) :
    HasTy o.v τ ∧ Within s.spent o.st.spent β ∧ o.st.spent ≤ c.limit := by
  obtain ⟨o', ho', hv, hw, _⟩ := soundness hO h hlin h0 hB
  rw [ho'] at he
  cases he
  exact ⟨hv, hw, fun d => by have h1 := hB d; have h2 := (hw d).2; simp at h1; omega⟩

/-! ## O avaliador nunca consome uma capacidade duas vezes -/

/-- `Fresh u u'`: `u'` estende `u` com identidades novas, sem repetição. -/
def Fresh (u u' : List Nat) : Prop := ∃ new : List Nat, u' = new ++ u ∧ new.Nodup ∧ ∀ x ∈ new, x ∉ u

theorem Fresh.refl (u : List Nat) : Fresh u u := ⟨[], rfl, List.nodup_nil, by simp⟩

theorem Fresh.trans {u₁ u₂ u₃ : List Nat} (h₁ : Fresh u₁ u₂) (h₂ : Fresh u₂ u₃) : Fresh u₁ u₃ := by
  obtain ⟨n₁, rfl, hd₁, hf₁⟩ := h₁
  obtain ⟨n₂, rfl, hd₂, hf₂⟩ := h₂
  refine ⟨n₂ ++ n₁, by simp, List.pairwise_append.mpr ⟨hd₂, hd₁, fun x hx₂ y hy₁ hxy => ?_⟩,
    fun x hx => ?_⟩
  · subst hxy
    exact hf₂ x hx₂ (List.mem_append_left _ hy₁)
  · rcases List.mem_append.mp hx with hx | hx
    · exact fun hu => hf₂ x hx (List.mem_append_right _ hu)
    · exact hf₁ x hx

theorem Fresh.cons {u : List Nat} {id : Nat} (h : id ∉ u) : Fresh u (id :: u) :=
  ⟨[id], rfl, List.pairwise_singleton _ _, by simpa using h⟩

theorem loop_fresh {f : Val → St → Except Err Out}
    (hf : ∀ v (s : St) o, f v s = .ok o → Fresh s.used o.st.used) :
    ∀ (vs : List Val) (s : St) r, loop f vs s = .ok r → Fresh s.used r.1.used
  | [], s, r, h => by simp [loop] at h; subst h; exact Fresh.refl _
  | v :: vs, s, r, h => by
    simp only [loop, bnd_eq_ok] at h
    obtain ⟨o, ho, r', hr', h⟩ := h
    cases h
    exact (hf v s o ho).trans (loop_fresh hf vs o.st r' hr')

/-- O conjunto consumido só cresce, com identidades novas: no sucesso e no `timeout`. -/
theorem fresh_preserves : Preserves S c True (fun s s' _ => Fresh s.used s'.used) where
  refl _ := Fresh.refl _
  trans h₁ h₂ := h₁.trans h₂
  endorse _ := Fresh.refl _
  reject := Fresh.refl _
  act _ hfree := Fresh.cons hfree
  drop hfree := Fresh.cons hfree
  infer := Fresh.refl _
  commit _ := fun h => h
  abort _ := fun _ _ _ h => h

/-- Por construção, o avaliador só acrescenta ao conjunto consumido identidades que ainda
não estavam lá, e nunca a mesma duas vezes. -/
theorem eval_fresh (e : Exp) {ρ : List Val} {s : St} {o : Out}
    (h : eval S c e ρ s = .ok o) : Fresh s.used o.st.used :=
  (eval_preserves fresh_preserves e (.inl trivial)).1 h

theorem eval_nodup {e : Exp} {ρ : List Val} {s : St} {o : Out}
    (h : eval S c e ρ s = .ok o) (hn : s.used.Nodup) : o.st.used.Nodup := by
  obtain ⟨new, hu, hd, hf⟩ := eval_fresh e h
  rw [hu]
  exact List.pairwise_append.mpr ⟨hd, hn, fun x hx y hy hxy => hf x hx (hxy ▸ hy)⟩

end

end Aletheia
