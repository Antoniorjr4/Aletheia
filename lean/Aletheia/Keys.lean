import Aletheia.Soundness

/-!
# Teorema T9: toda chave termina em `Done`, `Failed` ou `Transferred`

Uma ferramenta com chave roda por `call`, e a chave da operação é a identidade da capacidade
linear que a autoriza (`Eval.lean`). A chamada tem três respostas possíveis: aplicada,
recusada ou nenhuma (`TimeoutUnknown`). No último caso nasce a obrigação `Pending[k]`, e a
reconciliação tenta até `fuel` vezes pelo protocolo da classe do provedor. Uma resposta
conclusiva decide `Done` ou `Failed`. Sem ela, o tratador `reconcile p fuel n else e` recebe a
obrigação, precisa entregá-la por `transfer`, e só então a chamada fecha a chave em
`Transferred` e encerra o evento.

O teorema vale sobre o avaliador canônico `eval` dos T1–T6, para **qualquer** programa (com ou
sem sagas, bem tipado ou não), **qualquer** provedor (`Sig.reply` é arbitrária: pode recusar
tudo ou nunca responder) e nos dois desfechos da execução: valor final ou falha capturável
(pós-condição, recusa, encerramento pelo tratador). Também vale nos caminhos de compensação
de uma saga. Em cada caso, para toda chave que aparece no traço, os eventos dela formam um
ciclo de vida completo (`Lifecycle`):

* um único estado terminal, e ele é o último evento da chave. Nenhuma `Pending[k]` fica para
  trás, que seria o descarte silencioso;
* `Done` e `Failed` depois de `Pending[k]` exigem uma resposta conclusiva, a última. Um
  `TimeoutUnknown` nunca vira sucesso sem evidência;
* `Transferred` só vem depois de esgotar o combustível com respostas inconclusivas, ou de um
  provedor `opaque`, e só depois de o tratador entregar a obrigação por `transfer`.

`Transferred` **não** diz que a operação foi desfeita nem que não aconteceu: com um provedor
honesto, `Done` implica o efeito aplicado e `Failed` implica o efeito ausente
(`T9_honest`), mas nada se prova sobre `Transferred`. O exemplo `transferred_but_applied`
mostra o efeito no mundo e a chave transferida.
-/

namespace Aletheia

open Lab Budget

/-! ## O ciclo de vida de uma chave -/

/-- Os eventos do protocolo da chave `k`, em ordem. -/
def keyEvs (k : Nat) (tr : List Ev) : List Ev := tr.filter fun e => keyOf e == some k

def isSettle : Ev → Bool
  | .settle _ _ => true
  | _ => false

/-- As tentativas da reconciliação e o que elas sustentam. `Done` e `Failed` só com uma
resposta conclusiva, a última; `Transferred` só com o combustível esgotado em respostas
inconclusivas, ou com o provedor `opaque` (uma tentativa sem protocolo). -/
inductive Probes (k : Nat) : Nat → List Ev → KEnd → Prop where
  | nil : Probes k 0 [] .transferred
  | noProto {n : Nat} (i : Nat) : Probes k (n + 1) [.query k i none] .transferred
  | done {n : Nat} (i : Nat) : Probes k (n + 1) [.query k i (some .done)] .done
  | failed {n : Nat} (i : Nat) : Probes k (n + 1) [.query k i (some .failed)] .failed
  | unknown {n : Nat} {qs : List Ev} {e : KEnd} (i : Nat) :
      Probes k n qs e → Probes k (n + 1) (.query k i (some .unknown) :: qs) e

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
  | noProto i => simp [isSettle]
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
    e = .done → ∃ i, .query k i (some .done) ∈ qs := by
  induction h with
  | nil => intro h; cases h
  | noProto i => intro h; cases h
  | done i => intro _; exact ⟨i, by simp⟩
  | failed i => intro h; cases h
  | unknown i _ ih =>
    intro h
    obtain ⟨j, hj⟩ := ih h
    exact ⟨j, List.mem_cons_of_mem _ hj⟩

theorem Probes.failed_last {k n : Nat} {qs : List Ev} {e : KEnd} (h : Probes k n qs e) :
    e = .failed → ∃ i, .query k i (some .failed) ∈ qs := by
  induction h with
  | nil => intro h; cases h
  | noProto i => intro h; cases h
  | done i => intro h; cases h
  | failed i => intro _; exact ⟨i, by simp⟩
  | unknown i _ ih =>
    intro h
    obtain ⟨j, hj⟩ := ih h
    exact ⟨j, List.mem_cons_of_mem _ hj⟩

/-- `Transferred` só com respostas inconclusivas: o combustível inteiro, ou um provedor
`opaque`. -/
theorem Probes.transferred_inconclusive {k n : Nat} {qs : List Ev} {e : KEnd} (h : Probes k n qs e) :
    e = .transferred →
      (∀ x ∈ qs, ∃ i, x = .query k i (some .unknown) ∨ x = .query k i none) ∧
      (qs.length = n ∨ ∃ i, .query k i none ∈ qs) := by
  induction h with
  | nil => intro _; exact ⟨by simp, .inl rfl⟩
  | noProto i => intro _; exact ⟨by simp, .inr ⟨i, by simp⟩⟩
  | done i => intro h; cases h
  | failed i => intro h; cases h
  | unknown i _ ih =>
    intro h
    obtain ⟨hall, hlen⟩ := ih h
    refine ⟨fun x hx => ?_, ?_⟩
    · rcases List.mem_cons.mp hx with rfl | hx
      · exact ⟨i, .inl rfl⟩
      · exact hall x hx
    · rcases hlen with hl | ⟨j, hj⟩
      · exact .inl (by simp [hl])
      · exact .inr ⟨j, List.mem_cons_of_mem _ hj⟩

/-! ## A reconciliação produz um ciclo de vida -/

theorem reconcile_probes (S : Sig) (cls : Prov) (k : Nat) :
    ∀ (i n : Nat), Probes k n (reconcile S cls k i n).1 (reconcile S cls k i n).2
  | _, 0 => .nil
  | i, n + 1 => by
    simp only [reconcile]
    split
    · exact .noProto i
    · split
      · exact .done i
      · exact .failed i
      · exact .unknown i (reconcile_probes S cls k (i + 1) n)

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

/-- Um traço sem eventos de chave (o do tratador, pela guarda `transferOnly`). -/
theorem keyEvs_of_kevs_nil {tr : List Ev} (h : kevs tr = []) (k : Nat) : keyEvs k tr = [] := by
  unfold keyEvs
  refine List.filter_eq_nil_iff.mpr fun e he => ?_
  have : (keyOf e).isSome = false := by
    have := List.filter_eq_nil_iff.mp h e he
    simpa using this
  cases hk : keyOf e with
  | none => simp
  | some x => simp [hk] at this

theorem kevs_nil_of_transferOnly {k : Nat} {tr : List Ev} (h : transferOnly k tr = true) : kevs tr = [] := by
  simp only [transferOnly, Bool.and_eq_true, List.isEmpty_iff] at h
  exact h.1

/-- Os eventos de chave de uma chamada que reconciliou: o ciclo de vida da sua chave. -/
theorem keyEvs_rec (S : Sig) (cls : Prov) (t id n : Nat) (v : Val) (tail : List Ev) (e : KEnd)
    (ht : ∀ k, keyEvs k tail = if id = k then [.settle id e] else []) (k : Nat) :
    keyEvs k (.pending t id v :: ((reconcile S cls id 1 n).1 ++ tail)) =
      if id = k then .pending t id v :: ((reconcile S cls id 1 n).1 ++ [.settle id e]) else [] := by
  rw [keyEvs_cons_pending, keyEvs_append, keyEvs_queries (reconcile_queries S cls id 1 n), ht]
  by_cases h : id = k <;> simp [h]

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

theorem keyEvs_saga (k : Nat) (q : Cause) (r : Nat) (p : Bool) (S : Sig) (comp : List (Nat × Val)) (w : World) :
    keyEvs k (.fail q :: (compensate S comp w).evs ++ [.recover r p]) = [] := by
  have hc : (compensate S comp w).evs.filter (fun e => keyOf e == some k) = [] := by
    refine List.filter_eq_nil_iff.mpr fun e he => ?_
    obtain ⟨_, _, _, rfl⟩ := compensate_undo S _ _ _ he
    simp [keyOf]
  simp only [keyEvs, List.cons_append, List.filter_cons, List.filter_append, hc]
  simp [keyOf]

/-- Uma chamada que consome `id` e cujos eventos de chave são o ciclo de vida de `id`. -/
theorem keysOk_call {s s' : St} {tr : List Ev} {id : Nat} {L : List Ev}
    (hf : Fresh s.used s'.used) (hin : id ∈ s'.used) (hfree : id ∉ s.used)
    (hk : ∀ k, keyEvs k tr = if id = k then L else []) (hL : Lifecycle id L) : KeysOk s s' tr := by
  refine ⟨hf, fun k hne => ?_, fun k hne => ?_⟩
  · by_cases h : id = k
    · subst h; exact ⟨hin, hfree⟩
    · rw [hk k, if_neg h] at hne; exact absurd rfl hne
  · by_cases h : id = k
    · subst h; rw [hk id, if_pos rfl]; exact hL
    · rw [hk k, if_neg h] at hne; exact absurd rfl hne

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
  act _ hfree := KeysOk.noKeys (Fresh.cons hfree) fun _ => by simp [keyEvs, keyOf, actEvs]
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
  callDone := by
    intro _ s t id v _ hfree
    refine keysOk_call (L := [.settle id .done]) (Fresh.cons hfree) (List.mem_cons_self ..) hfree
      (fun k => ?_) .done
    by_cases h : id = k <;> simp [keyEvs, keyOf, h]
  callFailed := by
    intro _ s t id _ hfree
    refine keysOk_call (L := [.settle id .failed]) (Fresh.cons hfree) (List.mem_cons_self ..) hfree
      (fun k => ?_) .failed
    by_cases h : id = k <;> simp [keyEvs, keyOf, h]
  callRec := by
    intro _ s t id n v e _ hfree hr _
    refine keysOk_call (Fresh.cons hfree) (List.mem_cons_self ..) hfree
      (keyEvs_rec S (S.prov t) t id n v [.settle id e] e (fun k => keyEvs_settle k id e)) ?_
    have := reconcile_probes S (S.prov t) id 1 n
    rw [hr] at this
    exact .reconciled this
  callAbort := by
    intro _ s s₃ t id n v tr _ hfree hr ⟨hf, _, _⟩ hto
    have hnil := keyEvs_of_kevs_nil (kevs_nil_of_transferOnly hto)
    have hsub : id ∈ s₃.used := by
      obtain ⟨m, hm, _, _⟩ := hf; rw [hm]; exact List.mem_append_right _ (List.mem_cons_self ..)
    refine keysOk_call ((Fresh.cons hfree).trans hf) hsub hfree
      (keyEvs_rec S (S.prov t) t id n v (tr ++ [.settle id .transferred]) .transferred
        (fun k => by rw [keyEvs_append, hnil, keyEvs_settle]; simp)) ?_
    have := reconcile_probes S (S.prov t) id 1 n
    rw [hr] at this
    exact .reconciled this
  transfer := KeysOk.noKeys (Fresh.refl _) fun _ => by simp [keyEvs, keyOf]

/-! ## O teorema -/

/-- **Teorema T9.** Em toda execução de todo programa, com qualquer provedor, nos dois
desfechos (valor ou falha capturável), os eventos de cada chave que aparece no traço formam
um ciclo de vida completo: a resposta direta (`Done` ou `Failed`), ou `Pending[k]` seguido da
reconciliação e de um único estado terminal (`Done`, `Failed` ou `Transferred`). -/
theorem T9 (e : Exp) {ρ : List Val} {s : St} :
    (∀ {o : Out}, eval S c e ρ s = .ok o → ∀ k, keyEvs k o.tr ≠ [] → Lifecycle k (keyEvs k o.tr)) ∧
    (∀ {q : Cause} {s₁ : St} {tr : List Ev}, eval S c e ρ s = .error (.timeout q s₁ tr) →
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

/-- **T9, evidência.** Depois de `Pending[k]`, `Done` só com uma tentativa que observou a
operação aplicada: um `TimeoutUnknown` nunca vira sucesso por omissão. -/
theorem T9_done_observed (e : Exp) {ρ : List Val} {s : St} {tr : List Ev}
    (h : Outcome.tr (eval S c e ρ s) = some tr) {t k : Nat} {v : Val} (hp : Ev.pending t k v ∈ tr)
    (hd : Ev.settle k .done ∈ tr) : ∃ i, Ev.query k i (some .done) ∈ tr := by
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

/-- **T9, transferência.** `Transferred` só depois de `Pending[k]` e de respostas todas
inconclusivas: o combustível inteiro, ou um provedor `opaque`. -/
theorem T9_transferred_exhausted (e : Exp) {ρ : List Val} {s : St} {tr : List Ev}
    (h : Outcome.tr (eval S c e ρ s) = some tr) {k : Nat}
    (hd : Ev.settle k .transferred ∈ tr) :
    ∃ t v n qs, keyEvs k tr = .pending t k v :: qs ++ [.settle k .transferred] ∧
      (∀ x ∈ qs, ∃ i, x = .query k i (some .unknown) ∨ x = .query k i none) ∧
      (qs.length = n ∨ ∃ i, .query k i none ∈ qs) := by
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
    obtain ⟨hall, hlen⟩ := hq.transferred_inconclusive rfl
    exact ⟨t, v, n, qs, rfl, hall, hlen⟩

/-! ## `Transferred` exige um `transfer` explícito -/

/-- Toda chave fechada em `Transferred` foi entregue antes por `transfer`. -/
def Handed (tr : List Ev) : Prop := ∀ k, Ev.settle k .transferred ∈ tr → Ev.handoff k ∈ tr

theorem handoff_mem_of_transferOnly {k : Nat} {tr : List Ev} (h : transferOnly k tr = true) :
    Ev.handoff k ∈ tr := by
  simp only [transferOnly, Bool.and_eq_true] at h
  obtain ⟨_, h⟩ := h
  have : ∀ l : List Ev, l = handoffs tr → (match l with | [Ev.handoff k'] => k' == k | _ => false) = true →
      Ev.handoff k ∈ tr := by
    intro l hl hm
    match l, hm with
    | [Ev.handoff k'], hm =>
      simp at hm; subst hm
      have : Ev.handoff k' ∈ handoffs tr := by rw [← hl]; simp
      exact (List.mem_filter.mp this).1
  exact this _ rfl h

theorem handed_preserves : Preserves S c True (fun _ _ tr => Handed tr) where
  refl _ := fun _ h => by simp at h
  trans h₁ h₂ := fun k hk => by
    rcases List.mem_append.mp hk with hk | hk
    · exact List.mem_append_left _ (h₁ k hk)
    · exact List.mem_append_right _ (h₂ k hk)
  endorse _ := fun _ h => by simp at h
  reject := fun _ h => by simp at h
  act _ _ := fun _ h => by simp [actEvs] at h
  drop _ := fun _ h => by simp at h
  infer := fun _ h => by simp at h
  commit _ := fun h => h
  revoke _ := fun _ _ _ h => h
  abort _ := by
    intro s s₁ t q r p h k hk
    rcases List.mem_append.mp hk with hk | hk
    · rcases List.mem_append.mp hk with hk | hk
      · exact List.mem_append_left _ (List.mem_append_left _ (h k hk))
      · rcases List.mem_cons.mp hk with hk | hk
        · cases hk
        · obtain ⟨_, _, _, h'⟩ := compensate_undo S _ _ _ hk; cases h'
    · simp at hk
  callDone := by intro _ s t id v _ _ k h; simp at h
  callFailed := by intro _ s t id _ _ k h; simp at h
  callRec := by
    intro _ s t id n v e _ _ _ hne k hk
    rcases List.mem_cons.mp hk with hk | hk
    · cases hk
    · rcases List.mem_append.mp hk with hk | hk
      · obtain ⟨j, r, h'⟩ := reconcile_queries S (S.prov t) id 1 n _ hk; cases h'
      · simp at hk; obtain ⟨_, rfl⟩ := hk; exact absurd rfl hne
  callAbort := by
    intro _ s s₃ t id n v tr _ _ _ hh hto k hk
    rcases List.mem_cons.mp hk with hk | hk
    · cases hk
    · rcases List.mem_append.mp hk with hk | hk
      · obtain ⟨j, r, h'⟩ := reconcile_queries S (S.prov t) id 1 n _ hk; cases h'
      · rcases List.mem_append.mp hk with hk | hk
        · exact List.mem_cons_of_mem _ (List.mem_append_right _ (List.mem_append_left _ (hh k hk)))
        · simp at hk; subst hk
          exact List.mem_cons_of_mem _ (List.mem_append_right _ (List.mem_append_left _
            (handoff_mem_of_transferOnly hto)))
  transfer := fun _ h => by simp at h

/-- **T9, transferência explícita.** Nenhuma chave chega a `Transferred` sem que o tratador
a tenha entregado por `transfer`: o fechamento é consequência da entrega, não um default. -/
theorem T9_transfer_explicit (e : Exp) {ρ : List Val} {s : St} {tr : List Ev}
    (h : Outcome.tr (eval S c e ρ s) = some tr) {k : Nat}
    (hd : Ev.settle k .transferred ∈ tr) : Ev.handoff k ∈ tr := by
  revert h
  cases hr : eval S c e ρ s with
  | ok o => intro h; cases h; exact (eval_preserves handed_preserves e (.inl trivial)).1 hr k hd
  | error err =>
    cases err with
    | timeout q s₁ tr' => intro h; cases h; exact (eval_preserves handed_preserves e (.inl trivial)).2 hr k hd
    | _ => intro h; cases h

/-! ## Provedor honesto: o que `Done` e `Failed` dizem sobre o mundo -/

/-- As respostas registradas são as do provedor: toda tentativa no traço com resposta `r`
é `S.reply k i = r`. -/
def Faithful (S : Sig) (tr : List Ev) : Prop :=
  ∀ k i r, Ev.query k i (some r) ∈ tr → S.reply k i = r

theorem reconcile_faithful (S : Sig) (cls : Prov) (k : Nat) :
    ∀ (i n : Nat) {k' j : Nat} {r : Reply}, Ev.query k' j (some r) ∈ (reconcile S cls k i n).1 →
      S.reply k' j = r
  | _, 0, _, _, _, h => by simp [reconcile] at h
  | i, n + 1, k', j, r, h => by
    simp only [reconcile] at h
    split at h
    · simp at h
    · rename_i hcls
      split at h
      · rename_i hrep; simp at h; obtain ⟨rfl, rfl, rfl⟩ := h; exact hrep
      · rename_i hrep; simp at h; obtain ⟨rfl, rfl, rfl⟩ := h; exact hrep
      · rename_i hrep
        simp only [List.mem_cons] at h
        rcases h with h | h
        · simp at h; obtain ⟨rfl, rfl, rfl⟩ := h; exact hrep
        · exact reconcile_faithful S cls k (i + 1) n h

theorem faithful_preserves : Preserves S c True (fun _ _ tr => Faithful S tr) where
  refl _ := fun _ _ _ h => by simp at h
  trans h₁ h₂ := fun k i r hk => by
    rcases List.mem_append.mp hk with hk | hk
    · exact h₁ k i r hk
    · exact h₂ k i r hk
  endorse _ := fun _ _ _ h => by simp at h
  reject := fun _ _ _ h => by simp at h
  act _ _ := fun _ _ _ h => by simp [actEvs] at h
  drop _ := fun _ _ _ h => by simp at h
  infer := fun _ _ _ h => by simp at h
  commit _ := fun h => h
  revoke _ := fun _ _ _ h => h
  abort _ := by
    intro s s₁ t q r p h k i x hk
    rcases List.mem_append.mp hk with hk | hk
    · rcases List.mem_append.mp hk with hk | hk
      · exact h k i x hk
      · rcases List.mem_cons.mp hk with hk | hk
        · cases hk
        · obtain ⟨_, _, _, h'⟩ := compensate_undo S _ _ _ hk; cases h'
    · simp at hk
  callDone := by intro _ s t id v _ _ k i r h; simp at h
  callFailed := by intro _ s t id _ _ k i r h; simp at h
  callRec := by
    intro _ s t id n v e _ _ _ _ k i r hk
    rcases List.mem_cons.mp hk with hk | hk
    · cases hk
    · rcases List.mem_append.mp hk with hk | hk
      · exact reconcile_faithful S (S.prov t) id 1 n hk
      · simp at hk
  callAbort := by
    intro _ s s₃ t id n v tr _ _ _ hh _ k i r hk
    rcases List.mem_cons.mp hk with hk | hk
    · cases hk
    · rcases List.mem_append.mp hk with hk | hk
      · exact reconcile_faithful S (S.prov t) id 1 n hk
      · rcases List.mem_append.mp hk with hk | hk
        · exact hh k i r hk
        · simp at hk
  transfer := fun _ _ _ h => by simp at h

/-- Um provedor honesto: uma tentativa de reconciliação conclusiva diz a verdade sobre o
efeito (`applied`). A chamada original (tentativa 0) não entra: ela não respondeu. -/
def Honest (S : Sig) : Prop :=
  ∀ k i, 1 ≤ i → (S.reply k i = .done → S.applied k = true) ∧ (S.reply k i = .failed → S.applied k = false)

theorem reconcile_index (S : Sig) (cls : Prov) (k : Nat) :
    ∀ (i n : Nat) {k' j : Nat} {r : Option Reply}, Ev.query k' j r ∈ (reconcile S cls k i n).1 → i ≤ j
  | _, 0, _, _, _, h => by simp [reconcile] at h
  | i, n + 1, k', j, r, h => by
    simp only [reconcile] at h
    split at h
    · simp at h; omega
    · split at h
      · simp at h; omega
      · simp at h; omega
      · simp only [List.mem_cons] at h
        rcases h with h | h
        · simp at h; omega
        · have := reconcile_index S cls k (i + 1) n h; omega

/-- As tentativas registradas são de reconciliação: índice ≥ 1. -/
def Indexed (tr : List Ev) : Prop := ∀ k i r, Ev.query k i r ∈ tr → 1 ≤ i

theorem indexed_preserves : Preserves S c True (fun _ _ tr => Indexed tr) where
  refl _ := fun _ _ _ h => by simp at h
  trans h₁ h₂ := fun k i r hk => by
    rcases List.mem_append.mp hk with hk | hk
    · exact h₁ k i r hk
    · exact h₂ k i r hk
  endorse _ := fun _ _ _ h => by simp at h
  reject := fun _ _ _ h => by simp at h
  act _ _ := fun _ _ _ h => by simp [actEvs] at h
  drop _ := fun _ _ _ h => by simp at h
  infer := fun _ _ _ h => by simp at h
  commit _ := fun h => h
  revoke _ := fun _ _ _ h => h
  abort _ := by
    intro s s₁ t q r p h k i x hk
    rcases List.mem_append.mp hk with hk | hk
    · rcases List.mem_append.mp hk with hk | hk
      · exact h k i x hk
      · rcases List.mem_cons.mp hk with hk | hk
        · cases hk
        · obtain ⟨_, _, _, h'⟩ := compensate_undo S _ _ _ hk; cases h'
    · simp at hk
  callDone := by intro _ s t id v _ _ k i r h; simp at h
  callFailed := by intro _ s t id _ _ k i r h; simp at h
  callRec := by
    intro _ s t id n v e _ _ _ _ k i r hk
    rcases List.mem_cons.mp hk with hk | hk
    · cases hk
    · rcases List.mem_append.mp hk with hk | hk
      · exact reconcile_index S (S.prov t) id 1 n hk
      · simp at hk
  callAbort := by
    intro _ s s₃ t id n v tr _ _ _ hh _ k i r hk
    rcases List.mem_cons.mp hk with hk | hk
    · cases hk
    · rcases List.mem_append.mp hk with hk | hk
      · exact reconcile_index S (S.prov t) id 1 n hk
      · rcases List.mem_append.mp hk with hk | hk
        · exact hh k i r hk
        · simp at hk
  transfer := fun _ _ _ h => by simp at h

/-- **T9, provedor honesto.** Se o provedor diz a verdade nas tentativas de reconciliação,
`Done` depois de `Pending[k]` implica o efeito de `k` aplicado, e `Failed` depois de
`Pending[k]` implica o efeito ausente. Nada se conclui de `Transferred`: o efeito pode existir
(`transferred_but_applied`). -/
theorem T9_honest (hH : Honest S) (e : Exp) {ρ : List Val} {s : St} {tr : List Ev}
    (h : Outcome.tr (eval S c e ρ s) = some tr) {t k : Nat} {v : Val} (hp : Ev.pending t k v ∈ tr) :
    (Ev.settle k .done ∈ tr → S.applied k = true) ∧ (Ev.settle k .failed ∈ tr → S.applied k = false) := by
  have hfa : Faithful S tr := by
    revert h; cases hr : eval S c e ρ s with
    | ok o => intro h; cases h; exact (eval_preserves faithful_preserves e (.inl trivial)).1 hr
    | error err =>
      cases err with
      | timeout q s₁ tr' => intro h; cases h; exact (eval_preserves faithful_preserves e (.inl trivial)).2 hr
      | _ => intro h; cases h
  have hix : Indexed tr := by
    revert h; cases hr : eval S c e ρ s with
    | ok o => intro h; cases h; exact (eval_preserves indexed_preserves e (.inl trivial)).1 hr
    | error err =>
      cases err with
      | timeout q s₁ tr' => intro h; cases h; exact (eval_preserves indexed_preserves e (.inl trivial)).2 hr
      | _ => intro h; cases h
  have hmem : ∀ x ∈ tr, keyOf x = some k → x ∈ keyEvs k tr :=
    fun x hx hkx => List.mem_filter.mpr ⟨hx, by simp [hkx]⟩
  have hk : keyEvs k tr ≠ [] := by
    intro he; have := hmem _ hp rfl; rw [he] at this; simp at this
  have hsub : ∀ x ∈ keyEvs k tr, x ∈ tr := fun x hx => (List.mem_filter.mp hx).1
  have hlc := T9_outcome e h k hk
  have hp' := hmem _ hp rfl
  refine ⟨fun hd => ?_, fun hd => ?_⟩
  · have hd' := hmem _ hd rfl
    generalize hL : keyEvs k tr = L at hlc hd' hp'
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
      have hin : Ev.query k i (some .done) ∈ tr :=
        hsub _ (by rw [hL]; exact List.mem_cons_of_mem _ (List.mem_append_left _ hi))
      exact (hH k i (hix k i _ hin)).1 (hfa k i _ hin)
  · have hd' := hmem _ hd rfl
    generalize hL : keyEvs k tr = L at hlc hd' hp'
    cases hlc with
    | done => simp at hp'
    | failed => simp at hp'
    | @reconciled t' n v' qs r hq =>
      have hr : r = .failed := by
        rcases List.mem_cons.mp hd' with h' | h'
        · cases h'
        · rcases List.mem_append.mp h' with h'' | h''
          · have := hq.no_settle _ h''; simp [isSettle] at this
          · simp at h''; exact h''.symm
      obtain ⟨i, hi⟩ := hq.failed_last hr
      have hin : Ev.query k i (some .failed) ∈ tr :=
        hsub _ (by rw [hL]; exact List.mem_cons_of_mem _ (List.mem_append_left _ hi))
      exact (hH k i (hix k i _ hin)).2 (hfa k i _ hin)

end

/-! ## Exemplos -/

namespace KeysExample

/-- Uma ferramenta com chave (`scale(c, n, keys.mint())`), provedor `queryable`. O provedor
não responde à chamada nem à primeira consulta e confirma na segunda. -/
def S : Sig where
  cost := fun _ => 0
  mcost := fun _ => 0
  arg := fun _ => .int
  effect := fun _ => 0
  pol := fun _ _ => true
  step := fun _ v w => match v with | .int n => fun j => if j = 0 then w j + n else w j | _ => w
  reply := fun _ i => if i = 2 then .done else .unknown
  applied := fun _ => true

/-- O provedor que nunca responde, depois de aplicar o efeito: a verdade oculta é "aplicado". -/
def S' : Sig := { S with reply := fun _ _ => .unknown }

def c : Cfg := ⟨fun _ _ _ _ => .unit, fun _ => 100, fun _ => [0]⟩

def s₀ : St := ⟨[], 0, 0, fun _ => 0, []⟩

/-- O tratador canônico: `reconcile p fuel 3 else { transfer p; () }`. -/
def handler : Exp := .let_ (.transfer 0) (.val .unit)

/-- `TimeoutUnknown`, uma consulta inconclusiva, a confirmação: `Done`, e o efeito acontece. -/
example : (eval S c (.call 0 0 (.val (.int 5)) 3 handler) [.cap 7] s₀).map (·.tr) =
    .ok [.pending 0 7 (.int 5), .query 7 1 (some .unknown), .query 7 2 (some .done),
      .settle 7 .done] := rfl

/-- Sem resposta: três consultas inconclusivas, a entrega e a transferência; o evento é
encerrado pelo tratador. -/
example : (match eval S' c (.call 0 0 (.val (.int 5)) 3 handler) [.cap 7] s₀ with
    | .error (.timeout (.abort _) _ tr) => some tr | _ => none) =
    some [.pending 0 7 (.int 5), .query 7 1 (some .unknown), .query 7 2 (some .unknown),
      .query 7 3 (some .unknown), .handoff 7, .settle 7 .transferred] := rfl

/-- **`Transferred` não é "desfeito".** A chave terminou em `Transferred` e, mesmo assim, o
efeito existe no mundo: a célula 0 vale 5. -/
theorem transferred_but_applied :
    ∃ s₁, eval S' c (.call 0 0 (.val (.int 5)) 3 handler) [.cap 7] s₀ =
      .error (.timeout (.abort .unit) s₁ [.pending 0 7 (.int 5), .query 7 1 (some .unknown),
        .query 7 2 (some .unknown), .query 7 3 (some .unknown), .handoff 7, .settle 7 .transferred]) ∧
      s₁.w 0 = 5 :=
  ⟨_, rfl, rfl⟩

/-- O tratador que não transfere é recusado pela guarda: erro de linearidade, não silêncio. -/
example : (match eval S' c (.call 0 0 (.val (.int 5)) 3 (.val .unit)) [.cap 7] s₀ with
    | .error .reuse => true | _ => false) = true := rfl

end KeysExample

end Aletheia
