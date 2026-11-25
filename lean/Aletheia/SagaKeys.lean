import Aletheia.Saga
import Aletheia.Keys

/-!
# T6 com chaves e o registro depois da reconciliação

Rev12 provou o T6 com chamadas com chave (`T6_keys`): uma operação sem resposta entra no
registro da saga antes de se saber se o efeito existe, e o compensador dela precisa ser total
sobre "aconteceu" e "não aconteceu". Faltava um caso: a reconciliação que **prova** a ausência do
efeito (`Failed`, o `R0611` do ARX). Até a Rev12 a operação continuava no registro e a saga a
estornava mesmo assim — um efeito novo, sem causa, cobrado de um compensador que talvez não seja
total (`KeyedSagaExample.refuted_was_compensated`).

Agora (`recLog`, em `Eval.lean`) a operação refutada sai do registro. O enunciado acompanha:
as operações que a saga compensa são as emitidas **menos as refutadas** (`opsOf`). Como a
refutação aparece no traço depois da emissão, `opsOf` olha o traço inteiro; para que ele se
componha em sequência é preciso que as chaves de dois trechos sejam disjuntas, e é o que o
`KeysOk` do T9 garante (uma chave gasta num trecho não aparece no seguinte). Por isso o registro
(`LoggedK`) carrega o `KeysOk`.

A hipótese nova é sobre o provedor e é a metade de `Honest` que o registro usa (`Refutes`): uma
resposta `Failed` da reconciliação só vem de uma operação não aplicada. Sem ela, o T6 com chaves
é falso: um provedor que nega um efeito que aplicou deixa o efeito sem estorno
(`KeyedSagaExample.dishonest_refusal_strands`).
-/

namespace Aletheia

section
variable {S : Sig} {c : Cfg} {E : World → World → Prop}

/-- O compensador de `t` é total: aplicado a um mundo sem o efeito, devolve um mundo
equivalente. É o que um compensador vinculado à identidade da operação garante (estornar o
pagamento da chave `k` não faz nada se ele não existe); um compensador cego (subtrair `v`)
não garante (`SagaExample.blind_compensator_corrupts`). -/
def TotalUndo (S : Sig) (E : World → World → Prop) (t : Nat) : Prop :=
  ∀ (v : Val) {w w₀ : World}, E w w₀ → ∃ w', S.undo t v w = some w' ∧ E w' w₀

/-- A chave `k` foi confirmada **não aplicada** no traço: a reconciliação fechou `Failed`. -/
def refutedIn (tr : List Ev) (k : Nat) : Bool :=
  tr.any fun e => match e with | .settle k' .failed => k' == k | _ => false

/-- A operação `compensable` de um evento: uma ação, ou uma operação com chave em aberto que
`P` não declara refutada. -/
def opsF (S : Sig) (P : Nat → Bool) : Ev → Option (Nat × Val)
  | .act t _ v => if S.compensable t then some (t, v) else none
  | .pending t k v => if S.compensable t && !P k then some (t, v) else none
  | _ => none

/-- As operações `compensable` de um traço que podem ter acontecido: as ações e as operações
com chave aplicadas na hora (`act`) ou deixadas em aberto (`pending`), **menos** as que a
reconciliação refutou no próprio traço. -/
def opsOf (S : Sig) (tr : List Ev) : List (Nat × Val) := tr.filterMap (opsF S (refutedIn tr))

theorem refutedIn_append (a b : List Ev) (k : Nat) :
    refutedIn (a ++ b) k = (refutedIn a k || refutedIn b k) := by
  simp [refutedIn, List.any_append]

theorem opsF_congr {P Q : Nat → Bool} :
    ∀ (l : List Ev), (∀ t k v, Ev.pending t k v ∈ l → P k = Q k) →
      l.filterMap (opsF S P) = l.filterMap (opsF S Q)
  | [], _ => rfl
  | e :: l, h => by
    have ih := opsF_congr l fun t k v hm => h t k v (List.mem_cons_of_mem _ hm)
    cases e with
    | pending t k v => simp only [List.filterMap_cons, opsF, h t k v (List.mem_cons_self ..), ih]
    | _ => simp only [List.filterMap_cons, opsF, ih]

theorem opsOf_append {a b : List Ev}
    (h₁ : ∀ t k v, Ev.pending t k v ∈ a → refutedIn b k = false)
    (h₂ : ∀ t k v, Ev.pending t k v ∈ b → refutedIn a k = false) :
    opsOf S (a ++ b) = opsOf S a ++ opsOf S b := by
  unfold opsOf
  rw [List.filterMap_append]
  congr 1
  · exact opsF_congr a fun t k v hm => by rw [refutedIn_append, h₁ t k v hm, Bool.or_false]
  · exact opsF_congr b fun t k v hm => by rw [refutedIn_append, h₂ t k v hm, Bool.false_or]

theorem refutedIn_mem {k : Nat} {tr : List Ev} (h : refutedIn tr k = true) : Ev.settle k .failed ∈ tr := by
  obtain ⟨e, he, hk⟩ := List.any_eq_true.mp h
  cases e with
  | settle k' r =>
    cases r with
    | failed => simp at hk; subst hk; exact he
    | _ => simp at hk
  | _ => simp at hk

theorem mem_keyEvs {k : Nat} {e : Ev} {tr : List Ev} (h : e ∈ tr) (hk : keyOf e = some k) :
    keyEvs k tr ≠ [] := by
  intro hn
  have : e ∈ keyEvs k tr := List.mem_filter.mpr ⟨h, by simp [hk]⟩
  rw [hn] at this; simp at this

/-- Duas fatias consecutivas não compartilham chave: o que a primeira gasta, a segunda já não
pode usar. -/
theorem keysOk_disjoint {s₁ s₂ s₃ : St} {t₁ t₂ : List Ev} (k₁ : KeysOk s₁ s₂ t₁) (k₂ : KeysOk s₂ s₃ t₂)
    {k : Nat} (h₁ : keyEvs k t₁ ≠ []) (h₂ : keyEvs k t₂ ≠ []) : False :=
  (k₂.2.1 k h₂).2 (k₁.2.1 k h₁).1

theorem opsOf_seq {s₁ s₂ s₃ : St} {t₁ t₂ : List Ev} (k₁ : KeysOk s₁ s₂ t₁) (k₂ : KeysOk s₂ s₃ t₂) :
    opsOf S (t₁ ++ t₂) = opsOf S t₁ ++ opsOf S t₂ :=
  opsOf_append
    (fun _ k _ hm => by
      cases hr : refutedIn t₂ k
      · rfl
      · exact (keysOk_disjoint k₁ k₂ (mem_keyEvs hm rfl) (mem_keyEvs (refutedIn_mem hr) rfl)).elim)
    (fun _ k _ hm => by
      cases hr : refutedIn t₁ k
      · rfl
      · exact (keysOk_disjoint k₁ k₂ (mem_keyEvs (refutedIn_mem hr) rfl) (mem_keyEvs hm rfl)).elim)

theorem opsF_queries (P : Nat → Bool) (t k n : Nat) : (reconcile S t k 1 n).1.filterMap (opsF S P) = [] :=
  filterMap_queries _ (fun _ _ _ => rfl) (reconcile_queries S t k 1 n)

theorem refutedIn_queries (t k n k' : Nat) : refutedIn (reconcile S t k 1 n).1 k' = false := by
  cases h : refutedIn (reconcile S t k 1 n).1 k'
  · rfl
  · obtain ⟨j, r, hq⟩ := reconcile_queries S t k 1 n _ (refutedIn_mem h); cases hq

theorem pending_not_mem_of_kevs {tr : List Ev} (h : kevs tr = []) {t k : Nat} {v : Val} :
    Ev.pending t k v ∉ tr := fun hm => mem_keyEvs hm rfl (keyEvs_of_kevs_nil h k)

theorem refutedIn_of_kevs {tr : List Ev} (h : kevs tr = []) (k : Nat) : refutedIn tr k = false := by
  cases hr : refutedIn tr k
  · rfl
  · exact (mem_keyEvs (refutedIn_mem hr) rfl (keyEvs_of_kevs_nil h k)).elim

/-- Reaplicar um registro em que cada entrada traz se o efeito foi aplicado. -/
def replayB (S : Sig) : List (Nat × Val × Bool) → World → World
  | [], w => w
  | (t, v, b) :: rest, w => if b then S.step t v (replayB S rest w) else replayB S rest w

def strip (l : List (Nat × Val × Bool)) : List (Nat × Val) := l.map fun e => (e.1, e.2.1)

theorem replayB_append (S : Sig) :
    ∀ (a b : List (Nat × Val × Bool)) (w : World), replayB S (a ++ b) w = replayB S a (replayB S b w)
  | [], _, _ => rfl
  | (t, v, x) :: a, b, w => by simp only [List.cons_append, replayB, replayB_append S a b w]

theorem replayB_congr (hR : Restoring S E) :
    ∀ (log : List (Nat × Val × Bool)) {w₁ w₂ : World}, E w₁ w₂ → E (replayB S log w₁) (replayB S log w₂)
  | [], _, _, h => h
  | (t, v, b) :: rest, _, _, h => by
    simp only [replayB]
    cases b
    · exact replayB_congr hR rest h
    · exact hR.step t v (replayB_congr hR rest h)

/-- O registro com chaves: as chaves do trecho seguem o T9 (`KeysOk`), e o que a execução
acrescentou ao registro é o que ela emitiu e não foi refutado (da mais recente à mais antiga),
cada entrada não aplicada vem de uma operação em aberto do traço, e o mundo é `E`-equivalente a
reaplicar as que foram aplicadas. -/
def LoggedK (S : Sig) (E : World → World → Prop) (s s' : St) (tr : List Ev) : Prop :=
  KeysOk s s' tr ∧
  ∃ new : List (Nat × Val × Bool), s'.comp = strip new ++ s.comp ∧ strip new = (opsOf S tr).reverse ∧
    (∀ e ∈ new, e.2.2 = false → ∃ k v, Ev.pending e.1 k v ∈ tr) ∧ E s'.w (replayB S new s.w)

/-- O provedor não nega o que fez: uma resposta `Failed` da reconciliação (tentativa ≥ 1) só
vem de uma operação não aplicada. É a metade de `Honest` que o registro usa. -/
def Refutes (S : Sig) : Prop := ∀ t k i, 1 ≤ i → S.reply t k i = .failed → S.applied t k = false

theorem Honest.refutes (h : Honest S) : Refutes S := fun t k i hi hr => (h t k i hi).2 hr

/-- Uma reconciliação que fecha `Failed` viu uma resposta `Failed` numa tentativa `≥ i`. -/
theorem reconcile_failed (S : Sig) (t k : Nat) :
    ∀ i n, (reconcile S t k i n).2 = .failed → ∃ j, i ≤ j ∧ S.reply t k j = .failed
  | _, 0, h => by simp [reconcile] at h
  | i, n + 1, h => by
    simp only [reconcile] at h
    split at h
    · simp at h
    · split at h
      · simp at h
      · rename_i hr; exact ⟨i, Nat.le_refl _, hr⟩
      · obtain ⟨j, hj, hr⟩ := reconcile_failed S t k (i + 1) n h
        exact ⟨j, by omega, hr⟩

/-- A entrada de uma operação: registrada se `compensable`, com a verdade da aplicação. -/
def entry (S : Sig) (t : Nat) (v : Val) (b : Bool) : List (Nat × Val × Bool) :=
  if S.compensable t then [(t, v, b)] else []

theorem entry_log (S : Sig) (t : Nat) (v : Val) (b : Bool) (comp : List (Nat × Val)) :
    logAct S t v comp = strip (entry S t v b) ++ comp := by
  unfold logAct entry strip; split <;> simp_all

theorem entry_world (hR : Restoring S E) (t : Nat) (v : Val) (b : Bool) (w : World) :
    E (if b then S.step t v w else w) (replayB S (entry S t v b) w) := by
  unfold entry
  by_cases hc : S.compensable t = true
  · simp only [hc, if_true, replayB]; exact hR.refl _
  · have hc' : S.compensable t = false := by simpa using hc
    simp only [hc', Bool.false_eq_true, if_false, replayB]
    cases b
    · exact hR.refl _
    · exact hR.inert t v w hc'

theorem loggedK_preserves (hR : Restoring S E) (hRef : Refutes S) :
    Preserves S c False True (LoggedK S E) where
  refl s := ⟨(keys_preserves (S := S) (c := c)).refl s, [], rfl, rfl, by simp, hR.refl _⟩
  trans := by
    rintro s₁ s₂ s₃ t₁ t₂ ⟨k₁, n₁, hc₁, ha₁, hp₁, hw₁⟩ ⟨k₂, n₂, hc₂, ha₂, hp₂, hw₂⟩
    refine ⟨(keys_preserves (S := S) (c := c)).trans k₁ k₂, n₂ ++ n₁, by rw [hc₂, hc₁]; simp [strip, List.append_assoc],
      ?_, ?_, ?_⟩
    · simp only [strip, List.map_append] at ha₁ ha₂ ⊢
      rw [opsOf_seq k₁ k₂, ha₁, ha₂]; simp
    · intro e he hb
      rcases List.mem_append.mp he with he | he
      · obtain ⟨k, v, h⟩ := hp₂ e he hb; exact ⟨k, v, List.mem_append_right _ h⟩
      · obtain ⟨k, v, h⟩ := hp₁ e he hb; exact ⟨k, v, List.mem_append_left _ h⟩
    · rw [replayB_append]; exact hR.trans hw₂ (replayB_congr hR n₂ hw₁)
  endorse h := ⟨(keys_preserves (S := S) (c := c)).endorse h, [], rfl, rfl, by simp, hR.refl _⟩
  reject := ⟨(keys_preserves (S := S) (c := c)).reject, [], rfl, rfl, by simp, hR.refl _⟩
  act := by
    intro s t id v hg hu
    refine ⟨(keys_preserves (S := S) (c := c)).act hg hu, entry S t v true, by simp [actC, entry_log S t v true], ?_,
      by simp [entry], ?_⟩
    · by_cases hc : S.compensable t = true <;> simp [entry, strip, opsOf, opsF, actEvs, hc]
    · simpa [actW] using entry_world hR t v true s.w
  drop h := ⟨(keys_preserves (S := S) (c := c)).drop h, [], rfl, rfl, by simp, hR.refl _⟩
  infer := ⟨(keys_preserves (S := S) (c := c)).infer, [], rfl, rfl, by simp, hR.refl _⟩
  commit h := h.elim
  revoke h := h.elim
  abort h := h.elim
  callDone := by
    intro _ s t id kid v hg hu hk₁ hk₂
    refine ⟨(keys_preserves (S := S) (c := c)).callDone trivial hg hu hk₁ hk₂, entry S t v true,
      by simp [callSt, entry_log S t v true], ?_, by simp [entry], ?_⟩
    · by_cases hc : S.compensable t = true <;> simp [entry, strip, opsOf, opsF, hc]
    · simpa [callSt] using entry_world hR t v true s.w
  callFailed := by
    intro _ s t id kid hg hu hk₁ hk₂
    exact ⟨(keys_preserves (S := S) (c := c)).callFailed trivial hg hu hk₁ hk₂, [], by simp [callSt0, strip],
      by simp [strip, opsOf, opsF], by simp, by simpa [callSt0, replayB] using hR.refl _⟩
  callRec := by
    intro _ s t id kid n v e hg hu hk₁ hk₂ hr hne
    refine ⟨(keys_preserves (S := S) (c := c)).callRec trivial hg hu hk₁ hk₂ hr hne, ?_⟩
    have hP : ∀ P : Nat → Bool, ((reconcile S t kid 1 n).1 ++ [Ev.settle kid e]).filterMap (opsF S P) = [] := by
      intro P; rw [List.filterMap_append, opsF_queries]; rfl
    cases e with
    | transferred => exact absurd rfl hne
    | done =>
      have href : refutedIn (.pending t kid v :: ((reconcile S t kid 1 n).1 ++ [Ev.settle kid .done])) kid = false := by
        show refutedIn ([.pending t kid v] ++ ((reconcile S t kid 1 n).1 ++ [Ev.settle kid .done])) kid = false
        rw [refutedIn_append, refutedIn_append, refutedIn_queries]; simp [refutedIn]
      refine ⟨entry S t v (S.applied t kid), by simp [recSt, recLog, entry_log S t v (S.applied t kid)], ?_, ?_, ?_⟩
      · simp only [opsOf, List.filterMap_cons, hP]
        by_cases hc : S.compensable t = true <;> simp [entry, strip, opsF, hc, href]
      · intro x hx _
        unfold entry at hx; split at hx
        · simp at hx; subst hx; exact ⟨kid, v, List.mem_cons_self ..⟩
        · simp at hx
      · simpa [recSt] using entry_world hR t v (S.applied t kid) s.w
    | failed =>
      -- a operação refutada não entra no registro; pela `Refutes`, ela não foi aplicada
      have href : refutedIn (.pending t kid v :: ((reconcile S t kid 1 n).1 ++ [Ev.settle kid .failed])) kid = true := by
        show refutedIn ([.pending t kid v] ++ ((reconcile S t kid 1 n).1 ++ [Ev.settle kid .failed])) kid = true
        rw [refutedIn_append, refutedIn_append, refutedIn_queries]; simp [refutedIn]
      have happ : S.applied t kid = false := by
        obtain ⟨j, hj, hrj⟩ := reconcile_failed S t kid 1 n hr
        exact hRef t kid j hj hrj
      refine ⟨[], by simp [recSt, recLog, strip], ?_, by simp, ?_⟩
      · simp only [opsOf, List.filterMap_cons, hP]
        simp [strip, opsF, href]
      · simp only [recSt, happ, replayB]; exact hR.refl _
  callAbort := by
    intro _ s s₃ t id kid n v tr hg hu hk₁ hk₂ hr ⟨hK, nh, hch, hah, hph, hwh⟩ hto
    refine ⟨(keys_preserves (S := S) (c := c)).callAbort trivial hg hu hk₁ hk₂ hr hK hto, ?_⟩
    have hnil := kevs_nil_of_transferOnly hto
    have href : refutedIn (.pending t kid v :: ((reconcile S t kid 1 n).1 ++ (tr ++ [Ev.settle kid .transferred]))) kid = false := by
      show refutedIn ([.pending t kid v] ++ ((reconcile S t kid 1 n).1 ++ (tr ++ [Ev.settle kid .transferred]))) kid = false
      rw [refutedIn_append, refutedIn_append, refutedIn_append, refutedIn_queries, refutedIn_of_kevs hnil]
      simp [refutedIn]
    have htr : ∀ P : Nat → Bool, tr.filterMap (opsF S P) = opsOf S tr := fun P =>
      opsF_congr tr fun _ _ _ hm => absurd hm (pending_not_mem_of_kevs hnil)
    have hops : opsOf S (.pending t kid v :: ((reconcile S t kid 1 n).1 ++ (tr ++ [Ev.settle kid .transferred]))) =
        (if S.compensable t then [(t, v)] else []) ++ opsOf S tr := by
      show List.filterMap (opsF S (refutedIn (Ev.pending t kid v :: ((reconcile S t kid 1 n).1 ++
          (tr ++ [Ev.settle kid .transferred])))))
        (Ev.pending t kid v :: ((reconcile S t kid 1 n).1 ++ (tr ++ [Ev.settle kid .transferred]))) = _
      rw [List.filterMap_cons, List.filterMap_append, List.filterMap_append, opsF_queries, htr]
      by_cases hc : S.compensable t = true <;> simp [opsF, hc, href]
    refine ⟨nh ++ entry S t v (S.applied t kid), ?_, ?_, ?_, ?_⟩
    · rw [hch]; simp [callSt, entry_log S t v (S.applied t kid), strip, List.append_assoc]
    · simp only [strip, List.map_append] at hah ⊢
      rw [hah, hops]
      by_cases hc : S.compensable t = true <;> simp [entry, hc]
    · intro x hx hb
      rcases List.mem_append.mp hx with hx | hx
      · obtain ⟨k, w, h⟩ := hph x hx hb
        exact ⟨k, w, List.mem_cons_of_mem _ (List.mem_append_right _ (List.mem_append_left _ h))⟩
      · unfold entry at hx; split at hx
        · simp at hx; subst hx; exact ⟨kid, v, List.mem_cons_self ..⟩
        · simp at hx
    · rw [replayB_append]
      exact hR.trans hwh (replayB_congr hR nh (by simpa [callSt] using entry_world hR t v (S.applied t kid) s.w))
  mint := by
    rintro s s' t hn hk ⟨hK, n, hc, ha, hp, hw⟩
    refine ⟨(keys_preserves (S := S) (c := c)).mint hn hk hK, n, hc, ?_, fun e he hb => ?_, hw⟩
    · have hm : refutedIn (Ev.mint s.next :: t) = refutedIn t := by funext k; simp [refutedIn]
      rw [ha]; simp only [opsOf, hm, List.filterMap_cons, opsF]
    · obtain ⟨k, v, h⟩ := hp e he hb
      exact ⟨k, v, List.mem_cons_of_mem _ h⟩
  transfer := ⟨(keys_preserves (S := S) (c := c)).transfer, [], rfl, rfl, by simp, hR.refl _⟩

theorem eval_loggedK (hR : Restoring S E) (hRef : Refutes S) (e : Exp) (hsf : sagaFree e = true)
    {ρ : List Val} {s : St} :
    (∀ {o : Out}, eval S c e ρ s = .ok o → LoggedK S E s o.st o.tr) ∧
    (∀ {q s₁ tr}, eval S c e ρ s = .error (.timeout q s₁ tr) → LoggedK S E s s₁ tr) :=
  eval_preserves (loggedK_preserves hR hRef) e ⟨.inr hsf, .inl trivial⟩

/-- Compensar um registro com entradas aplicadas e não aplicadas restaura, se o compensador de
cada entrada não aplicada for total. -/
theorem compensate_restoresK (hR : Restoring S E) :
    ∀ (log : List (Nat × Val × Bool)) {w w₀ : World}, E w (replayB S log w₀) →
      (∀ e ∈ log, e.2.2 = false → TotalUndo S E e.1) →
      (compensate S (strip log) w).ok = true ∧ E (compensate S (strip log) w).w w₀
  | [], _, _, h, _ => ⟨rfl, h⟩
  | (t, v, b) :: rest, _, _, h, ht => by
    have ht' : ∀ e ∈ rest, e.2.2 = false → TotalUndo S E e.1 :=
      fun e he => ht e (List.mem_cons_of_mem _ he)
    cases b with
    | true =>
      obtain ⟨w', hu, hw'⟩ := hR.undo t v (by simpa [replayB] using h)
      simp only [strip, List.map_cons, compensate, hu]
      exact compensate_restoresK hR rest hw' ht'
    | false =>
      obtain ⟨w', hu, hw'⟩ := ht (t, v, false) (List.mem_cons_self ..) rfl v (by simpa [replayB] using h)
      simp only [strip, List.map_cons, compensate, hu]
      exact compensate_restoresK hR rest hw' ht'

/-- **T6 com chaves (Rev13).** Sob contratos `restoring` para `E`, com um provedor que não nega o
que fez (`Refutes`), se o corpo (sem saga aninhada, com chamadas com chave) de uma saga falha, e
toda ferramenta que ficou em aberto no traço (`Pending`) tem compensador total:

* as compensações são exatamente as operações que o corpo emitiu e a reconciliação não refutou
  (aplicadas na hora ou em aberto), cada uma uma vez, da mais recente à mais antiga: uma
  operação que a reconciliação provou ausente **não** é estornada;
* nenhuma é recusada;
* o mundo compensado é `E`-equivalente ao de antes da saga — aplicada ou não cada operação em
  aberto (`S.applied`), o que o agente não sabe. -/
theorem T6_keys (hR : Restoring S E) (hRef : Refutes S) {body : Exp} (hsf : sagaFree body = true)
    {ρ : List Val} {s : St} {q : Cause} {s₁ : St} {tr : List Ev}
    (hb : eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr))
    (htot : ∀ t k v, Ev.pending t k v ∈ tr → TotalUndo S E t) :
    undoTargets (compensate S s₁.comp s₁.w).evs = (opsOf S tr).reverse ∧
      (compensate S s₁.comp s₁.w).ok = true ∧ E (compensate S s₁.comp s₁.w).w s.w := by
  obtain ⟨_, new, hc, ha, hp, hw⟩ := (eval_loggedK hR hRef body hsf).2 hb
  simp only [List.append_nil] at hc
  rw [hc]
  refine ⟨by rw [compensate_targets, ha], compensate_restoresK hR new hw fun e he hb' => ?_⟩
  obtain ⟨k, v, h⟩ := hp e he hb'
  exact htot e.1 k v h

/-- A operação de uma chave refutada no traço não é alvo de compensação, qualquer que seja a
ferramenta. -/
theorem refuted_excluded {tr : List Ev} {t k : Nat} {v : Val} (hr : refutedIn tr k = true) :
    opsF S (refutedIn tr) (.pending t k v) = none := by
  simp [opsF, hr]

/-- O T6 com chaves contém o T6: sem chamada com chave não há `Pending` e as operações são as
ações. -/
theorem opsOf_keyless {tr : List Ev} (h : ∀ t k v, Ev.pending t k v ∉ tr) : opsOf S tr = actsOf S tr := by
  unfold opsOf
  induction tr with
  | nil => rfl
  | cons e l ih =>
    have hl : ∀ t k v, Ev.pending t k v ∉ l := fun t k v hm => h t k v (List.mem_cons_of_mem _ hm)
    have ih' := ih hl
    have hc : l.filterMap (opsF S (refutedIn (e :: l))) = l.filterMap (opsF S (refutedIn l)) :=
      opsF_congr l fun t k v hm => absurd hm (hl t k v)
    cases e with
    | pending t k v => exact absurd (List.mem_cons_self ..) (h t k v)
    | act t id v =>
      unfold actsOf at ih' ⊢
      simp only [List.filterMap_cons, hc, ih', opsF]
    | _ =>
      unfold actsOf at ih' ⊢
      simp only [List.filterMap_cons, hc, ih', opsF]

end

/-! ## Exemplo: compensar uma operação em aberto -/

namespace KeyedSagaExample

/-- A ferramenta 0 soma `n` na célula 0 (um pagamento); o provedor nunca responde e o efeito
**não** foi aplicado (`applied = false`). -/
def S : Sig where
  cost := fun _ _ => 0
  mcost := fun _ _ => 0
  arg := fun _ => .int
  effect := fun _ => 0
  pol := fun _ _ => true
  step := fun _ v w => match v with | .int n => fun i => if i = 0 then w 0 + n else w i | _ => w
  -- o compensador cego: estorna `n` sem saber se o pagamento existe
  undo := fun _ v w => match v with | .int n => some (fun i => if i = 0 then w 0 - n else w i) | _ => some w
  reply := fun _ _ _ => .unknown
  applied := fun _ _ => false

def c : Cfg := ⟨fun _ _ _ _ => .unit, fun _ => 100, fun _ => [0]⟩

def s₀ : St := ⟨[], fun _ => 0, 0, fun _ => 0, [], 10, []⟩

/-- `atomic { pay(c, 5, keys.mint()) handle timeout_unknown(p) { reconcile p fuel 1 else
{ transfer p; () } } }`: a operação fica em aberto, a obrigação é entregue, a saga compensa. -/
def saga : Exp :=
  .atomic (.mint (.call 0 1 0 (.val (.int 5)) 1 (.let_ (.transfer 0) (.val .unit)))) 0 (.val .unit) (.val .unit)

/-- **O compensador cego corrompe.** O pagamento nunca aconteceu, a saga o estorna mesmo assim:
o mundo termina em −5, não em 0. É o que `TotalUndo` exclui; sem ele, o T6 com chaves é falso. -/
theorem blind_compensator_corrupts :
    (match eval S c saga [.cap 7] s₀ with | .error (.timeout _ s _) => some (s.w 0) | _ => none) = some (-5) :=
  rfl

/-- O compensador vinculado à operação: a célula 1 registra se o pagamento existe, e o estorno
só age se existir. -/
def Sid (b : Bool) : Sig :=
  { S with
    step := fun _ v w => match v with
      | .int n => fun i => if i = 0 then w 0 + n else if i = 1 then 1 else w i
      | _ => w
    undo := fun _ v w => match v with
      | .int n => some (if w 1 = 1 then fun i => if i = 0 then w 0 - n else if i = 1 then 0 else w i else w)
      | _ => some w
    applied := fun _ _ => b }

/-- **Com o compensador vinculado, a saga restaura nos dois mundos**: aplicado ou não, o que o
agente não sabe. -/
theorem bound_compensator_restores :
    (match eval (Sid false) c saga [.cap 7] s₀ with | .error (.timeout _ s _) => some (s.w 0, s.w 1) | _ => none) =
      some (0, 0) ∧
    (match eval (Sid true) c saga [.cap 7] s₀ with | .error (.timeout _ s _) => some (s.w 0, s.w 1) | _ => none) =
      some (0, 0) := ⟨rfl, rfl⟩

/-- O provedor que responde à reconciliação: sem resposta na chamada, `Failed` na consulta. -/
def Sref (b : Bool) : Sig :=
  { S with reply := fun _ _ i => if i = 0 then .unknown else .failed, applied := fun _ _ => b }

/-- `atomic { pay(c, 5, keys.mint()) handle timeout_unknown(p) { reconcile p fuel 1 else
{ transfer p; () } } } recover q0 else ()`: a reconciliação refuta o pagamento (`R0611`). -/
def sagaR : Exp :=
  .atomic (.mint (.call 0 1 0 (.val (.int 5)) 1 (.let_ (.transfer 0) (.val .unit)))) 0 (.val .unit) (.val .unit)

/-- **A operação refutada não é estornada (`refuted_not_compensated`).** Com o mesmo compensador
cego de `blind_compensator_corrupts`, a reconciliação prova que o pagamento não existe; a
operação sai do registro, a saga não compensa nada e o mundo termina em 0. Na Rev12 ela ficava
no registro e o estorno levava o mundo a −5. -/
theorem refuted_not_compensated :
    (match eval (Sref false) c sagaR [.cap 7] s₀ with
      | .ok o => some (o.st.w 0, undoTargets o.tr) | _ => none) = some (0, []) := rfl

/-- **Sem `Refutes`, o efeito fica sem estorno (`dishonest_refusal_strands`).** Um provedor que
responde `Failed` a um pagamento que aplicou: a operação sai do registro (a resposta a refutou),
nada é compensado e o mundo termina em 5. A hipótese `Refutes` do `T6_keys` é necessária. -/
theorem dishonest_refusal_strands :
    (match eval (Sref true) c sagaR [.cap 7] s₀ with
      | .ok o => some (o.st.w 0, undoTargets o.tr) | _ => none) = some (5, []) := rfl

end KeyedSagaExample

end Aletheia
