import Aletheia.Soundness

/-!
# Teorema T9: toda chave termina em `Done`, `Failed` ou `Transferred`

Uma ferramenta `keyed` recebe uma chave `own IdemKey`, e a chave de cada operação é a
identidade da capacidade linear que a autoriza (`Eval.lean`). A chamada tem três respostas
possíveis: aplicada, recusada ou nenhuma (`TimeoutUnknown`). No último caso nasce a obrigação
`Pending[k]`, e a reconciliação consulta o status da operação até `fuel` vezes. Uma consulta
conclusiva decide `Done` ou `Failed`; se nenhuma for conclusiva, a obrigação é transferida ao
reconciliador durável (`Transferred`).

O teorema vale sobre o avaliador canônico `eval` dos T1–T6, para **qualquer** programa (com ou
sem sagas, bem tipado ou não), **qualquer** provedor (`Sig.reply` é arbitrária: pode recusar
tudo ou nunca responder) e nos **dois** desfechos da execução: valor final ou `timeout` de uma
pós-condição. Também vale nos caminhos de compensação de uma saga. Em cada caso, para toda
chave que aparece no traço, os eventos dela formam um ciclo de vida completo (`Lifecycle`):

* um único estado terminal, e ele é o último evento da chave. Nenhuma `Pending[k]` fica para
  trás, que seria o descarte silencioso;
* `Done` e `Failed` depois de `Pending[k]` exigem uma consulta conclusiva, a última. Um
  `timeout` nunca vira sucesso sem evidência;
* `Transferred` só acontece depois de `fuel` consultas, todas inconclusivas.

Com o T1 (`caps_exactly_once`), cada capacidade do agente é consumida uma vez. Uma
capacidade de ferramenta `keyed` usada por `act` cunha, portanto, exatamente uma chave.
-/

namespace Aletheia

open Lab Budget

/-! ## O ciclo de vida de uma chave -/

/-- A chave de um evento do protocolo. -/
def keyOf : Ev → Option Nat
  | .pending _ k _ => some k
  | .query k _ _ => some k
  | .settle k _ => some k
  | _ => none

/-- Os eventos do protocolo da chave `k`, em ordem. -/
def keyEvs (k : Nat) (tr : List Ev) : List Ev := tr.filter fun e => keyOf e == some k

def isSettle : Ev → Bool
  | .settle _ _ => true
  | _ => false

/-- As consultas da reconciliação e o que elas sustentam. `Done` e `Failed` só com uma
consulta conclusiva, a última; `Transferred` só depois de consultas todas inconclusivas. -/
inductive Probes (k : Nat) : Nat → List Ev → KEnd → Prop where
  | nil : Probes k 0 [] .transferred
  | done {n : Nat} (i : Nat) : Probes k (n + 1) [.query k i .done] .done
  | failed {n : Nat} (i : Nat) : Probes k (n + 1) [.query k i .failed] .failed
  | unknown {n : Nat} {qs : List Ev} {e : KEnd} (i : Nat) :
      Probes k n qs e → Probes k (n + 1) (.query k i .unknown :: qs) e

/-- O ciclo de vida de uma chave: a resposta direta do provedor, ou `Pending[k]` seguido da
reconciliação com combustível `n`; nos dois casos, um único estado terminal, no fim. -/
inductive Lifecycle (k : Nat) : List Ev → Prop where
  | done : Lifecycle k [.settle k .done]
  | failed : Lifecycle k [.settle k .failed]
  | reconciled {t n : Nat} {v : Val} {qs : List Ev} {e : KEnd} :
      Probes k n qs e → Lifecycle k (.pending t k v :: qs ++ [.settle k e])

theorem Probes.no_settle {k n : Nat} {qs : List Ev} {e : KEnd} (h : Probes k n qs e) :
    ∀ x ∈ qs, isSettle x = false := by
  induction h with
  | nil => simp
  | done i => simp [isSettle]
  | failed i => simp [isSettle]
  | unknown i _ ih =>
    intro x hx
    rcases List.mem_cons.mp hx with rfl | hx
    · rfl
    · exact ih x hx

/-- Um único estado terminal, e ele é o último evento da chave. -/
theorem Lifecycle.terminal {k : Nat} {l : List Ev} (h : Lifecycle k l) :
    ∃ pre e, l = pre ++ [.settle k e] ∧ ∀ x ∈ pre, isSettle x = false := by
  cases h with
  | done => exact ⟨[], .done, rfl, by simp⟩
  | failed => exact ⟨[], .failed, rfl, by simp⟩
  | @reconciled t n v qs e hq =>
    refine ⟨.pending t k v :: qs, e, by simp, fun x hx => ?_⟩
    rcases List.mem_cons.mp hx with rfl | hx
    · rfl
    · exact hq.no_settle x hx

theorem Probes.done_last {k n : Nat} {qs : List Ev} {e : KEnd} (h : Probes k n qs e) :
    e = .done → ∃ i, .query k i .done ∈ qs := by
  induction h with
  | nil => intro h; cases h
  | done i => intro _; exact ⟨i, by simp⟩
  | failed i => intro h; cases h
  | unknown i _ ih =>
    intro h
    obtain ⟨j, hj⟩ := ih h
    exact ⟨j, List.mem_cons_of_mem _ hj⟩

theorem Probes.transferred_all_unknown {k n : Nat} {qs : List Ev} {e : KEnd} (h : Probes k n qs e) :
    e = .transferred → qs.length = n ∧ ∀ x ∈ qs, ∃ i, x = .query k i .unknown := by
  induction h with
  | nil => intro _; exact ⟨rfl, by simp⟩
  | done i => intro h; cases h
  | failed i => intro h; cases h
  | unknown i _ ih =>
    intro h
    obtain ⟨hl, hall⟩ := ih h
    refine ⟨by simp [hl], fun x hx => ?_⟩
    rcases List.mem_cons.mp hx with rfl | hx
    · exact ⟨i, rfl⟩
    · exact hall x hx

/-! ## A reconciliação produz um ciclo de vida -/

theorem reconcile_probes (S : Sig) (k : Nat) :
    ∀ (i n : Nat), Probes k n (reconcile S k i n).1 (reconcile S k i n).2
  | _, 0 => .nil
  | i, n + 1 => by
    simp only [reconcile]
    split
    · exact .done i
    · exact .failed i
    · exact .unknown i (reconcile_probes S k (i + 1) n)

theorem keyEvs_queries {k k' : Nat} {qs : List Ev} (h : ∀ e ∈ qs, ∃ j r, e = .query k j r) :
    keyEvs k' qs = if k = k' then qs else [] := by
  unfold keyEvs
  by_cases hk : k = k'
  · subst hk
    simp only [if_true]
    exact List.filter_eq_self.mpr fun e he => by obtain ⟨j, r, rfl⟩ := h e he; simp [keyOf]
  · simp only [hk, if_false]
    exact List.filter_eq_nil_iff.mpr fun e he => by obtain ⟨j, r, rfl⟩ := h e he; simp [keyOf, hk]

theorem keyEvs_append (k : Nat) (a b : List Ev) : keyEvs k (a ++ b) = keyEvs k a ++ keyEvs k b := by
  simp [keyEvs, List.filter_append]

theorem keyEvs_cons_pending (k' t k : Nat) (v : Val) (l : List Ev) :
    keyEvs k' (.pending t k v :: l) = if k = k' then .pending t k v :: keyEvs k' l else keyEvs k' l := by
  by_cases h : k = k' <;> simp [keyEvs, List.filter_cons, keyOf, h]

theorem keyEvs_settle (k' k : Nat) (e : KEnd) :
    keyEvs k' [.settle k e] = if k = k' then [.settle k e] else [] := by
  by_cases h : k = k' <;> simp [keyEvs, List.filter_cons, keyOf, h]

/-- Os eventos de chave de uma ação: o ciclo de vida da sua própria chave e nada mais. -/
theorem keyEvs_actEvs (S : Sig) (t id : Nat) (v : Val) (k : Nat) :
    (k = id → keyEvs k (actEvs S t id v) = [] ∨ Lifecycle k (keyEvs k (actEvs S t id v))) ∧
    (k ≠ id → keyEvs k (actEvs S t id v) = []) := by
  unfold actEvs
  by_cases hk : S.keyed t = true
  · simp only [hk, if_true]
    cases h0 : S.reply id 0 with
    | done =>
      refine ⟨fun h => .inr ?_, fun h => ?_⟩
      · subst h; simp [keyEvs, keyOf]; exact .done
      · simp [keyEvs, keyOf, Ne.symm h]
    | failed =>
      refine ⟨fun h => .inr ?_, fun h => ?_⟩
      · subst h; simp [keyEvs, keyOf]; exact .failed
      · simp [keyEvs, keyOf, Ne.symm h]
    | unknown =>
      dsimp only
      have hq := reconcile_queries S id 1 (S.fuel t)
      have hp := reconcile_probes S id 1 (S.fuel t)
      have hact : ∀ k', keyEvs k'
          (if (reconcile S id 1 (S.fuel t)).2 == KEnd.done then [Ev.act t id v] else []) = [] := by
        intro k'; split <;> simp [keyEvs, keyOf]
      simp only [List.cons_append, keyEvs_append, keyEvs_cons_pending, keyEvs_queries hq, hact,
        keyEvs_settle, List.append_nil]
      refine ⟨fun h => .inr ?_, fun h => ?_⟩
      · subst h
        simp only [if_true]
        exact .reconciled hp
      · simp [Ne.symm h]
  · simp only [hk, Bool.false_eq_true, if_false]
    exact ⟨fun _ => .inl (by simp [keyEvs, keyOf]), fun _ => by simp [keyEvs, keyOf]⟩

/-! ## O invariante -/

/-- O invariante do T9: o conjunto consumido cresce sem repetição (`Fresh`); toda chave do
traço é uma capacidade consumida no trecho; e os eventos de cada chave formam um ciclo de
vida completo. -/
def KeysOk (s s' : St) (tr : List Ev) : Prop :=
  Fresh s.used s'.used ∧
  (∀ k, keyEvs k tr ≠ [] → k ∈ s'.used ∧ k ∉ s.used) ∧
  (∀ k, keyEvs k tr ≠ [] → Lifecycle k (keyEvs k tr))

theorem KeysOk.noKeys {s s' : St} {tr : List Ev} (hf : Fresh s.used s'.used)
    (h : ∀ k, keyEvs k tr = []) : KeysOk s s' tr :=
  ⟨hf, fun k hk => absurd (h k) hk, fun k hk => absurd (h k) hk⟩

theorem keyEvs_saga (k q r : Nat) (p : Bool) (S : Sig) (comp : List (Nat × Val)) (w : World) :
    keyEvs k (.fail q :: (compensate S comp w).evs ++ [.recover r p]) = [] := by
  have hc : (compensate S comp w).evs.filter (fun e => keyOf e == some k) = [] := by
    refine List.filter_eq_nil_iff.mpr fun e he => ?_
    obtain ⟨_, _, _, rfl⟩ := compensate_undo S _ _ _ he
    simp [keyOf]
  simp only [keyEvs, List.cons_append, List.filter_cons, List.filter_append, hc]
  simp [keyOf]

section
variable {S : Sig} {c : Cfg}

theorem keys_preserves : Preserves S c True KeysOk where
  refl s := KeysOk.noKeys (Fresh.refl _) fun _ => rfl
  trans := by
    rintro s₁ s₂ s₃ t₁ t₂ ⟨hf₁, hm₁, hl₁⟩ ⟨hf₂, hm₂, hl₂⟩
    have hsub : ∀ x, x ∈ s₂.used → x ∈ s₃.used := by
      obtain ⟨n, hn, _, _⟩ := hf₂; intro x hx; rw [hn]; exact List.mem_append_right _ hx
    have hsub₁ : ∀ x, x ∈ s₁.used → x ∈ s₂.used := by
      obtain ⟨n, hn, _, _⟩ := hf₁; intro x hx; rw [hn]; exact List.mem_append_right _ hx
    refine ⟨hf₁.trans hf₂, fun k hk => ?_, fun k hk => ?_⟩
    · rw [keyEvs_append] at hk
      by_cases h₁ : keyEvs k t₁ = []
      · rw [h₁, List.nil_append] at hk
        obtain ⟨a, b⟩ := hm₂ k hk
        exact ⟨a, fun h => b (hsub₁ k h)⟩
      · obtain ⟨a, b⟩ := hm₁ k h₁
        exact ⟨hsub k a, b⟩
    · rw [keyEvs_append] at hk ⊢
      by_cases h₁ : keyEvs k t₁ = []
      · rw [h₁, List.nil_append] at hk ⊢
        exact hl₂ k hk
      · by_cases h₂ : keyEvs k t₂ = []
        · rw [h₂, List.append_nil]; exact hl₁ k h₁
        · exact absurd (hm₁ k h₁).1 (hm₂ k h₂).2
  endorse _ := KeysOk.noKeys (Fresh.refl _) fun _ => by simp [keyEvs, keyOf]
  reject := KeysOk.noKeys (Fresh.refl _) fun _ => by simp [keyEvs, keyOf]
  act := by
    intro s t id v _ hfree
    refine ⟨Fresh.cons hfree, fun k hk => ?_, fun k hk => ?_⟩
    · by_cases h : k = id
      · subst h; exact ⟨List.mem_cons_self .., hfree⟩
      · exact absurd ((keyEvs_actEvs S t id v k).2 h) hk
    · by_cases h : k = id
      · rcases (keyEvs_actEvs S t id v k).1 h with h' | h'
        · exact absurd h' hk
        · exact h'
      · exact absurd ((keyEvs_actEvs S t id v k).2 h) hk
  drop hfree := KeysOk.noKeys (Fresh.cons hfree) fun _ => rfl
  infer := KeysOk.noKeys (Fresh.refl _) fun _ => rfl
  commit _ := fun h => h
  revoke _ := by
    rintro s s' t ids hd hf ⟨hfr, hm, hl⟩
    refine ⟨hfr.trans ⟨ids, rfl, hd, hf⟩, fun k hk => ?_, hl⟩
    obtain ⟨a, b⟩ := hm k hk
    exact ⟨List.mem_append_right _ a, b⟩
  abort _ := by
    rintro s s₁ t q r p ⟨hfr, hm, hl⟩
    refine ⟨hfr, fun k hk => ?_, fun k hk => ?_⟩
    · rw [List.append_assoc, keyEvs_append, keyEvs_saga, List.append_nil] at hk
      exact hm k hk
    · rw [List.append_assoc, keyEvs_append, keyEvs_saga, List.append_nil] at hk ⊢
      exact hl k hk

/-! ## O teorema -/

/-- **Teorema T9.** Em toda execução de todo programa, com qualquer provedor, nos dois
desfechos (valor ou `timeout` de uma pós-condição), os eventos de cada chave que aparece no
traço formam um ciclo de vida completo: a resposta direta (`Done` ou `Failed`), ou
`Pending[k]` seguido da reconciliação e de um único estado terminal (`Done`, `Failed` ou
`Transferred`). -/
theorem T9 (e : Exp) {ρ : List Val} {s : St} :
    (∀ {o : Out}, eval S c e ρ s = .ok o → ∀ k, keyEvs k o.tr ≠ [] → Lifecycle k (keyEvs k o.tr)) ∧
    (∀ {q : Nat} {s₁ : St} {tr : List Ev}, eval S c e ρ s = .error (.timeout q s₁ tr) →
      ∀ k, keyEvs k tr ≠ [] → Lifecycle k (keyEvs k tr)) :=
  ⟨fun h => ((eval_preserves keys_preserves e (.inl trivial)).1 h).2.2,
    fun h => ((eval_preserves keys_preserves e (.inl trivial)).2 h).2.2⟩

/-- O traço de um desfecho, seja ele qual for. -/
def Outcome.tr : Except Err Out → Option (List Ev)
  | .ok o => some o.tr
  | .error (.timeout _ _ tr) => some tr
  | .error _ => none

theorem T9_outcome (e : Exp) {ρ : List Val} {s : St} {tr : List Ev}
    (h : Outcome.tr (eval S c e ρ s) = some tr) : ∀ k, keyEvs k tr ≠ [] → Lifecycle k (keyEvs k tr) := by
  revert h
  cases hr : eval S c e ρ s with
  | ok o => intro h; cases h; exact (T9 e).1 hr
  | error err =>
    cases err with
    | timeout q s₁ tr' => intro h; cases h; exact (T9 e).2 hr
    | _ => intro h; cases h

/-- **T9, sem descarte silencioso.** Toda `Pending[k]` do traço chega a um estado terminal, e
toda chave tem exatamente um, no fim. -/
theorem T9_no_silent_discard (e : Exp) {ρ : List Val} {s : St} {tr : List Ev}
    (h : Outcome.tr (eval S c e ρ s) = some tr) {t k : Nat} {v : Val} (hp : Ev.pending t k v ∈ tr) :
    ∃ pre r, keyEvs k tr = pre ++ [.settle k r] ∧ ∀ x ∈ pre, isSettle x = false := by
  have hk : keyEvs k tr ≠ [] := by
    intro he
    have : Ev.pending t k v ∈ keyEvs k tr := List.mem_filter.mpr ⟨hp, by simp [keyOf]⟩
    rw [he] at this
    simp at this
  exact (T9_outcome e h k hk).terminal

/-- **T9, evidência.** Depois de `Pending[k]`, `Done` só com uma consulta que observou a
operação aplicada: um `timeout` nunca vira sucesso por omissão. -/
theorem T9_done_observed (e : Exp) {ρ : List Val} {s : St} {tr : List Ev}
    (h : Outcome.tr (eval S c e ρ s) = some tr) {t k : Nat} {v : Val} (hp : Ev.pending t k v ∈ tr)
    (hd : Ev.settle k .done ∈ tr) : ∃ i, Ev.query k i .done ∈ tr := by
  have hmem : ∀ x ∈ tr, keyOf x = some k → x ∈ keyEvs k tr :=
    fun x hx hkx => List.mem_filter.mpr ⟨hx, by simp [hkx]⟩
  have hk : keyEvs k tr ≠ [] := by
    intro he; have := hmem _ hp rfl; rw [he] at this; simp at this
  have hsub : ∀ x ∈ keyEvs k tr, x ∈ tr := fun x hx => (List.mem_filter.mp hx).1
  have hd' := hmem _ hd rfl
  have hp' := hmem _ hp rfl
  cases hl : keyEvs k tr with
  | nil => exact absurd hl hk
  | cons x xs =>
    have hlc := T9_outcome e h k hk
    rw [hl] at hlc hd' hp'
    cases hlc with
    | done => simp at hp'
    | failed => simp at hp'
    | @reconciled t' n v' qs r hq =>
      have hr : r = .done := by
        rcases List.mem_cons.mp hd' with h' | h'
        · cases h'
        · rcases List.mem_append.mp h' with h'' | h''
          · have := hq.no_settle _ h''; simp [isSettle] at this
          · simp at h''; exact h''.symm
      obtain ⟨i, hi⟩ := hq.done_last hr
      refine ⟨i, hsub _ ?_⟩
      rw [hl]
      exact List.mem_cons_of_mem _ (List.mem_append_left _ hi)

/-- **T9, transferência.** `Transferred` só depois de `fuel` consultas, todas inconclusivas. -/
theorem T9_transferred_exhausted (e : Exp) {ρ : List Val} {s : St} {tr : List Ev}
    (h : Outcome.tr (eval S c e ρ s) = some tr) {k : Nat}
    (hd : Ev.settle k .transferred ∈ tr) :
    ∃ t v n qs, keyEvs k tr = .pending t k v :: qs ++ [.settle k .transferred] ∧ qs.length = n ∧
      ∀ x ∈ qs, ∃ i, x = .query k i .unknown := by
  have hd' : Ev.settle k .transferred ∈ keyEvs k tr := List.mem_filter.mpr ⟨hd, by simp [keyOf]⟩
  have hk : keyEvs k tr ≠ [] := by intro he; rw [he] at hd'; simp at hd'
  have hlc := T9_outcome e h k hk
  generalize hL : keyEvs k tr = L at hlc hd'
  cases hlc with
  | done => simp at hd'
  | failed => simp at hd'
  | @reconciled t n v qs r hq =>
    have hr : r = .transferred := by
      rcases List.mem_cons.mp hd' with h' | h'
      · cases h'
      · rcases List.mem_append.mp h' with h'' | h''
        · have := hq.no_settle _ h''; simp [isSettle] at this
        · simp at h''; exact h''.symm
    subst hr
    obtain ⟨hl, hall⟩ := hq.transferred_all_unknown rfl
    exact ⟨t, v, n, qs, rfl, hl, hall⟩

end

/-! ## Exemplos -/

namespace KeysExample

/-- Uma ferramenta com chave (`scale(c, n, keys.mint())`), combustível 3. O provedor não
responde à chamada nem à primeira consulta e confirma na segunda. -/
def S : Sig where
  cost := fun _ => 0
  mcost := fun _ => 0
  arg := fun _ => .int
  effect := fun _ => 0
  pol := fun _ _ => true
  keyed := fun _ => true
  fuel := fun _ => 3
  reply := fun _ i => if i = 2 then .done else .unknown

/-- O provedor que nunca responde. -/
def S' : Sig := { S with reply := fun _ _ => .unknown }

def c : Cfg := ⟨fun _ _ _ _ => .unit, fun _ => 100, fun _ => [0]⟩

def s₀ : St := ⟨[], 0, 0, fun _ => 0, []⟩

/-- `TimeoutUnknown`, uma consulta inconclusiva, a confirmação: `Done`, e o efeito acontece. -/
example : (eval S c (.act 0 0 (.val (.int 5))) [.cap 7] s₀).map (·.tr) =
    .ok [.pending 0 7 (.int 5), .query 7 1 .unknown, .query 7 2 .done, .act 0 7 (.int 5),
      .settle 7 .done] := rfl

/-- Sem resposta: três consultas inconclusivas e a transferência; o efeito não entra no
traço nem no mundo do agente. -/
example : (eval S' c (.act 0 0 (.val (.int 5))) [.cap 7] s₀).map (·.tr) =
    .ok [.pending 0 7 (.int 5), .query 7 1 .unknown, .query 7 2 .unknown, .query 7 3 .unknown,
      .settle 7 .transferred] := rfl

end KeysExample

end Aletheia
