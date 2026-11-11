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

/-- Consumo parcial: tudo o que a avaliação acrescentou a `u` tem contagem 1. Uma execução
interrompida por `timeout` pode não ter chegado a consumir o resto. -/
def Partial (ρ : List Val) (f : Nat → Option Nat) (u u' : List Nat) : Prop :=
  ∃ new : List Nat, u' = new ++ u ∧ ∀ id ∈ new, ∃ k, ρ[k]? = some (Val.cap id) ∧ f k = some 1

theorem Consumed.toPartial {ρ : List Val} {f : Nat → Option Nat} {u u' : List Nat}
    (h : Consumed ρ f u u') : Partial ρ f u u' := by
  obtain ⟨new, rfl, hnew⟩ := h
  exact ⟨new, rfl, fun id hin => (hnew id).mp hin⟩

/-- O que a segunda parte consome continua livre depois da primeira, mesmo que a primeira
tenha parado no meio: nenhuma capacidade é consumida pelas duas (E1) e capacidades
distintas têm identidades distintas. -/
theorem Lin.seqP {ρ : List Val} {u u₁ : List Nat} {f f₁ f₂ : Nat → Option Nat}
    (hs : Splits f f₁ f₂) (hD : CapsDistinct ρ) (h : Lin ρ u f) (h₁ : Partial ρ f₁ u u₁) :
    Lin ρ u₁ f₂ := by
  intro k id hk
  obtain ⟨m, hm, hle, hfree⟩ := h k id hk
  obtain ⟨x, y, hx, hy, rfl⟩ := hs k m hm
  refine ⟨y, hy, by omega, fun hy1 => ?_⟩
  obtain ⟨new, rfl, hnew⟩ := h₁
  intro hmem
  rcases List.mem_append.mp hmem with hin | hin
  · obtain ⟨k', hk', hx'⟩ := hnew id hin
    have := hD k k' id hk hk'
    subst this
    rw [hx] at hx'
    cases hx'
    omega
  · exact hfree (by omega) hin

theorem Lin.seq {ρ : List Val} {u u₁ : List Nat} {f f₁ f₂ : Nat → Option Nat}
    (hs : Splits f f₁ f₂) (hD : CapsDistinct ρ) (h : Lin ρ u f) (h₁ : Consumed ρ f₁ u u₁) :
    Lin ρ u₁ f₂ :=
  Lin.seqP hs hD h h₁.toPartial

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

theorem Partial.refl (ρ : List Val) (f : Nat → Option Nat) (u : List Nat) : Partial ρ f u u :=
  ⟨[], rfl, by simp⟩

/-- Composição sequencial do consumo parcial. -/
theorem Partial.seq {ρ : List Val} {u u₁ u₂ : List Nat} {f f₁ f₂ : Nat → Option Nat}
    (hs : Splits f f₁ f₂) (h : Lin ρ u f) (h₁ : Partial ρ f₁ u u₁) (h₂ : Partial ρ f₂ u₁ u₂) :
    Partial ρ f u u₂ := by
  obtain ⟨n₁, rfl, hn₁⟩ := h₁
  obtain ⟨n₂, rfl, hn₂⟩ := h₂
  refine ⟨n₂ ++ n₁, by simp, fun id hin => ?_⟩
  rcases List.mem_append.mp hin with hin | hin
  · obtain ⟨k, hk, hy⟩ := hn₂ id hin
    obtain ⟨m, hm, hle, _⟩ := h k id hk
    obtain ⟨x, y, _, hy', rfl⟩ := hs k m hm
    rw [hy] at hy'; cases hy'
    exact ⟨k, hk, by rw [hm]; congr 1; omega⟩
  · obtain ⟨k, hk, hx⟩ := hn₁ id hin
    obtain ⟨m, hm, hle, _⟩ := h k id hk
    obtain ⟨x, y, hx', _, rfl⟩ := hs k m hm
    rw [hx] at hx'; cases hx'
    exact ⟨k, hk, by rw [hm]; congr 1; omega⟩

/-- Parar na primeira parte é um consumo parcial do todo. -/
theorem Partial.left {ρ : List Val} {u u₁ : List Nat} {f f₁ f₂ : Nat → Option Nat}
    (h₁ : Partial ρ f₁ u u₁) (hs : Splits f f₁ f₂) (h : Lin ρ u f) : Partial ρ f u u₁ :=
  Partial.seq hs h h₁ (Partial.refl _ _ _)

theorem Partial.uncons {ρ : List Val} {v : Val} {f : Nat → Option Nat} {u u' : List Nat}
    (hv : ∀ id, v ≠ .cap id) (h : Partial (v :: ρ) f u u') :
    Partial ρ (fun k => f (k + 1)) u u' := by
  obtain ⟨new, rfl, hnew⟩ := h
  refine ⟨new, rfl, fun id hin => ?_⟩
  obtain ⟨k, hk, h1⟩ := hnew id hin
  cases k with
  | zero => simp at hk; exact absurd hk (hv id)
  | succ k => exact ⟨k, by simpa using hk, h1⟩

theorem Partial.unchanged {ρ : List Val} {f : Nat → Option Nat} {u u' : List Nat}
    (h : Partial ρ f u u') (hf : ∀ k id, ρ[k]? = some (Val.cap id) → f k ≠ some 1) :
    u' = u := by
  obtain ⟨new, rfl, hnew⟩ := h
  cases new with
  | nil => rfl
  | cons x xs =>
    obtain ⟨k, hk, h1⟩ := hnew x (List.mem_cons_self ..)
    exact absurd h1 (hf k x hk)

/-- **A saída da saga fecha a conta.** O caminho percorrido consumiu uma parte das
capacidades da saga; `exitSaga` revoga as restantes. Juntas, são exatamente as capacidades
que `consumes` conta: nenhuma fica viva depois da saga e nenhuma é contada duas vezes. -/
theorem Partial.exit {ρ : List Val} {f : Nat → Option Nat} {u u' : List Nat}
    (hL : Lin ρ u f) (h : Partial ρ f u u') : Consumed ρ f u (revoke ρ f u' ++ u') := by
  obtain ⟨new, rfl, hnew⟩ := h
  refine ⟨revoke ρ f (new ++ u) ++ new, by simp, fun id => ⟨fun hin => ?_, fun ⟨k, hk, h1⟩ => ?_⟩⟩
  · rcases List.mem_append.mp hin with hin | hin
    · exact (mem_revoke.mp hin).1
    · exact hnew id hin
  · by_cases hm : id ∈ new ++ u
    · rcases List.mem_append.mp hm with hm | hm
      · exact List.mem_append_right _ hm
      · obtain ⟨m, hm', _, hfree⟩ := hL k id hk
        rw [h1] at hm'; cases hm'
        exact absurd hm (hfree rfl)
    · exact List.mem_append_left _ (mem_revoke.mpr ⟨⟨k, hk, h1⟩, hm⟩)

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

theorem splits_atomic_f (body : Exp) (r : Nat) (f g : Exp) :
    Splits (consumes (.atomic body r f g)) (consumes body) (consumes f) := by
  intro k m h
  obtain ⟨x, y, hx, hy, rfl⟩ := Count.add_eq h
  exact ⟨x, y, hx, (Count.same_eq hy).1, rfl⟩

theorem splits_atomic_g (body : Exp) (r : Nat) (f g : Exp) :
    Splits (consumes (.atomic body r f g)) (consumes body) (consumes g) := by
  intro k m h
  obtain ⟨x, y, hx, hy, rfl⟩ := Count.add_eq h
  exact ⟨x, y, hx, (Count.same_eq hy).2, rfl⟩

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

/-! ## Os dois desfechos -/

/-- A causa de uma falha bem tipada: se for o encerramento pelo tratador de
`TimeoutUnknown`, o valor tem o tipo de retorno do agente. -/
def CauseOk (S : Sig) : Cause → Prop
  | .abort v => HasTy v S.ret
  | _ => True

/-- O desfecho de uma avaliação bem tipada. Ou ela termina com um valor do tipo estático,
dentro do orçamento e com consumo exato; ou ela falha por uma causa que uma saga captura
(pós-condição não observada, operação de chave recusada, encerramento pelo tratador), e a
falha sobe com o gasto dentro do orçamento, um consumo parcial e, no encerramento, um valor
do tipo de retorno do agente. -/
abbrev Sound (S : Sig) (ρ : List Val) (f : Nat → Option Nat) (τ : Ty) (β : Budget) (s : St)
    (r : Except Err Out) : Prop :=
  (∃ o, r = .ok o ∧ HasTy o.v τ ∧ Within s.spent o.st.spent β ∧ Consumed ρ f s.used o.st.used) ∨
  (∃ q s₁ tr, r = .error (.timeout q s₁ tr) ∧ Within s.spent s₁.spent β ∧ Partial ρ f s.used s₁.used ∧
    CauseOk S q)

/-! ## O laço -/

/-- Um laço cujo corpo não consome capacidade de fora gasta no máximo `n • β(corpo)` e não
altera o conjunto consumido, termine ele normalmente ou num `timeout`. -/
theorem loop_sound {S : Sig} {f : Val → St → Except Err Out} {τ : Ty} {βb L : Budget}
    {P : St → Prop}
    (hf : ∀ v (s : St), P s → HasTy v τ → s.spent + βb ≤ L →
      (∃ o, f v s = .ok o ∧ Within s.spent o.st.spent βb ∧ o.st.used = s.used ∧ P o.st) ∨
      (∃ q s₁ tr, f v s = .error (.timeout q s₁ tr) ∧ Within s.spent s₁.spent βb ∧
        s₁.used = s.used ∧ CauseOk S q)) :
    ∀ (n : Nat) (vs : List Val) (s : St), P s → vs.length ≤ n → (∀ v ∈ vs, HasTy v τ) →
      s.spent + n • βb ≤ L →
      (∃ s' tr, loop f vs s = .ok (s', tr) ∧ Within s.spent s'.spent (n • βb) ∧ s'.used = s.used) ∨
      (∃ q s₁ tr, loop f vs s = .error (.timeout q s₁ tr) ∧ Within s.spent s₁.spent (n • βb) ∧
        s₁.used = s.used ∧ CauseOk S q)
  | n, [], s, _, _, _, _ => .inl ⟨s, [], rfl, Within.refl _ _, rfl⟩
  | n, v :: vs, s, hP, hlen, hall, hB => by
    obtain ⟨n', rfl⟩ : ∃ n', n = n' + 1 := ⟨n - 1, by simp at hlen; omega⟩
    have hB₁ : s.spent + βb ≤ L := fun d => by
      have := hB d; simp only [add_apply, scale_apply, Nat.add_mul, Nat.one_mul] at this ⊢; omega
    rcases hf v s hP (hall v (List.mem_cons_self ..)) hB₁ with ⟨o, he, hw, hu, hPo⟩ | ⟨q, s₁, tr, he, hw, hu, hq⟩
    · have hB₂ : o.st.spent + n' • βb ≤ L := fun d => by
        have := hB d; have := (hw d).2
        simp only [add_apply, scale_apply, Nat.add_mul, Nat.one_mul] at *; omega
      rcases loop_sound hf n' vs o.st hPo (by simp at hlen; omega)
          (fun x hx => hall x (List.mem_cons_of_mem _ hx)) hB₂ with
        ⟨s', tr, hl, hw', hu'⟩ | ⟨q, s₁, tr, hl, hw', hu', hq⟩
      · refine .inl ⟨s', o.tr ++ tr, by simp [loop, he, hl], ?_, by rw [hu', hu]⟩
        refine (hw.trans hw').mono fun d => ?_
        simp only [add_apply, scale_apply, Nat.add_mul, Nat.one_mul]; omega
      · refine .inr ⟨q, s₁, o.tr ++ tr, by simp only [loop, he, hl, bnd_ok, bnd_error] <;> rfl, ?_,
          by rw [hu', hu], hq⟩
        refine (hw.trans hw').mono fun d => ?_
        simp only [add_apply, scale_apply, Nat.add_mul, Nat.one_mul]; omega
    · refine .inr ⟨q, s₁, tr, by simp only [loop, he, bnd_error], hw.mono fun d => ?_, hu, hq⟩
      simp only [scale_apply, Nat.add_mul, Nat.one_mul]; omega

/-! ## O tratador puro

O tratador de `TimeoutUnknown` roda num ambiente fechado: dados ligados por `let` sobre a
obrigação, que fica no fundo. Bem tipado e puro, ele termina, não mexe no estado, e cada
`transfer` que executa fecha a obrigação. Quantos executa, diz `consumes`: com o E1 (uma vez
em todo caminho), exatamente um. É a ponte entre a regra estática (`consumes e 0 = some 1`) e
a guarda dinâmica (`transferOnly`) do avaliador. -/

theorem pure_handler {S : Sig} {c : Cfg} {t id : Nat} {Γ : List Bind} {pc : Lab} {e : Exp}
    {τ : Ty} {ι : Lab} {β : Budget} (h : HasType S Γ pc e τ ι β) :
    pureH e = true → ∀ {Γd : List Bind} {ds : List Val} {s : St} {m : Nat}, Γ = Γd ++ [.pend] →
      EnvOk Γd ds → (∀ j : Nat, Γd[j]? ≠ some Bind.pend) → consumes e ds.length = some m →
      ∃ o, eval S c e (ds ++ [.pend t id]) s = .ok o ∧ o.st = s ∧ HasTy o.v τ ∧
        o.tr = List.replicate m (.handoff id) := by
  induction h with
  | val hv =>
    intro _ Γd ds s m _ _ _ hm
    simp [consumes] at hm; subst hm
    exact ⟨⟨_, s, []⟩, rfl, rfl, hv, rfl⟩
  | @var Γ pc k τ ι hk =>
    intro _ Γd ds s m hΓ hE hnp hm
    simp [consumes] at hm; subst hm; subst hΓ
    have hlt : k < Γd.length := by
      rcases Nat.lt_or_ge k Γd.length with hlt | hge
      · exact hlt
      · have : (Γd ++ [Bind.pend])[k]? = [Bind.pend][k - Γd.length]? :=
          List.getElem?_append_right hge
        rw [this] at hk
        cases hkd : k - Γd.length with
        | zero => simp [hkd] at hk
        | succ x => simp [hkd] at hk
    have hk' : Γd[k]? = some (.data τ ι) := by rwa [List.getElem?_append_left hlt] at hk
    obtain ⟨v, hv, hty⟩ := hE.data_lookup hk'
    have hlen : ds.length = Γd.length := hE.length
    refine ⟨⟨v, s, []⟩, ?_, rfl, hty, rfl⟩
    simp only [eval]
    rw [List.getElem?_append_left (by omega), hv]
  | @let_ Γ pc e₁ e₂ τ₁ τ₂ ι₁ ι₂ β₁ β₂ _ _ ih₁ ih₂ =>
    intro hp Γd ds s m hΓ hE hnp hm
    simp only [pureH, Bool.and_eq_true] at hp
    obtain ⟨x, y, hx, hy, rfl⟩ := Count.add_eq hm
    obtain ⟨o₁, he₁, hs₁, hv₁, ht₁⟩ := ih₁ hp.1 hΓ hE hnp hx
    have hnp' : ∀ j : Nat, (Bind.data τ₁ ι₁ :: Γd)[j]? ≠ some Bind.pend := by
      intro j; cases j with
      | zero => simp
      | succ j => simpa using hnp j
    obtain ⟨o₂, he₂, hs₂, hv₂, ht₂⟩ := ih₂ hp.2 (Γd := .data τ₁ ι₁ :: Γd) (ds := o₁.v :: ds) (s := o₁.st)
      (by simp [hΓ]) (.data hv₁ hE) hnp' (by simpa using hy)
    refine ⟨⟨o₂.v, o₂.st, o₁.tr ++ o₂.tr⟩, ?_, by rw [hs₂, hs₁], hv₂, by rw [ht₁, ht₂, List.replicate_append_replicate]⟩
    simp only [eval, he₁, bnd_ok]
    rw [show o₁.v :: (ds ++ [Val.pend t id]) = (o₁.v :: ds) ++ [Val.pend t id] by simp, he₂]
    simp [pre]
  | @ite Γ pc cnd tb fb τ ιc ιt ιf βc βt βf _ _ _ ihc iht ihf =>
    intro hp Γd ds s m hΓ hE hnp hm
    simp only [pureH, Bool.and_eq_true] at hp
    obtain ⟨x, y, hx, hy, rfl⟩ := Count.add_eq hm
    obtain ⟨hyt, hyf⟩ := Count.same_eq hy
    obtain ⟨oc, hec, hsc, hvc, htc⟩ := ihc hp.1.1 hΓ hE hnp hx
    obtain ⟨b, hb⟩ := hvc.bool_inv
    cases b with
    | true =>
      obtain ⟨o', he', hs', hv', ht'⟩ := iht hp.1.2 (s := oc.st) hΓ hE hnp hyt
      refine ⟨⟨o'.v, o'.st, oc.tr ++ o'.tr⟩, ?_, by rw [hs', hsc], hv', by rw [htc, ht', List.replicate_append_replicate]⟩
      simp [eval, hec, hb, he', pre]
    | false =>
      obtain ⟨o', he', hs', hv', ht'⟩ := ihf hp.2 (s := oc.st) hΓ hE hnp hyf
      refine ⟨⟨o'.v, o'.st, oc.tr ++ o'.tr⟩, ?_, by rw [hs', hsc], hv', by rw [htc, ht', List.replicate_append_replicate]⟩
      simp [eval, hec, hb, he', pre]
  | @prim Γ pc op a b ιa ιb βa βb _ _ iha ihb =>
    intro hp Γd ds s m hΓ hE hnp hm
    simp only [pureH, Bool.and_eq_true] at hp
    obtain ⟨x, y, hx, hy, rfl⟩ := Count.add_eq hm
    obtain ⟨oa, hea, hsa, hva, hta⟩ := iha hp.1 hΓ hE hnp hx
    obtain ⟨ob, heb, hsb, hvb, htb⟩ := ihb hp.2 (s := oa.st) hΓ hE hnp hy
    obtain ⟨u, hu⟩ := hva.int_inv
    obtain ⟨w, hw⟩ := hvb.int_inv
    refine ⟨⟨op.apply u w, ob.st, oa.tr ++ ob.tr⟩, ?_, by rw [hsb, hsa], op.apply_ty u w,
      by rw [hta, htb, List.replicate_append_replicate]⟩
    simp [eval, hea, heb, hu, hw, pre]
  | @transfer Γ pc q _ hq =>
    intro _ Γd ds s m hΓ hE hnp hm
    subst hΓ
    have hlen : ds.length = Γd.length := hE.length
    have hqe : q = Γd.length := by
      rcases Nat.lt_or_ge q Γd.length with hlt | hge
      · rw [List.getElem?_append_left hlt] at hq; exact absurd hq (hnp q)
      · have : (Γd ++ [Bind.pend])[q]? = [Bind.pend][q - Γd.length]? :=
          List.getElem?_append_right hge
        rw [this] at hq
        cases hkd : q - Γd.length with
        | zero => omega
        | succ x => simp [hkd] at hq
    subst hqe
    simp [consumes, hlen] at hm; subst hm
    refine ⟨⟨.unit, s, [.handoff id]⟩, ?_, rfl, .unit, rfl⟩
    simp only [eval]
    rw [show (ds ++ [Val.pend t id])[Γd.length]? = some (Val.pend t id) by
      rw [← hlen]; simp]
  | act => intro hp; simp [pureH] at hp
  | drop => intro hp; simp [pureH] at hp
  | endorse => intro hp; simp [pureH] at hp
  | for_ => intro hp; simp [pureH] at hp
  | infer => intro hp; simp [pureH] at hp
  | await => intro hp; simp [pureH] at hp
  | atomic => intro hp; simp [pureH] at hp
  | call => intro hp; simp [pureH] at hp
  | mint => intro hp; simp [pureH] at hp

/-! ## O teorema -/

section
variable {S : Sig} {c : Cfg}

/-! ## Chaves frescas

A chave de `mint` é o contador `next`. Para que ela seja fresca, o contador precisa estar
acima de toda identidade do ambiente e do conjunto consumido, e as identidades vivas não
podem estar entre as aposentadas. `KF` é esse invariante; o avaliador o preserva por
construção (`Mono`), e cada passo de `sound` o restabelece. -/

structure KF (ρ : List Val) (s : St) : Prop where
  cap  : ∀ (k x : Nat), ρ[k]? = some (Val.cap x) → x < s.next ∧ x ∉ s.keys
  used : ∀ x ∈ s.used, x < s.next
  keys : ∀ x ∈ s.keys, x < s.next

/-- O contador só cresce, e toda chave aposentada no trecho foi cunhada no trecho. -/
structure Mono (s s' : St) : Prop where
  le   : s.next ≤ s'.next
  keys : ∀ x ∈ s'.keys, x ∈ s.keys ∨ (s.next ≤ x ∧ x < s'.next)

theorem Mono.refl (s : St) : Mono s s := ⟨Nat.le_refl _, fun _ hx => .inl hx⟩

theorem Mono.trans {s₁ s₂ s₃ : St} (h₁ : Mono s₁ s₂) (h₂ : Mono s₂ s₃) : Mono s₁ s₃ := by
  refine ⟨Nat.le_trans h₁.le h₂.le, fun x hx => ?_⟩
  rcases h₂.keys x hx with hx | ⟨a, b⟩
  · rcases h₁.keys x hx with hx | ⟨a', b'⟩
    · exact .inl hx
    · exact .inr ⟨a', Nat.lt_of_lt_of_le b' h₂.le⟩
  · exact .inr ⟨Nat.le_trans h₁.le a, b⟩

theorem mono_preserves : Preserves S c True (fun s s' _ => Mono s s') where
  refl s := Mono.refl s
  trans h₁ h₂ := h₁.trans h₂
  endorse _ := ⟨Nat.le_refl _, fun x hx => .inl hx⟩
  reject := ⟨Nat.le_refl _, fun x hx => .inl hx⟩
  act _ _ := ⟨Nat.le_refl _, fun x hx => .inl hx⟩
  drop _ := ⟨Nat.le_refl _, fun x hx => .inl hx⟩
  infer := ⟨Nat.le_refl _, fun x hx => .inl hx⟩
  commit _ := fun h => ⟨h.le, h.keys⟩
  revoke _ := fun _ _ _ h => ⟨h.le, h.keys⟩
  abort _ := fun _ _ _ h => ⟨h.le, h.keys⟩
  callDone := by intro _ s t id kid v _ _ _ _; exact ⟨Nat.le_refl _, fun x hx => .inl hx⟩
  callFailed := by intro _ s t id kid _ _ _ _; exact ⟨Nat.le_refl _, fun x hx => .inl hx⟩
  callRec := by intro _ s t id kid n v e _ _ _ _ _ _; exact ⟨Nat.le_refl _, fun x hx => .inl hx⟩
  callAbort := by intro _ s s₃ t id kid n v tr _ _ _ _ _ h _; exact ⟨h.le, h.keys⟩
  mint := by
    intro s s' t _ _ h
    refine ⟨Nat.le_trans (Nat.le_succ _) h.le, fun x hx => ?_⟩
    rcases List.mem_cons.mp hx with rfl | hx
    · exact .inr ⟨Nat.le_refl _, Nat.lt_of_lt_of_le (Nat.lt_succ_self _) h.le⟩
    · rcases h.keys x hx with hx | ⟨a, b⟩
      · exact .inl hx
      · exact .inr ⟨Nat.le_of_succ_le a, b⟩
  transfer := ⟨Nat.le_refl _, fun x hx => .inl hx⟩

theorem eval_mono_ok {e : Exp} {ρ : List Val} {s : St} {o : Out} (h : eval S c e ρ s = .ok o) :
    Mono s o.st := (eval_preserves mono_preserves e (.inl trivial)).1 h

theorem eval_mono_to {e : Exp} {ρ : List Val} {s : St} {q : Cause} {s₁ : St} {tr : List Ev}
    (h : eval S c e ρ s = .error (.timeout q s₁ tr)) : Mono s s₁ :=
  (eval_preserves mono_preserves e (.inl trivial)).2 h

theorem KF.step {ρ : List Val} {s s' : St} (hK : KF ρ s) (hm : Mono s s')
    (hu : ∀ x ∈ s'.used, x ∈ s.used ∨ ∃ k : Nat, ρ[k]? = some (Val.cap x)) : KF ρ s' := by
  refine ⟨fun k x hk => ⟨Nat.lt_of_lt_of_le (hK.cap k x hk).1 hm.le, fun hx => ?_⟩, fun x hx => ?_,
    fun x hx => ?_⟩
  · rcases hm.keys x hx with hx | ⟨a, _⟩
    · exact (hK.cap k x hk).2 hx
    · exact absurd (hK.cap k x hk).1 (Nat.not_lt.mpr a)
  · rcases hu x hx with hx | ⟨k, hk⟩
    · exact Nat.lt_of_lt_of_le (hK.used x hx) hm.le
    · exact Nat.lt_of_lt_of_le (hK.cap k x hk).1 hm.le
  · rcases hm.keys x hx with hx | ⟨_, b⟩
    · exact Nat.lt_of_lt_of_le (hK.keys x hx) hm.le
    · exact b

theorem KF.cons {ρ : List Val} {s : St} {v : Val} (hK : KF ρ s) (hv : ∀ id, v ≠ .cap id) :
    KF (v :: ρ) s := by
  refine ⟨fun k x hk => ?_, hK.used, hK.keys⟩
  cases k with
  | zero => simp at hk; exact absurd hk (hv x)
  | succ k => exact hK.cap k x (by simpa using hk)

/-- Dentro de `mint`: a chave nova é o contador, e o contador sobe. -/
theorem KF.mint {ρ : List Val} {s : St} (hK : KF ρ s) :
    KF (.cap s.next :: ρ) { s with next := s.next + 1 } := by
  refine ⟨fun k x hk => ?_, fun x hx => Nat.lt_succ_of_lt (hK.used x hx),
    fun x hx => Nat.lt_succ_of_lt (hK.keys x hx)⟩
  cases k with
  | zero =>
    simp at hk; subst hk
    exact ⟨Nat.lt_succ_self _, fun h => Nat.lt_irrefl _ (hK.keys _ h)⟩
  | succ k =>
    have := hK.cap k x (by simpa using hk)
    exact ⟨Nat.lt_succ_of_lt this.1, this.2⟩

theorem Consumed.sub {ρ : List Val} {f : Nat → Option Nat} {u u' : List Nat} (h : Consumed ρ f u u') :
    ∀ x ∈ u', x ∈ u ∨ ∃ k : Nat, ρ[k]? = some (Val.cap x) := by
  obtain ⟨new, rfl, hnew⟩ := h
  intro x hx
  rcases List.mem_append.mp hx with hx | hx
  · obtain ⟨k, hk, _⟩ := (hnew x).mp hx; exact .inr ⟨k, hk⟩
  · exact .inl hx

theorem Partial.sub {ρ : List Val} {f : Nat → Option Nat} {u u' : List Nat} (h : Partial ρ f u u') :
    ∀ x ∈ u', x ∈ u ∨ ∃ k : Nat, ρ[k]? = some (Val.cap x) := by
  obtain ⟨new, rfl, hnew⟩ := h
  intro x hx
  rcases List.mem_append.mp hx with hx | hx
  · obtain ⟨k, hk, _⟩ := hnew x hx; exact .inr ⟨k, hk⟩
  · exact .inl hx

theorem CapsDistinct.fresh {ρ : List Val} {n : Nat} (h : CapsDistinct ρ)
    (hn : ∀ k : Nat, ρ[k]? ≠ some (Val.cap n)) : CapsDistinct (.cap n :: ρ) := by
  intro i j id hi hj
  cases i with
  | zero =>
    cases j with
    | zero => rfl
    | succ j => simp at hi hj; subst hi; exact absurd hj (hn j)
  | succ i =>
    cases j with
    | zero => simp at hi hj; subst hj; exact absurd hi (hn i)
    | succ j => simp at hi hj; exact congrArg (· + 1) (h i j id hi hj)

theorem Granted.key {G : Nat → List EffectId} {Γ : List Bind} {ρ : List Val} {n : Nat}
    (h : Granted G Γ ρ) : Granted G (.key :: Γ) (.cap n :: ρ) := by
  intro k E id hk hid
  cases k with
  | zero => simp at hk
  | succ k => exact h k E id (by simpa using hk) (by simpa using hid)

theorem Lin.mint {ρ : List Val} {u : List Nat} {f : Nat → Option Nat} {n : Nat}
    (h : Lin ρ u fun k => f (k + 1)) (h0 : f 0 = some 1) (hn : n ∉ u) : Lin (.cap n :: ρ) u f := by
  intro k id hk
  cases k with
  | zero => simp at hk; subst hk; exact ⟨1, h0, Nat.le_refl _, fun _ => hn⟩
  | succ k => exact h k id (by simpa using hk)

/-- Na saída de `mint`, a chave sai do conjunto consumido: o consumo exato volta a falar só
das capacidades do ambiente de fora. -/
theorem Consumed.unmint {ρ : List Val} {f : Nat → Option Nat} {u u' : List Nat} {n : Nat}
    (h : Consumed (.cap n :: ρ) f u u') (hn : n ∉ u) (hρ : ∀ k : Nat, ρ[k]? ≠ some (Val.cap n)) :
    Consumed ρ (fun k => f (k + 1)) u (u'.filter (· ≠ n)) := by
  obtain ⟨new, rfl, hnew⟩ := h
  refine ⟨new.filter (· ≠ n), ?_, fun x => ⟨fun hx => ?_, fun ⟨k, hk, h1⟩ => ?_⟩⟩
  · rw [List.filter_append]
    congr 1
    exact List.filter_eq_self.mpr fun x hx => by simp only [ne_eq, decide_eq_true_eq]; rintro rfl; exact hn hx
  · obtain ⟨hx, hxn⟩ := List.mem_filter.mp hx
    obtain ⟨k, hk, h1⟩ := (hnew x).mp hx
    cases k with
    | zero => simp at hk; subst hk; simp at hxn
    | succ k => exact ⟨k, by simpa using hk, h1⟩
  · refine List.mem_filter.mpr ⟨(hnew x).mpr ⟨k + 1, by simpa using hk, h1⟩, ?_⟩
    simp only [ne_eq, decide_eq_true_eq]; rintro rfl; exact hρ k hk

theorem Partial.unmint {ρ : List Val} {f : Nat → Option Nat} {u u' : List Nat} {n : Nat}
    (h : Partial (.cap n :: ρ) f u u') (hn : n ∉ u) :
    Partial ρ (fun k => f (k + 1)) u (u'.filter (· ≠ n)) := by
  obtain ⟨new, rfl, hnew⟩ := h
  refine ⟨new.filter (· ≠ n), ?_, fun x hx => ?_⟩
  · rw [List.filter_append]
    congr 1
    exact List.filter_eq_self.mpr fun x hx => by simp only [ne_eq, decide_eq_true_eq]; rintro rfl; exact hn hx
  · obtain ⟨hx, hxn⟩ := List.mem_filter.mp hx
    obtain ⟨k, hk, h1⟩ := hnew x hx
    cases k with
    | zero => simp at hk; subst hk; simp at hxn
    | succ k => exact ⟨k, by simpa using hk, h1⟩

/-- A chamada consome a capacidade e a chave, duas posições distintas. -/
theorem Consumed.double {ρ : List Val} {j κ id kid : Nat} {u : List Nat}
    (hid : ρ[j]? = some (Val.cap id)) (hkid : ρ[κ]? = some (Val.cap kid)) (hjκ : j ≠ κ) :
    Consumed ρ (fun k => some ((if j = k then 1 else 0) + (if κ = k then 1 else 0))) u
      (kid :: id :: u) := by
  refine ⟨[kid, id], rfl, fun x => ⟨fun h => ?_, fun ⟨k, hk, h1⟩ => ?_⟩⟩
  · simp only [List.mem_cons, List.not_mem_nil, or_false] at h
    rcases h with rfl | rfl
    · exact ⟨κ, hkid, by simp [hjκ]⟩
    · exact ⟨j, hid, by simp [Ne.symm hjκ]⟩
  · by_cases hjk : j = k
    · subst hjk; rw [hid] at hk; cases hk; simp
    · by_cases hκk : κ = k
      · subst hκk; rw [hkid] at hk; cases hk; simp
      · simp [hjk, hκk] at h1

theorem sound (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} (h : HasType S Γ pc e τ ι β) :
    ∀ {ρ : List Val} {s : St}, EnvOk Γ ρ → CapsDistinct ρ → Granted c.grants Γ ρ →
      Lin ρ s.used (consumes e) → KF ρ s → s.spent + β ≤ c.limit →
      Sound S ρ (consumes e) τ β s (eval S c e ρ s) := by
  induction h with
  | val hv =>
    intro ρ s _ _ _ _ _ _
    exact .inl ⟨⟨_, s, []⟩, rfl, hv, Within.refl _ _, Consumed.nil fun _ _ _ => by simp [consumes]⟩
  | var hk =>
    intro ρ s hE _ _ _ _ _
    obtain ⟨v, hρ, hv⟩ := hE.data_lookup hk
    exact .inl ⟨⟨v, s, []⟩, by simp [eval, hρ], hv, Within.refl _ _,
      Consumed.nil fun _ _ _ => by simp [consumes]⟩
  | @let_ Γ pc e₁ e₂ τ₁ τ₂ ι₁ ι₂ β₁ β₂ _ _ ih₁ ih₂ =>
    intro ρ s hE hD hG hL hK hB
    have hs := splits_let e₁ e₂
    rcases ih₁ hE hD hG (hL.left hs) hK (fun d => by have := hB d; simp at this ⊢; omega) with
      ⟨o₁, he₁, hv₁, hw₁, hc₁⟩ | ⟨q, s₁, tr, he₁, hw₁, hp₁, hq₁⟩
    · have hvc := hv₁.not_cap
      rcases ih₂ (.data hv₁ hE) (hD.cons hvc) hG.cons (Lin.cons hvc (hL.seq hs hD hc₁))
          ((hK.step (eval_mono_ok he₁) hc₁.sub).cons hvc)
          (fun d => by have := hB d; have := (hw₁ d).2; simp at *; omega) with
        ⟨o₂, he₂, hv₂, hw₂, hc₂⟩ | ⟨q, s₂, tr, he₂, hw₂, hp₂, hq₂⟩
      · exact .inl ⟨⟨o₂.v, o₂.st, o₁.tr ++ o₂.tr⟩, by simp [eval, he₁, he₂], hv₂, hw₁.trans hw₂,
          Consumed.seq hs hL hc₁ (hc₂.uncons hvc)⟩
      · exact .inr ⟨q, s₂, o₁.tr ++ tr, by simp only [eval, he₁, he₂, bnd_ok, bnd_error] <;> rfl,
          hw₁.trans hw₂, Partial.seq hs hL hc₁.toPartial (hp₂.uncons hvc), hq₂⟩
    · exact .inr ⟨q, s₁, tr, by simp only [eval, he₁, bnd_error], hw₁.mono fun d => by simp,
        hp₁.left hs hL, hq₁⟩
  | @ite Γ pc cnd t f τ ιc ιt ιf βc βt βf _ _ _ ihc iht ihf =>
    intro ρ s hE hD hG hL hK hB
    have hst := splits_ite_t cnd t f
    have hsf := splits_ite_f cnd t f
    rcases ihc hE hD hG (hL.left hst) hK (fun d => by have := hB d; simp at this ⊢; omega) with
      ⟨oc, hec, hvc, hwc, hcc⟩ | ⟨q, s₁, tr, hec, hwc, hpc, hqc⟩
    · obtain ⟨b, hb⟩ := hvc.bool_inv
      cases b with
      | true =>
        rcases iht hE hD hG (hL.seq hst hD hcc) (hK.step (eval_mono_ok hec) hcc.sub)
            (fun d => by have := hB d; have := (hwc d).2; simp at *; omega) with
          ⟨o', he', hv', hw', hc'⟩ | ⟨q, s₁, tr, he', hw', hp', hq'⟩
        · refine .inl ⟨⟨o'.v, o'.st, oc.tr ++ o'.tr⟩, by simp [eval, hec, hb, he'], hv', ?_,
            Consumed.seq hst hL hcc hc'⟩
          exact (hwc.trans hw').mono fun d => by simp; omega
        · refine .inr ⟨q, s₁, oc.tr ++ tr, by simp [eval, hec, hb, he', pre] <;> rfl, ?_,
            Partial.seq hst hL hcc.toPartial hp', hq'⟩
          exact (hwc.trans hw').mono fun d => by simp; omega
      | false =>
        rcases ihf hE hD hG (hL.seq hsf hD hcc) (hK.step (eval_mono_ok hec) hcc.sub)
            (fun d => by have := hB d; have := (hwc d).2; simp at *; omega) with
          ⟨o', he', hv', hw', hc'⟩ | ⟨q, s₁, tr, he', hw', hp', hq'⟩
        · refine .inl ⟨⟨o'.v, o'.st, oc.tr ++ o'.tr⟩, by simp [eval, hec, hb, he'], hv', ?_,
            Consumed.seq hsf hL hcc hc'⟩
          exact (hwc.trans hw').mono fun d => by simp; omega
        · refine .inr ⟨q, s₁, oc.tr ++ tr, by simp [eval, hec, hb, he', pre] <;> rfl, ?_,
            Partial.seq hsf hL hcc.toPartial hp', hq'⟩
          exact (hwc.trans hw').mono fun d => by simp; omega
    · exact .inr ⟨q, s₁, tr, by simp only [eval, hec, bnd_error], hwc.mono fun d => by simp,
        hpc.left hst hL, hqc⟩
  | @prim Γ pc op a b ιa ιb βa βb _ _ iha ihb =>
    intro ρ s hE hD hG hL hK hB
    have hs := splits_prim op a b
    rcases iha hE hD hG (hL.left hs) hK (fun d => by have := hB d; simp at this ⊢; omega) with
      ⟨oa, hea, hva, hwa, hca⟩ | ⟨q, s₁, tr, hea, hwa, hpa, hqa⟩
    · rcases ihb hE hD hG (hL.seq hs hD hca) (hK.step (eval_mono_ok hea) hca.sub)
          (fun d => by have := hB d; have := (hwa d).2; simp at *; omega) with
        ⟨ob, heb, hvb, hwb, hcb⟩ | ⟨q, s₁, tr, heb, hwb, hpb, hqb⟩
      · obtain ⟨x, hx⟩ := hva.int_inv
        obtain ⟨y, hy⟩ := hvb.int_inv
        exact .inl ⟨⟨op.apply x y, ob.st, oa.tr ++ ob.tr⟩, by simp [eval, hea, heb, hx, hy],
          op.apply_ty x y, hwa.trans hwb, Consumed.seq hs hL hca hcb⟩
      · exact .inr ⟨q, s₁, oa.tr ++ tr, by simp only [eval, hea, heb, bnd_ok, bnd_error] <;> rfl,
          hwa.trans hwb, Partial.seq hs hL hca.toPartial hpb, hqb⟩
    · exact .inr ⟨q, s₁, tr, by simp only [eval, hea, bnd_error], hwa.mono fun d => by simp,
        hpa.left hs hL, hqa⟩
  | @endorse Γ pc p e d τ ιe ιd βe βd _ _ _ ihe ihd =>
    intro ρ s hE hD hG hL hK hB
    have hsd := splits_endorse_d p e d
    have hse := splits_endorse_e p e d
    rcases ihe hE hD hG (hL.left hsd) hK (fun d => by have := hB d; simp at this ⊢; omega) with
      ⟨oe, hee, hve, hwe, hce⟩ | ⟨q, s₁, tr, hee, hwe, hpe, hqe⟩
    · by_cases hp : S.pol p oe.v = true
      · exact .inl ⟨⟨oe.v, oe.st, oe.tr ++ [.endorse p (some oe.v)]⟩, by simp [eval, hee, hp], hve,
          hwe.mono fun d => by simp,
          Consumed.seq hse hL hce (Consumed.nil fun _ _ _ => by simp)⟩
      · rcases ihd hE hD hG (hL.seq hsd hD hce) (hK.step (eval_mono_ok hee) hce.sub)
            (fun d => by have := hB d; have := (hwe d).2; simp at *; omega) with
          ⟨od, hed, hvd, hwd, hcd⟩ | ⟨q, s₁, tr, hed, hwd, hpd, hqd⟩
        · exact .inl ⟨⟨od.v, od.st, oe.tr ++ .endorse p none :: od.tr⟩, by simp [eval, hee, hp, hed],
            hvd, hwe.trans hwd, Consumed.seq hsd hL hce hcd⟩
        · exact .inr ⟨q, s₁, oe.tr ++ ([.endorse p none] ++ tr),
            by simp [eval, hee, hp, hed, pre] <;> rfl, hwe.trans hwd,
            Partial.seq hsd hL hce.toPartial hpd, hqd⟩
    · exact .inr ⟨q, s₁, tr, by simp only [eval, hee, bnd_error], hwe.mono fun d => by simp,
        hpe.left hsd hL, hqe⟩
  | @act Γ pc t j a ιa βa E _ hk heff _ _ iha =>
    intro ρ s hE hD hG hL hK hB
    have hs := splits_act t j a
    rcases iha hE hD hG (hL.left hs) hK (fun d => by have := hB d; simp at this ⊢; omega) with
      ⟨oa, hea, _, hwa, hca⟩ | ⟨q, s₁, tr, hea, hwa, hpa, hqa⟩
    · obtain ⟨id, hid⟩ := hE.cap_lookup hk
      have hgrant : S.effect t ∈ c.grants id := hG j E id hk hid _ heff
      have hfree : id ∉ oa.st.used := by
        obtain ⟨m, hm, _, hm1⟩ := hL.seq hs hD hca j id hid
        simp at hm; subst hm; exact hm1 rfl
      have hble : ble (oa.st.spent + S.cost t) c.limit = true :=
        ble_iff.mpr fun d => by have := hB d; have := (hwa d).2; simp at *; omega
      exact .inl ⟨⟨.unit, ⟨id :: oa.st.used, oa.st.spent + S.cost t, oa.st.calls,
        actW S t id oa.v oa.st.w, actC S t id oa.v oa.st.comp, oa.st.next, oa.st.keys⟩,
        oa.tr ++ actEvs S t id oa.v⟩,
        by simp [eval, hea, doAct, hid, hgrant, hfree, hble], .unit, hwa.trans (Within.step _ _),
        Consumed.seq hs hL hca (Consumed.single hid)⟩
    · exact .inr ⟨q, s₁, tr, by simp only [eval, hea, bnd_error], hwa.mono fun d => by simp,
        hpa.left hs hL, hqa⟩
  | @drop Γ pc j E hk =>
    intro ρ s hE _ _ hL _ _
    obtain ⟨id, hid⟩ := hE.cap_lookup hk
    have hfree : id ∉ s.used := by
      obtain ⟨m, hm, _, hm1⟩ := hL j id hid
      simp [consumes] at hm; subst hm; exact hm1 rfl
    exact .inl ⟨⟨.unit, ⟨id :: s.used, s.spent, s.calls, s.w, s.comp, s.next, s.keys⟩, []⟩,
      by simp [eval, doDrop, hid, hfree], .unit, Within.refl _ _, Consumed.single hid⟩
  | @for_ Γ pc n xs body τ τb ιx ιb βx βb _ _ ihx ihb =>
    intro ρ s hE hD hG hL hK hB
    have hs := splits_for n xs body
    rcases ihx hE hD hG (hL.left hs) hK
        (fun d => by have := hB d; simp only [add_apply] at this ⊢; omega) with
      ⟨ox, hex, hvx, hwx, hcx⟩ | ⟨q, s₁, tr, hex, hwx, hpx, hqx⟩
    · obtain ⟨vs, hvs, hall, hlen⟩ := hvx.list_inv
      have hzero := for_body_zero hL
      have hf : ∀ v (s' : St), KF ρ s' → HasTy v τ → s'.spent + βb ≤ c.limit →
          (∃ o, eval S c body (v :: ρ) s' = .ok o ∧ Within s'.spent o.st.spent βb ∧
            o.st.used = s'.used ∧ KF ρ o.st) ∨
          (∃ q s₁ tr, eval S c body (v :: ρ) s' = .error (.timeout q s₁ tr) ∧
            Within s'.spent s₁.spent βb ∧ s₁.used = s'.used ∧ CauseOk S q) := by
        intro v s' hK' hv hb
        have hvc := hv.not_cap
        have hLb : Lin (v :: ρ) s'.used (consumes body) :=
          Lin.cons hvc fun k id hk => ⟨0, hzero k id hk, by omega, by omega⟩
        rcases ihb (.data hv hE) (hD.cons hvc) hG.cons hLb (hK'.cons hvc) hb with
          ⟨o, he, _, hw, hcb⟩ | ⟨q, s₁, tr, he, hw, hpb, hqb⟩
        · have hu := (hcb.uncons hvc).unchanged fun k id hk => by simp [hzero k id hk]
          exact .inl ⟨o, he, hw, hu, hK'.step (eval_mono_ok he) fun x hx => .inl (hu ▸ hx)⟩
        · exact .inr ⟨q, s₁, tr, he, hw,
            (hpb.uncons hvc).unchanged fun k id hk => by simp [hzero k id hk], hqb⟩
      rcases loop_sound (P := KF ρ) hf n vs ox.st (hK.step (eval_mono_ok hex) hcx.sub) hlen hall
          (fun d => by have := hB d; have := (hwx d).2; simp only [add_apply, scale_apply] at *; omega) with
        ⟨s', tr, hl, hwl, hul⟩ | ⟨q, s₁, tr, hl, hwl, hul, hql⟩
      · refine .inl ⟨⟨.unit, s', ox.tr ++ tr⟩, by simp [eval, hex, hvs, hl], .unit, hwx.trans hwl, ?_⟩
        exact Consumed.seq hs hL hcx (by rw [hul]; exact Consumed.nil fun _ _ _ => by simp)
      · refine .inr ⟨q, s₁, ox.tr ++ tr, by simp [eval, hex, hvs, hl, pre] <;> rfl, hwx.trans hwl, ?_, hql⟩
        exact Partial.seq hs hL hcx.toPartial (by rw [hul]; exact Partial.refl _ _ _)
    · exact .inr ⟨q, s₁, tr, by simp only [eval, hex, bnd_error], hwx.mono fun d => by simp,
        hpx.left hs hL, hqx⟩
  | @infer Γ pc m t a τa ιa βa _ iha =>
    intro ρ s hE hD hG hL hK hB
    rcases iha hE hD hG hL hK (fun d => by have := hB d; simp at this ⊢; omega) with
      ⟨oa, hea, _, hwa, hca⟩ | ⟨q, s₁, tr, hea, hwa, hpa, hqa⟩
    · have hble : ble (oa.st.spent + S.mcost m) c.limit = true :=
        ble_iff.mpr fun d => by have := hB d; have := (hwa d).2; simp at *; omega
      exact .inl ⟨⟨c.oracle oa.st.calls m t oa.v, ⟨oa.st.used, oa.st.spent + S.mcost m,
        oa.st.calls + 1, oa.st.w, oa.st.comp, oa.st.next, oa.st.keys⟩, oa.tr⟩,
        by simp [eval, hea, doInfer, hble], hO _ _ _ _, hwa.trans (Within.step _ _), hca⟩
    · exact .inr ⟨q, s₁, tr, by simp only [eval, hea, bnd_error], hwa.mono fun d => by simp, hpa, hqa⟩
  | @await Γ pc q _ =>
    intro ρ s _ _ _ _ _ _
    cases hq : S.probe q s.w
    · exact .inr ⟨.probe q, s, [], by simp [eval, hq], Within.refl _ _, Partial.refl _ _ _, trivial⟩
    · exact .inl ⟨⟨.unit, s, []⟩, by simp [eval, hq], .unit, Within.refl _ _,
        Consumed.nil fun _ _ _ => by simp [consumes]⟩
  | @atomic Γ pc body r f g τ ιb ιf ιg βb βf βg _ _ _ _ ihb ihf ihg =>
    intro ρ s hE hD hG hL hK hB
    have hsf := splits_atomic_f body r f g
    have hsg := splits_atomic_g body r f g
    rcases ihb (s := { s with comp := [] }) hE hD hG (hL.left hsf) ⟨hK.cap, hK.used, hK.keys⟩
        (fun d => by have := hB d; simp at this ⊢; omega) with
      ⟨o, he, hv, hw, hc⟩ | ⟨q, s₁, tr, he, hw, hp, hq⟩
    · -- commit: o corpo consumiu sua parte, a saída revoga a dos tratadores
      exact .inl ⟨_, eval_atomic_ok he, hv, hw.mono fun d => by simp,
        (hc.toPartial.left hsf hL).exit hL⟩
    · by_cases hy : (q.isAbort && ((compensate S s₁.comp s₁.w).ok && S.probe r (compensate S s₁.comp s₁.w).w)) = true
      · -- o encerramento pelo tratador atravessa a saga recuperada
        rw [eval_atomic_reraise he hy]
        exact .inr ⟨_, _, _, rfl, hw.mono fun d => by simp, hp.left hsf hL, hq⟩
      -- falha: compensa, roda um tratador; a saída revoga o que o caminho não consumiu
      rw [eval_atomic_fail he (by simpa using hy)]
      have fin : ∀ (evs : List Ev) (hd : Exp) (βh : Budget) (r' : Except Err Out),
          Splits (consumes (.atomic body r f g)) (consumes body) (consumes hd) → βh ≤ βf ⊔ βg →
          Sound S ρ (consumes hd) τ βh { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp } r' →
          Sound S ρ (consumes (.atomic body r f g)) τ (βb + (βf ⊔ βg)) s
            (pre evs (bnd r' fun o' => .ok ⟨o'.v, exitSaga ρ (.atomic body r f g) o'.st, evs ++ o'.tr⟩)) := by
        intro evs hd βh r' hs hβ hr
        rcases hr with ⟨o', rfl, hv', hw', hc'⟩ | ⟨q', s₃, tr', rfl, hw', hp', hq'⟩
        · refine .inl ⟨⟨o'.v, exitSaga ρ (.atomic body r f g) o'.st, evs ++ o'.tr⟩, rfl, hv', ?_,
            (Partial.seq hs hL hp hc'.toPartial).exit hL⟩
          exact (hw.trans hw').mono fun d => by have := hβ d; simp at *; omega
        · refine .inr ⟨q', s₃, evs ++ tr', rfl, ?_, Partial.seq hs hL hp hp', hq'⟩
          exact (hw.trans hw').mono fun d => by have := hβ d; simp at *; omega
      cases (compensate S s₁.comp s₁.w).ok && S.probe r (compensate S s₁.comp s₁.w).w
      · exact fin _ g βg _ hsg (le_join_right _ _)
          (ihg (s := { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp }) hE hD hG
            (Lin.seqP hsg hD hL hp) (hK.step ⟨(eval_mono_to he).le, (eval_mono_to he).keys⟩ hp.sub)
            (fun d => by have := hB d; have := (hw d).2; simp at *; omega))
      · exact fin _ f βf _ hsf (le_join_left _ _)
          (ihf (s := { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp }) hE hD hG
            (Lin.seqP hsf hD hL hp) (hK.step ⟨(eval_mono_to he).le, (eval_mono_to he).keys⟩ hp.sub)
            (fun d => by have := hB d; have := (hw d).2; simp at *; omega))
  | @call Γ pc t j κ a n e ιa βa E ιe βe _ hk heff hκ _ _ hte hpure hcons iha _ =>
    intro ρ s hE hD hG hL hK hB
    have hs : Splits (consumes (.call t j κ a n e)) (consumes a)
        (fun k => some ((if j = k then 1 else 0) + (if κ = k then 1 else 0))) :=
      fun _ _ h => Count.add_eq h
    rcases iha hE hD hG (hL.left hs) hK (fun d => by have := hB d; simp at this ⊢; omega) with
      ⟨oa, hea, _, hwa, hca⟩ | ⟨q, s₁, tr, hea, hwa, hpa, hqa⟩
    · obtain ⟨id, hid⟩ := hE.cap_lookup hk
      obtain ⟨kid, hkid⟩ := hE.key_lookup hκ
      have hjκ : j ≠ κ := by intro h; subst h; rw [hk] at hκ; cases hκ
      have hidk : kid ≠ id := by intro h; subst h; exact hjκ (hD _ _ _ hid hkid)
      have hgrant : S.effect t ∈ c.grants id := hG j E id hk hid _ heff
      have hfree : id ∉ oa.st.used := by
        obtain ⟨m, hm, _, hm1⟩ := hL.seq hs hD hca j id hid
        simp [Ne.symm hjκ] at hm; subst hm; exact hm1 rfl
      have hkfree : kid ∉ oa.st.used := by
        obtain ⟨m, hm, _, hm1⟩ := hL.seq hs hD hca κ kid hkid
        simp [hjκ] at hm; subst hm; exact hm1 rfl
      have hkk : kid ∉ oa.st.keys := ((hK.step (eval_mono_ok hea) hca.sub).cap κ kid hkid).2
      have hble : ble (oa.st.spent + S.cost t) c.limit = true :=
        ble_iff.mpr fun d => by have := hB d; have := (hwa d).2; simp at *; omega
      have hcon : Consumed ρ (consumes (.call t j κ a n e)) s.used (kid :: id :: oa.st.used) :=
        Consumed.seq hs hL hca (Consumed.double hid hkid hjκ)
      have hwc : Within s.spent (oa.st.spent + S.cost t) (βa + S.cost t) := hwa.trans (Within.step _ _)
      have hev : eval S c (.call t j κ a n e) ρ s = bnd (eval S c a ρ s) (fun o =>
          doCall S c t ρ[j]? ρ[κ]? n (fun id s' => eval S c e [.pend t id] s') o) := rfl
      rw [hev, hea, bnd_ok]
      unfold doCall
      simp only [hid, hkid, hgrant, hfree, hble, if_true, if_false, List.mem_cons, hidk, hkfree,
        hkk, or_self, or_false, false_or]
      cases hr0 : S.reply t kid 0 with
      | done =>
        exact .inl ⟨_, rfl, .unit, hwc, hcon⟩
      | failed =>
        exact .inr ⟨.refused kid, callSt0 S t id kid oa.st, oa.tr ++ [.settle kid .failed],
          by simp [pre] <;> rfl, hwc, hcon.toPartial, trivial⟩
      | unknown =>
        simp only
        cases hr : (reconcile S t kid 1 n).2 with
        | done => exact .inl ⟨_, rfl, .unit, hwc, hcon⟩
        | failed =>
          exact .inr ⟨.refused kid, callSt S t id kid oa.v oa.st (S.applied t kid),
            oa.tr ++ (.pending t kid oa.v :: ((reconcile S t kid 1 n).1 ++ [.settle kid .failed])),
            by simp [pre] <;> rfl, hwc, hcon.toPartial, trivial⟩
        | transferred =>
          simp only
          obtain ⟨oh, heh, hsh, hvh, hth⟩ := pure_handler (c := c) (t := t) (id := kid) hte hpure
            (Γd := []) (ds := []) (s := callSt S t id kid oa.v oa.st (S.applied t kid)) (m := 1)
            (by simp) .nil (by simp) (by simpa using hcons)
          have heh' : eval S c e [.pend t kid] (callSt S t id kid oa.v oa.st (S.applied t kid)) = .ok oh := by
            simpa using heh
          rw [heh']
          have hto : transferOnly kid oh.tr = true := by
            simp [transferOnly, hth, kevs, keyOf, handoffs]
          simp only [hto, if_true]
          refine .inr ⟨.abort oh.v, oh.st,
            oa.tr ++ (.pending t kid oa.v :: ((reconcile S t kid 1 n).1 ++ (oh.tr ++ [.settle kid .transferred]))),
            by simp [pre] <;> rfl, ?_, ?_, hvh⟩
          · rw [hsh]; exact hwc
          · rw [hsh]; exact hcon.toPartial
    · exact .inr ⟨q, s₁, tr, by simp only [eval, hea, bnd_error], hwa.mono fun d => by simp,
        hpa.left hs hL, hqa⟩
  | @mint Γ pc e τ ι β _ _ hcons ih =>
    intro ρ s hE hD hG hL hK hB
    have hnu : s.next ∉ s.used := fun h => Nat.lt_irrefl _ (hK.used _ h)
    have hnk : s.next ∉ s.keys := fun h => Nat.lt_irrefl _ (hK.keys _ h)
    have hnρ : ∀ k : Nat, ρ[k]? ≠ some (Val.cap s.next) := fun k h => Nat.lt_irrefl _ (hK.cap k _ h).1
    have hev : eval S c (.mint e) ρ s =
        mintOut s.next (eval S c e (.cap s.next :: ρ) { s with next := s.next + 1 }) := by
      simp only [eval]; rw [if_neg (not_or.mpr ⟨hnu, hnk⟩)]
    rw [hev]
    rcases ih (ρ := .cap s.next :: ρ) (s := { s with next := s.next + 1 }) (.key hE)
        (hD.fresh hnρ) hG.key (Lin.mint hL hcons hnu) hK.mint hB with
      ⟨o, heo, hv, hw, hc⟩ | ⟨q, s₁, tr, heo, hw, hp, hq⟩
    · rw [heo]
      exact .inl ⟨_, rfl, hv, hw, hc.unmint hnu hnρ⟩
    · rw [heo]
      exact .inr ⟨q, _, _, rfl, hw, hp.unmint hnu, hq⟩
  | @transfer Γ pc p _ hp =>
    intro ρ s hE _ _ _ _ _
    obtain ⟨t, id, hid⟩ := hE.pend_lookup hp
    refine .inl ⟨⟨.unit, s, [.handoff id]⟩, by simp [eval, hid], .unit, Within.refl _ _,
      Consumed.nil fun k id' hk => ?_⟩
    by_cases hpk : p = k
    · subst hpk; rw [hid] at hk; cases hk
    · simp [consumes, hpk]

/-! ## Enunciados -/

/-- O ponto de partida de uma execução do agente: o ambiente realiza Γ; as capacidades têm
identidades distintas, nenhuma foi consumida e o runtime concede a cada uma pelo menos os
efeitos do seu tipo `Cap[E]`. -/
structure Start (G : Nat → List EffectId) (Γ : List Bind) (ρ : List Val) (s : St) : Prop where
  env     : EnvOk Γ ρ
  caps    : CapsDistinct ρ
  granted : Granted G Γ ρ
  fresh   : ∀ k id : Nat, ρ[k]? = some (Val.cap id) → id ∉ s.used
  /-- o agente começa sem chaves: elas nascem de `mint` -/
  nokey   : ∀ k : Nat, Γ[k]? ≠ some Bind.key
  /-- o contador de cunhagem está acima de toda identidade em uso -/
  kf      : KF ρ s

theorem linear_lin {Γ : List Bind} {ρ : List Val} {s : St} {e : Exp} (hE : EnvOk Γ ρ)
    (hfresh : ∀ k id : Nat, ρ[k]? = some (Val.cap id) → id ∉ s.used)
    (hnk : ∀ k : Nat, Γ[k]? ≠ some Bind.key) (h : Linear Γ e) :
    Lin ρ s.used (consumes e) := by
  intro k id hk
  rcases hE.cap_pos hk with ⟨E, hE'⟩ | hκ
  · exact ⟨1, h k E hE', Nat.le_refl _, fun _ => hfresh k id hk⟩
  · exact absurd hκ (hnk k)

/-- **T1 nos dois desfechos.** Vale para todo programa bem tipado e linear, com ou sem
sagas e com ou sem chamadas com chave. Ou ele termina com um valor do tipo estático, dentro
de β e tendo consumido exatamente as capacidades do agente; ou ele falha por uma causa que
uma saga captura, com o gasto dentro de β e tendo consumido só capacidades do agente, cada
uma no máximo uma vez. Se a causa for o encerramento pelo tratador de `TimeoutUnknown`, o
valor tem o tipo de retorno do agente. Em particular, os dois ramos de uma saga (confirmação
e compensação) respeitam a linearidade de `Cap[E]`: o que o caminho percorrido não consumiu é
revogado na saída. -/
theorem soundness_total (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty}
    {ι : Lab} {β : Budget} {ρ : List Val} {s : St} (h : HasType S Γ pc e τ ι β)
    (hlin : Linear Γ e) (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    (∃ o, eval S c e ρ s = .ok o ∧ HasTy o.v τ ∧ Within s.spent o.st.spent β ∧
      ∃ new : List Nat, o.st.used = new ++ s.used ∧
        ∀ id : Nat, id ∈ new ↔ ∃ k : Nat, ρ[k]? = some (Val.cap id)) ∨
    (∃ q s₁ tr, eval S c e ρ s = .error (.timeout q s₁ tr) ∧ Within s.spent s₁.spent β ∧
      (∃ new : List Nat, s₁.used = new ++ s.used ∧
        ∀ id ∈ new, ∃ k : Nat, ρ[k]? = some (Val.cap id)) ∧ CauseOk S q) := by
  rcases sound hO h h0.env h0.caps h0.granted (linear_lin h0.env h0.fresh h0.nokey hlin) h0.kf hB with
    ⟨o, he, hv, hw, new, hnew, hmem⟩ | ⟨q, s₁, tr, he, hw, ⟨new, hnew, hmem⟩, hq⟩
  · refine .inl ⟨o, he, hv, hw, new, hnew, fun id => ⟨fun hin => ?_, fun ⟨k, hk⟩ => ?_⟩⟩
    · obtain ⟨k, hk, _⟩ := (hmem id).mp hin
      exact ⟨k, hk⟩
    · rcases h0.env.cap_pos hk with ⟨E, hE'⟩ | hκ
      · exact (hmem id).mpr ⟨k, hk, hlin k E hE'⟩
      · exact absurd hκ (h0.nokey k)
  · refine .inr ⟨q, s₁, tr, he, hw, ⟨new, hnew, fun id hin => ?_⟩, hq⟩
    obtain ⟨k, hk, _⟩ := hmem id hin
    exact ⟨k, hk⟩

/-- **Teorema T1 (solidez).** Um programa bem tipado e linear, com todo `await` dentro de
uma saga (`Guarded`) e cujo orçamento estático cabe no teto, executa a partir de um ponto de
partida válido e chega a um de três desfechos, nenhum deles um erro de programa:

* termina com um valor do tipo estático, dentro de β em cada dimensão, e o conjunto
  consumido ganha exatamente as capacidades do agente;
* o provedor recusou uma operação com chave (`Failed`, na chamada ou na reconciliação), fora
  de qualquer saga que a capturasse;
* o tratador de `TimeoutUnknown` transferiu a obrigação e encerrou o evento com um valor do
  tipo de retorno do agente.

Uma pós-condição nunca falha fora de uma saga. Sem chamadas com chave, só o primeiro
desfecho existe (`soundness_keyless`). -/
theorem soundness (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} {ρ : List Val} {s : St} (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (hg : Guarded e) (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    (∃ o, eval S c e ρ s = .ok o ∧ HasTy o.v τ ∧ Within s.spent o.st.spent β ∧
      ∃ new : List Nat, o.st.used = new ++ s.used ∧
        ∀ id : Nat, id ∈ new ↔ ∃ k : Nat, ρ[k]? = some (Val.cap id)) ∨
    (∃ k s₁ tr, eval S c e ρ s = .error (.timeout (.refused k) s₁ tr)) ∨
    (∃ v s₁ tr, eval S c e ρ s = .error (.timeout (.abort v) s₁ tr) ∧ HasTy v S.ret) := by
  rcases soundness_total hO h hlin h0 hB with hok | ⟨q, s₁, tr, he, _, _, hq⟩
  · exact .inl hok
  · cases q with
    | probe q => exact absurd he (guarded_no_timeout e hg ρ s q s₁ tr)
    | refused k => exact .inr (.inl ⟨k, s₁, tr, he⟩)
    | abort v => exact .inr (.inr ⟨v, s₁, tr, he, hq⟩)

/-- **T1, progresso.** Nenhum dos quatro erros de programa acontece: `stuck`, efeito sem
capacidade ou sem concessão (`noCap`), reuso ou descarte de recurso linear (`reuse`) e estouro
de orçamento (`budget`). Toda falha que sobe é uma recusa do provedor ou o encerramento pelo
tratador; nenhuma é uma pós-condição. -/
theorem progress (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} {ρ : List Val} {s : St} (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (hg : Guarded e) (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    ∀ err, eval S c e ρ s = .error err →
      ∃ q s₁ tr, err = .timeout q s₁ tr ∧ ((∃ k, q = .refused k) ∨ ∃ v, q = .abort v) := by
  intro err herr
  rcases soundness hO h hlin hg h0 hB with ⟨o, ho, _⟩ | ⟨k, s₁, tr, ho⟩ | ⟨v, s₁, tr, ho, _⟩
  · rw [ho] at herr; cases herr
  · rw [ho] at herr; cases herr; exact ⟨_, _, _, rfl, .inl ⟨k, rfl⟩⟩
  · rw [ho] at herr; cases herr; exact ⟨_, _, _, rfl, .inr ⟨v, rfl⟩⟩

theorem loop_probe_only {f : Val → St → Except Err Out}
    (hf : ∀ v s q s₁ tr, f v s = .error (.timeout q s₁ tr) → ∃ q', q = .probe q') :
    ∀ (vs : List Val) (s : St) (q : Cause) (s₁ : St) (tr : List Ev),
      loop f vs s = .error (.timeout q s₁ tr) → ∃ q', q = .probe q'
  | [], s, q, s₁, tr, h => by simp [loop] at h
  | v :: vs, s, q, s₁, tr, h => by
    simp only [loop] at h
    rcases bnd_eq_timeout h with h₀ | ⟨o, t, _, h₁, _⟩
    · exact hf v s q s₁ tr h₀
    · rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
      · exact loop_probe_only hf vs _ q s₁ _ h₂
      · cases h₃

/-- Sem chamadas com chave, a única causa de falha é uma pós-condição. -/
theorem keyless_probe_only : ∀ (e : Exp), keyless e = true →
    ∀ (ρ : List Val) (s : St) (q : Cause) (s₁ : St) (tr : List Ev),
      eval S c e ρ s = .error (.timeout q s₁ tr) → ∃ q', q = .probe q'
  | .val _, _, ρ, s, q, s₁, tr, h => by simp [eval] at h
  | .var k, _, ρ, s, q, s₁, tr, h => by
    simp only [eval] at h; split at h <;> cases h
  | .let_ e₁ e₂, hg, ρ, s, q, s₁, tr, h => by
    simp only [keyless, Bool.and_eq_true] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨o₁, t, _, h₁, _⟩
    · exact keyless_probe_only e₁ hg.1 _ _ _ _ _ h₀
    · rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
      · exact keyless_probe_only e₂ hg.2 _ _ _ _ _ h₂
      · cases h₃
  | .ite x t f, hg, ρ, s, q, s₁, tr, h => by
    simp only [keyless, Bool.and_eq_true] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨o, tr', _, h₁, _⟩
    · exact keyless_probe_only x hg.1.1 _ _ _ _ _ h₀
    · revert h₁; split
      · intro h₁
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · rcases cond_eq h₂ with ⟨_, h₄⟩ | ⟨_, h₄⟩
          · exact keyless_probe_only t hg.1.2 _ _ _ _ _ h₄
          · exact keyless_probe_only f hg.2 _ _ _ _ _ h₄
        · cases h₃
      · intro h₁; cases h₁
  | .prim op a b, hg, ρ, s, q, s₁, tr, h => by
    simp only [keyless, Bool.and_eq_true] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨o₁, t, _, h₁, _⟩
    · exact keyless_probe_only a hg.1 _ _ _ _ _ h₀
    · rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
      · exact keyless_probe_only b hg.2 _ _ _ _ _ h₂
      · revert h₃; split <;> intro h₃ <;> cases h₃
  | .endorse p e d, hg, ρ, s, q, s₁, tr, h => by
    simp only [keyless, Bool.and_eq_true] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨oe, t, _, h₁, _⟩
    · exact keyless_probe_only e hg.1 _ _ _ _ _ h₀
    · revert h₁; split
      · intro h₁; cases h₁
      · intro h₁
        obtain ⟨t', h₂, _⟩ := pre_eq_timeout h₁
        rcases bnd_eq_timeout h₂ with h₃ | ⟨_, _, _, h₄, _⟩
        · exact keyless_probe_only d hg.2 _ _ _ _ _ h₃
        · cases h₄
  | .act t k a, hg, ρ, s, q, s₁, tr, h => by
    simp only [keyless] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨_, _, _, h₁, _⟩
    · exact keyless_probe_only a hg _ _ _ _ _ h₀
    · exact absurd h₁ doAct_ne_timeout
  | .drop k, _, ρ, s, q, s₁, tr, h => by
    simp only [eval] at h; exact absurd h doDrop_ne_timeout
  | .for_ n xs body, hg, ρ, s, q, s₁, tr, h => by
    simp only [keyless, Bool.and_eq_true] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨ox, t, _, h₁, _⟩
    · exact keyless_probe_only xs hg.1 _ _ _ _ _ h₀
    · revert h₁; split
      · intro h₁
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · exact loop_probe_only (fun v s' => keyless_probe_only body hg.2 (v :: ρ) s') _ _ _ _ _ h₂
        · cases h₃
      · intro h₁; cases h₁
  | .infer m t a, hg, ρ, s, q, s₁, tr, h => by
    simp only [keyless] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨_, _, _, h₁, _⟩
    · exact keyless_probe_only a hg _ _ _ _ _ h₀
    · exact absurd h₁ doInfer_ne_timeout
  | .await q', _, ρ, s, q, s₁, tr, h => by
    simp only [eval] at h; split at h
    · cases h
    · cases h; exact ⟨q', rfl⟩
  | .atomic body r f g, hg, ρ, s, q, s₁, tr, h => by
    simp only [keyless, Bool.and_eq_true] at hg
    simp only [eval] at h
    split at h
    · cases h
    · rename_i q₀ s₀ t₀ hb
      obtain ⟨q', rfl⟩ := keyless_probe_only body hg.1.1 _ _ _ _ _ hb
      split at h
      · rename_i hy; simp [Cause.isAbort] at hy
      · obtain ⟨t', h₁, _⟩ := pre_eq_timeout h
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · rcases cond_eq h₂ with ⟨_, h₄⟩ | ⟨_, h₄⟩
          · exact keyless_probe_only f hg.1.2 _ _ _ _ _ h₄
          · exact keyless_probe_only g hg.2 _ _ _ _ _ h₄
        · cases h₃
    · rename_i hne _
      cases h
      exact absurd rfl (hne _ _ _)
  | .call .., hg, _, _, _, _, _, _ => by simp [keyless] at hg
  | .mint e, hg, ρ, s, q, s₁, tr, h => by
    simp only [keyless] at hg
    obtain ⟨_, _, s', tr', h₃, _⟩ := eval_mint_timeout h
    exact keyless_probe_only e hg _ _ _ _ _ h₃
  | .transfer p, _, ρ, s, q, s₁, tr, h => by
    simp only [eval] at h; split at h <;> cases h

/-- **T1 sem chamadas com chave**, o enunciado das versões anteriores, intacto: um programa
bem tipado, linear e `Guarded`, sem `call`, executa sem erro nenhum. -/
theorem soundness_keyless (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty}
    {ι : Lab} {β : Budget} {ρ : List Val} {s : St} (h : HasType S Γ pc e τ ι β)
    (hlin : Linear Γ e) (hg : Guarded e) (hk : keyless e = true) (h0 : Start c.grants Γ ρ s)
    (hB : s.spent + β ≤ c.limit) :
    ∃ o, eval S c e ρ s = .ok o ∧ HasTy o.v τ ∧ Within s.spent o.st.spent β ∧
      ∃ new : List Nat, o.st.used = new ++ s.used ∧
        ∀ id : Nat, id ∈ new ↔ ∃ k : Nat, ρ[k]? = some (Val.cap id) := by
  rcases soundness hO h hlin hg h0 hB with hok | ⟨k, s₁, tr, he⟩ | ⟨v, s₁, tr, he, _⟩
  · exact hok
  · obtain ⟨q', hq'⟩ := keyless_probe_only e hk ρ s _ s₁ tr he; cases hq'
  · obtain ⟨q', hq'⟩ := keyless_probe_only e hk ρ s _ s₁ tr he; cases hq'

/-- **T1, preservação.** Se a execução termina, o valor tem o tipo estático, o gasto real
fica dentro de β e, portanto, dentro do teto do agente. Vale com ou sem sagas. -/
theorem preservation (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} {ρ : List Val} {s : St} {o : Out} (h : HasType S Γ pc e τ ι β)
    (hlin : Linear Γ e) (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit)
    (he : eval S c e ρ s = .ok o) :
    HasTy o.v τ ∧ Within s.spent o.st.spent β ∧ o.st.spent ≤ c.limit := by
  rcases soundness_total hO h hlin h0 hB with ⟨o', ho', hv, hw, _⟩ | ⟨_, _, _, ho', _⟩
  · rw [ho'] at he
    cases he
    exact ⟨hv, hw, fun d => by have h1 := hB d; have h2 := (hw d).2; simp at h1; omega⟩
  · rw [ho'] at he; cases he

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
  revoke _ := fun ids hd hf h => h.trans ⟨ids, rfl, hd, hf⟩
  abort _ := fun _ _ _ h => h
  callDone := by intro _ s t id kid v _ hfree hk _; exact (Fresh.cons hfree).trans (Fresh.cons hk)
  callFailed := by intro _ s t id kid _ hfree hk _; exact (Fresh.cons hfree).trans (Fresh.cons hk)
  callRec := by
    intro _ s t id kid n v e _ hfree hk _ _ _; exact (Fresh.cons hfree).trans (Fresh.cons hk)
  callAbort := by
    intro _ s s₃ t id kid n v tr _ hfree hk _ _ h _
    exact ((Fresh.cons hfree).trans (Fresh.cons hk)).trans h
  mint := by
    intro s s' t hn _ h
    obtain ⟨new, hnew, hd, hf⟩ := h
    refine ⟨new.filter (· ≠ s.next), ?_, hd.filter _, fun x hx => hf x (List.mem_filter.mp hx).1⟩
    show s'.used.filter (· ≠ s.next) = _
    rw [hnew, List.filter_append]
    congr 1
    exact List.filter_eq_self.mpr fun x hx => by simp only [ne_eq, decide_eq_true_eq]; rintro rfl; exact hn hx
  transfer := Fresh.refl _

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
