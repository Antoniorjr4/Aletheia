import Aletheia.Eval

/-!
# Sagas no núcleo: o Teorema T6

As sagas estão no avaliador canônico (`Eval.lean`), o mesmo de T1–T3:

* `await q`: a pós-condição observada pela sonda confiável `q`; se ela não confirma, a saga
  corrente falha com `timeout`, que carrega o estado e os eventos até ali;
* `atomic body r f g`: a saga. Ela roda o corpo com um registro de compensação novo. Se o
  corpo termina, os efeitos ficam (`commit`). Se falha, as compensações do registro rodam da
  ação mais recente para a mais antiga, como obrigação e sem reserva de orçamento. Uma
  compensação recusada pelo provedor não interrompe as seguintes. Depois roda a sonda de
  recuperação `r`. `f` (`on failure`) roda se todas as compensações deram certo e a sonda
  confirma; `g` (`on compensation_failed`) roda em qualquer outro caso.

O mundo é abstrato: as transições das ferramentas (`Sig.step`), as compensações
(`Sig.undo`, que podem falhar) e as sondas (`Sig.probe`) são funções arbitrárias da
assinatura. O T6 tem duas partes.

1. **Sem mascaramento** (`T6_no_masking`, `T6_recovered`), para qualquer mundo e qualquer
   compensação: o tratador `on failure` só roda se todas as compensações deram certo e a
   sonda observou o invariante no mundo compensado. Com uma sonda correta, a recuperação
   notificada implica o invariante. Uma falha nunca vira sucesso em silêncio: ou o
   invariante foi observado, ou o programa está em `CompensationFailed`.
2. **Restauração composicional** (`T6`, `T6_saga`), sob contratos `restoring`: se cada
   compensação desfaz a sua ação a menos de uma equivalência `E` do mundo, então uma saga
   cujo corpo falha compensa **exatamente** as ações que o corpo executou, cada uma uma vez
   e na ordem reversa. Todas as compensações dão certo, e o mundo entregue ao tratador é
   `E`-equivalente ao mundo de antes da saga. Todo invariante que `E` respeita volta a
   valer, e a escolha entre `on failure` e `CompensationFailed` passa a depender só da
   sonda.

A parte 2 vale para corpos sem saga aninhada (`sagaFree`). O motivo está no ARX: uma saga
interna que confirma descarta o seu registro, e os efeitos dela ficam fora das compensações
da saga externa. A semântica formal reproduz essa escolha, e o teorema a torna explícita.
-/

namespace Aletheia

section
variable {S : Sig} {c : Cfg}

/-! ## O registro de compensação -/

/-- O mundo depois de reaplicar um registro (o mais recente primeiro) a partir de `w`. -/
def replay (S : Sig) : List (Nat × Val) → World → World
  | [], w => w
  | (t, v) :: rest, w => S.step t v (replay S rest w)

theorem replay_append (S : Sig) :
    ∀ (a b : List (Nat × Val)) (w : World), replay S (a ++ b) w = replay S a (replay S b w)
  | [], _, _ => rfl
  | (t, v) :: a, b, w => by simp only [List.cons_append, replay, replay_append S a b w]

/-- As ações `compensable` de um traço, em ordem: `(ferramenta, argumento)`. -/
def actsOf (S : Sig) (tr : List Ev) : List (Nat × Val) :=
  tr.filterMap fun
    | .act t _ v => if S.compensable t then some (t, v) else none
    | _ => none

/-- As compensações de uma lista de eventos, em ordem. -/
def undoTargets (evs : List Ev) : List (Nat × Val) :=
  evs.filterMap fun
    | .undo t v _ => some (t, v)
    | _ => none

/-- As compensações visitam o registro inteiro, da ação mais recente à mais antiga, uma vez
cada. -/
theorem compensate_targets (S : Sig) :
    ∀ (log : List (Nat × Val)) (w : World), undoTargets (compensate S log w).evs = log
  | [], _ => rfl
  | (t, v) :: rest, w => by
    simp only [compensate]
    split <;> simp [undoTargets, List.filterMap_cons] <;>
      exact compensate_targets S rest _

/-- `ok` diz exatamente que nenhuma compensação foi recusada. -/
theorem compensate_ok_iff (S : Sig) :
    ∀ (log : List (Nat × Val)) (w : World),
      (compensate S log w).ok = true ↔ ∀ t v, Ev.undo t v false ∉ (compensate S log w).evs
  | [], _ => by simp [compensate]
  | (t, v) :: rest, w => by
    simp only [compensate]
    split
    · simp only [List.mem_cons, Ev.undo.injEq, Bool.false_eq_true, and_false, false_or]
      exact compensate_ok_iff S rest _
    · constructor
      · intro h; cases h
      · intro h; exact (h t v List.mem_cons_self).elim

/-! ## T6, parte 1: sem mascaramento -/

/-- O caminho de falha de uma saga, por extenso: o corpo falhou, as compensações rodaram, a
sonda observou o mundo compensado, e o tratador foi escolhido por `ok && sonda`. O traço
registra o corpo até a falha, a falha, cada compensação, a observação da sonda e o
tratador. -/
theorem atomic_failed {body : Exp} {r : Nat} {f g : Exp} {ρ : List Val} {s : St} {o : Out}
    {q : Cause} {s₁ : St} {tr : List Ev}
    (hb : eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr))
    (h : eval S c (.atomic body r f g) ρ s = .ok o) :
    ∃ o', eval S c (cond ((compensate S s₁.comp s₁.w).ok && S.probe r (compensate S s₁.comp s₁.w).w) f g)
        ρ { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp } = .ok o' ∧
      o = ⟨o'.v, exitSaga ρ (.atomic body r f g) o'.st, tr ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++
        [.recover r (S.probe r (compensate S s₁.comp s₁.w).w)] ++ o'.tr⟩ := by
  simp only [eval, hb] at h
  split at h
  · simp [pre] at h
  obtain ⟨o', h', h⟩ := bnd_eq_ok.mp (pre_eq_ok.mp h)
  cases h
  exact ⟨o', by rw [← eval_cond]; exact h', rfl⟩

/-- **T6, sem mascaramento.** Para qualquer mundo, qualquer ferramenta e qualquer
compensação: se o corpo de uma saga falha, `on failure` roda exatamente quando todas as
compensações deram certo e a sonda de recuperação confirmou; em qualquer outro caso roda
`on compensation_failed`. -/
theorem T6_no_masking {body : Exp} {r : Nat} {f g : Exp} {ρ : List Val} {s : St} {o : Out}
    {q : Cause} {s₁ : St} {tr : List Ev}
    (hb : eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr))
    (h : eval S c (.atomic body r f g) ρ s = .ok o) :
    ∃ o', o.tr = tr ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++
        [.recover r (S.probe r (compensate S s₁.comp s₁.w).w)] ++ o'.tr ∧
      (((compensate S s₁.comp s₁.w).ok = true ∧ S.probe r (compensate S s₁.comp s₁.w).w = true ∧
          eval S c f ρ { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp } = .ok o') ∨
        (((compensate S s₁.comp s₁.w).ok = false ∨ S.probe r (compensate S s₁.comp s₁.w).w = false) ∧
          eval S c g ρ { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp } = .ok o')) := by
  obtain ⟨o', he, rfl⟩ := atomic_failed hb h
  refine ⟨o', rfl, ?_⟩
  cases hk : (compensate S s₁.comp s₁.w).ok <;> cases hp : S.probe r (compensate S s₁.comp s₁.w).w <;>
    simp only [hk, hp, Bool.and_true, Bool.and_false, Bool.true_and, Bool.false_and, cond_true,
      cond_false] at he <;> simp [he]

/-- **T6, recuperação notificada é verificada.** Com uma sonda correta para o invariante
`Inv`, o caminho de falha de uma saga termina de um de dois jeitos. Ou nenhuma compensação foi
recusada, o mundo entregue ao tratador satisfaz `Inv` e o tratador é `on failure`. Ou alguma
compensação foi recusada ou a sonda não confirmou, e o tratador é `on compensation_failed`. -/
theorem T6_recovered {Inv : World → Prop} {body : Exp} {r : Nat} {f g : Exp} {ρ : List Val} {s : St}
    {o : Out} {q : Cause} {s₁ : St} {tr : List Ev}
    (hsound : ∀ w, S.probe r w = true → Inv w)
    (hb : eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr))
    (h : eval S c (.atomic body r f g) ρ s = .ok o) :
    ∃ o', o.tr = tr ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++
        [.recover r (S.probe r (compensate S s₁.comp s₁.w).w)] ++ o'.tr ∧
      (((∀ t v, Ev.undo t v false ∉ (compensate S s₁.comp s₁.w).evs) ∧ Inv (compensate S s₁.comp s₁.w).w ∧
          eval S c f ρ { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp } = .ok o') ∨
        (((compensate S s₁.comp s₁.w).ok = false ∨ S.probe r (compensate S s₁.comp s₁.w).w = false) ∧
          eval S c g ρ { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp } = .ok o')) := by
  obtain ⟨o', htr, hok | hcf⟩ := T6_no_masking hb h
  · obtain ⟨hk, hp, hf⟩ := hok
    exact ⟨o', htr, .inl ⟨(compensate_ok_iff S _ _).mp hk, hsound _ hp, hf⟩⟩
  · exact ⟨o', htr, .inr hcf⟩

/-! ## T6, parte 2: restauração sob contratos `restoring` -/

/-- O contrato `restoring` das compensações, relativo a uma equivalência `E` do mundo (a
igualdade, ou a igualdade a menos do que a saga não precisa restaurar). As transições
respeitam `E`, e a compensação de `t(v)`, aplicada a qualquer mundo equivalente ao
resultado de `t(v)` sobre `w₀`, tem sucesso e devolve um mundo equivalente a `w₀`. -/
structure Restoring (S : Sig) (E : World → World → Prop) : Prop where
  refl  : ∀ w, E w w
  trans : ∀ {a b d : World}, E a b → E b d → E a d
  step  : ∀ (t : Nat) (v : Val) {w₁ w₂ : World}, E w₁ w₂ → E (S.step t v w₁) (S.step t v w₂)
  undo  : ∀ (t : Nat) (v : Val) {w w₀ : World}, E w (S.step t v w₀) →
    ∃ w', S.undo t v w = some w' ∧ E w' w₀
  /-- o que não é `compensable` (leituras, reversíveis) não muda o que a saga restaura -/
  inert : ∀ (t : Nat) (v : Val) (w : World), S.compensable t = false → E (S.step t v w) w

variable {E : World → World → Prop}

theorem replay_congr (hR : Restoring S E) :
    ∀ (log : List (Nat × Val)) {w₁ w₂ : World}, E w₁ w₂ → E (replay S log w₁) (replay S log w₂)
  | [], _, _, h => h
  | (t, v) :: rest, _, _, h => hR.step t v (replay_congr hR rest h)

/-- O registro acompanha o mundo: o que a execução acrescentou ao registro são as suas
ações, da mais recente à mais antiga, e o mundo é `E`-equivalente a reaplicá-las sobre o
mundo de partida. -/
def Logged (S : Sig) (E : World → World → Prop) (s s' : St) (tr : List Ev) : Prop :=
  ∃ new, s'.comp = new ++ s.comp ∧ new = (actsOf S tr).reverse ∧ E s'.w (replay S new s.w)

theorem logged_preserves (hR : Restoring S E) : Preserves S c False False (Logged S E) where
  refl _ := ⟨[], rfl, rfl, hR.refl _⟩
  trans := by
    rintro s₁ s₂ s₃ t₁ t₂ ⟨n₁, hc₁, ha₁, hw₁⟩ ⟨n₂, hc₂, ha₂, hw₂⟩
    refine ⟨n₂ ++ n₁, by rw [hc₂, hc₁, List.append_assoc], ?_, ?_⟩
    · simp [actsOf, List.filterMap_append, ha₁, ha₂]
    · rw [replay_append]
      exact hR.trans hw₂ (replay_congr hR n₂ hw₁)
  endorse _ := ⟨[], rfl, rfl, hR.refl _⟩
  reject := ⟨[], rfl, rfl, hR.refl _⟩
  act := by
    intro s t id v _ _
    have hacts : actsOf S (actEvs S t id v) = if S.compensable t then [(t, v)] else [] := by
      by_cases hc : S.compensable t = true <;> simp [actsOf, actEvs, hc]
    by_cases hc : S.compensable t = true
    · exact ⟨[(t, v)], by simp [actC, logAct, hc], by simp [hacts, hc],
        by simp [replay, actW]; exact hR.refl _⟩
    · have hc' : S.compensable t = false := by simpa using hc
      exact ⟨[], by simp [actC, logAct, hc'], by simp [hacts, hc'],
        by simpa [replay, actW] using hR.inert t v s.w hc'⟩
  drop _ := ⟨[], rfl, rfl, hR.refl _⟩
  infer := ⟨[], rfl, rfl, hR.refl _⟩
  commit h := h.elim
  revoke h := h.elim
  abort h := h.elim
  callDone h := h.elim
  callFailed h := h.elim
  callRec h := h.elim
  callAbort h := h.elim
  mint := by
    rintro s s' t _ _ ⟨n, hc, ha, hw⟩
    exact ⟨n, hc, by simpa [actsOf] using ha, hw⟩
  transfer := ⟨[], rfl, rfl, hR.refl _⟩

/-- O registro acompanha o mundo em toda execução sem saga aninhada e sem chamada com
chave, no sucesso e na falha. Uma operação com chave que ficou sem resposta entra no registro
mesmo que o efeito não exista (compensar é total sobre "aconteceu" e "não aconteceu"), então o
registro deixa de ser a lista do que foi aplicado: por isso o fragmento exclui `call`. -/
theorem eval_logged (hR : Restoring S E) (e : Exp) (hsf : sagaFree e = true) (hk : keyless e = true)
    {ρ : List Val} {s : St} :
    (∀ {o : Out}, eval S c e ρ s = .ok o → Logged S E s o.st o.tr) ∧
    (∀ {q s₁ tr}, eval S c e ρ s = .error (.timeout q s₁ tr) → Logged S E s s₁ tr) :=
  eval_preserves (logged_preserves hR) e ⟨.inr hsf, .inr hk⟩

/-- Sob `restoring`, compensar um registro a partir de um mundo equivalente à sua
reaplicação sobre `w₀` dá certo em todos os passos e volta a um mundo equivalente a `w₀`. -/
theorem compensate_restores (hR : Restoring S E) :
    ∀ (log : List (Nat × Val)) {w w₀ : World}, E w (replay S log w₀) →
      (compensate S log w).ok = true ∧ E (compensate S log w).w w₀
  | [], _, _, h => ⟨rfl, h⟩
  | (t, v) :: rest, _, _, h => by
    obtain ⟨w', hu, hw'⟩ := hR.undo t v h
    simp only [compensate, hu]
    exact compensate_restores hR rest hw'

/-- **T6, restauração.** Sob contratos `restoring` para `E`, se o corpo (sem saga aninhada)
de uma saga falha:

* as compensações são exatamente as ações que o corpo executou, cada uma uma vez, da mais
  recente à mais antiga;
* nenhuma é recusada;
* o mundo compensado é `E`-equivalente ao mundo de antes da saga. -/
theorem T6 (hR : Restoring S E) {body : Exp} (hsf : sagaFree body = true) (hk : keyless body = true)
    {ρ : List Val} {s : St} {q : Cause} {s₁ : St} {tr : List Ev}
    (hb : eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr)) :
    undoTargets (compensate S s₁.comp s₁.w).evs = (actsOf S tr).reverse ∧
      (compensate S s₁.comp s₁.w).ok = true ∧ E (compensate S s₁.comp s₁.w).w s.w := by
  obtain ⟨new, hc, ha, hw⟩ := (eval_logged hR body hsf hk).2 hb
  simp only [List.append_nil] at hc
  rw [hc]
  exact ⟨by rw [compensate_targets, ha], compensate_restores hR new hw⟩

/-- **T6 para a saga inteira.** Sob contratos `restoring` para `E` e um invariante `Inv` que
`E` respeita e que vale antes da saga, toda saga (com corpo sem saga aninhada) que termina:

* confirma, com o resultado e os efeitos do corpo; ou
* falha no corpo, compensa exatamente o que o corpo fez, sem recusa, e entrega ao tratador
  um mundo que satisfaz `Inv`. O tratador é `on failure` se a sonda confirma e
  `CompensationFailed` se não: só a sonda decide. -/
theorem T6_saga {Inv : World → Prop} (hR : Restoring S E) (hInv : ∀ w w', E w w' → Inv w' → Inv w)
    {body : Exp} {r : Nat} {f g : Exp} (hsf : sagaFree body = true) (hk : keyless body = true)
    {ρ : List Val} {s : St} {o : Out}
    (hs : Inv s.w) (h : eval S c (.atomic body r f g) ρ s = .ok o) :
    (∃ o₁, eval S c body ρ { s with comp := [] } = .ok o₁ ∧
        o = ⟨o₁.v, exitSaga ρ (.atomic body r f g) { o₁.st with comp := s.comp }, o₁.tr⟩) ∨
    (∃ q s₁ tr o', eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr) ∧
        undoTargets (compensate S s₁.comp s₁.w).evs = (actsOf S tr).reverse ∧
        (compensate S s₁.comp s₁.w).ok = true ∧ Inv (compensate S s₁.comp s₁.w).w ∧
        eval S c (cond (S.probe r (compensate S s₁.comp s₁.w).w) f g) ρ
          { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp } = .ok o' ∧
        o.tr = tr ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++
          [.recover r (S.probe r (compensate S s₁.comp s₁.w).w)] ++ o'.tr) := by
  cases hb : eval S c body ρ { s with comp := [] } with
  | ok o₁ =>
    simp only [eval, hb] at h
    cases h
    exact .inl ⟨o₁, rfl, rfl⟩
  | error err =>
    cases err with
    | timeout q s₁ tr =>
      obtain ⟨htg, hok, hw⟩ := T6 hR hsf hk hb
      obtain ⟨o', he, rfl⟩ := atomic_failed hb h
      rw [hok, Bool.true_and] at he
      exact .inr ⟨q, s₁, tr, o', rfl, htg, hok, hInv _ _ hw hs, he, rfl⟩
    | _ => simp [eval, hb] at h

/-! ## T6 com chaves (Rev12): sem a hipótese `keyless`

Uma operação com chave sem resposta entra no registro antes de se saber se o efeito existe
(`callSt` registra; o mundo muda só se `applied`). O registro deixa de ser a lista do que foi
aplicado: é a lista do que foi **emitido**, cada entrada com a verdade oculta da sua aplicação.
O contrato que fecha a conta é o que o ARX enuncia num comentário ("compensar é total sobre
aconteceu e não aconteceu"): o compensador de uma operação em aberto, aplicado sem o efeito, não
muda o que a saga restaura. Aqui ele vira uma hipótese explícita sobre o provedor, cobrada só das
ferramentas que de fato ficaram em aberto no traço. -/

/-- O compensador de `t` é total: aplicado a um mundo sem o efeito, devolve um mundo
equivalente. É o que um compensador vinculado à identidade da operação garante (estornar o
pagamento da chave `k` não faz nada se ele não existe); um compensador cego (subtrair `v`)
não garante (`SagaExample.blind_compensator_corrupts`). -/
def TotalUndo (S : Sig) (E : World → World → Prop) (t : Nat) : Prop :=
  ∀ (v : Val) {w w₀ : World}, E w w₀ → ∃ w', S.undo t v w = some w' ∧ E w' w₀

/-- As operações `compensable` emitidas num traço: as ações e as operações com chave, aplicadas
na hora (`act`) ou deixadas em aberto (`pending`). -/
def opsOf (S : Sig) (tr : List Ev) : List (Nat × Val) :=
  tr.filterMap fun
    | .act t _ v => if S.compensable t then some (t, v) else none
    | .pending t _ v => if S.compensable t then some (t, v) else none
    | _ => none

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

/-- O registro com chaves: o que a execução acrescentou é o que ela emitiu (da mais recente à
mais antiga), cada entrada não aplicada vem de uma operação em aberto do traço, e o mundo é
`E`-equivalente a reaplicar as que foram aplicadas. -/
def LoggedK (S : Sig) (E : World → World → Prop) (s s' : St) (tr : List Ev) : Prop :=
  ∃ new : List (Nat × Val × Bool), s'.comp = strip new ++ s.comp ∧ strip new = (opsOf S tr).reverse ∧
    (∀ e ∈ new, e.2.2 = false → ∃ k v, Ev.pending e.1 k v ∈ tr) ∧ E s'.w (replayB S new s.w)

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

theorem loggedK_preserves (hR : Restoring S E) : Preserves S c False True (LoggedK S E) where
  refl _ := ⟨[], rfl, rfl, by simp, hR.refl _⟩
  trans := by
    rintro s₁ s₂ s₃ t₁ t₂ ⟨n₁, hc₁, ha₁, hp₁, hw₁⟩ ⟨n₂, hc₂, ha₂, hp₂, hw₂⟩
    refine ⟨n₂ ++ n₁, by rw [hc₂, hc₁]; simp [strip, List.append_assoc], ?_, ?_, ?_⟩
    · simp only [strip, List.map_append] at ha₁ ha₂ ⊢
      simp [opsOf, List.filterMap_append, ha₁, ha₂]
    · intro e he hb
      rcases List.mem_append.mp he with he | he
      · obtain ⟨k, v, h⟩ := hp₂ e he hb; exact ⟨k, v, List.mem_append_right _ h⟩
      · obtain ⟨k, v, h⟩ := hp₁ e he hb; exact ⟨k, v, List.mem_append_left _ h⟩
    · rw [replayB_append]; exact hR.trans hw₂ (replayB_congr hR n₂ hw₁)
  endorse _ := ⟨[], rfl, rfl, by simp, hR.refl _⟩
  reject := ⟨[], rfl, rfl, by simp, hR.refl _⟩
  act := by
    intro s t id v _ _
    refine ⟨entry S t v true, by simp [actC, entry_log S t v true], ?_, by simp [entry], ?_⟩
    · by_cases hc : S.compensable t = true <;> simp [entry, strip, opsOf, actEvs, hc]
    · simpa [actW] using entry_world hR t v true s.w
  drop _ := ⟨[], rfl, rfl, by simp, hR.refl _⟩
  infer := ⟨[], rfl, rfl, by simp, hR.refl _⟩
  commit h := h.elim
  revoke h := h.elim
  abort h := h.elim
  callDone := by
    intro _ s t id kid v _ _ _ _
    refine ⟨entry S t v true, by simp [callSt, entry_log S t v true], ?_, by simp [entry], ?_⟩
    · by_cases hc : S.compensable t = true <;> simp [entry, strip, opsOf, hc]
    · simpa [callSt] using entry_world hR t v true s.w
  callFailed := by
    intro _ s t id kid _ _ _ _
    exact ⟨[], by simp [callSt0, strip], by simp [strip, opsOf], by simp, by simpa [callSt0, replayB] using hR.refl _⟩
  callRec := by
    intro _ s t id kid n v e _ _ _ _ _ _
    have hq := reconcile_queries S t kid 1 n
    have hops : opsOf S (reconcile S t kid 1 n).1 = [] := by
      unfold opsOf; rw [List.filterMap_eq_nil_iff]
      intro x hx; obtain ⟨j, r, rfl⟩ := hq x hx; rfl
    refine ⟨entry S t v (S.applied t kid), by simp [callSt, entry_log S t v (S.applied t kid)], ?_, ?_, ?_⟩
    · by_cases hc : S.compensable t = true <;>
        simp [entry, strip, opsOf, List.filterMap_append, hc] <;> simpa [opsOf] using hops
    · intro x hx _
      unfold entry at hx; split at hx
      · simp at hx; subst hx; exact ⟨kid, v, List.mem_cons_self ..⟩
      · simp at hx
    · simpa [callSt] using entry_world hR t v (S.applied t kid) s.w
  callAbort := by
    intro _ s s₃ t id kid n v tr _ _ _ _ _ ⟨nh, hch, hah, hph, hwh⟩ _
    have hq := reconcile_queries S t kid 1 n
    have hops : opsOf S (reconcile S t kid 1 n).1 = [] := by
      unfold opsOf; rw [List.filterMap_eq_nil_iff]
      intro x hx; obtain ⟨j, r, rfl⟩ := hq x hx; rfl
    refine ⟨nh ++ entry S t v (S.applied t kid), ?_, ?_, ?_, ?_⟩
    · rw [hch]; simp [callSt, entry_log S t v (S.applied t kid), strip, List.append_assoc]
    · simp only [strip, List.map_append] at hah ⊢
      rw [hah]
      by_cases hc : S.compensable t = true <;>
        simp [entry, opsOf, List.filterMap_append, hc] <;> simpa [opsOf] using hops
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
    rintro s s' t _ _ ⟨n, hc, ha, hp, hw⟩
    refine ⟨n, hc, by simpa [opsOf] using ha, fun e he hb => ?_, hw⟩
    obtain ⟨k, v, h⟩ := hp e he hb
    exact ⟨k, v, List.mem_cons_of_mem _ h⟩
  transfer := ⟨[], rfl, rfl, by simp, hR.refl _⟩

theorem eval_loggedK (hR : Restoring S E) (e : Exp) (hsf : sagaFree e = true) {ρ : List Val} {s : St} :
    (∀ {o : Out}, eval S c e ρ s = .ok o → LoggedK S E s o.st o.tr) ∧
    (∀ {q s₁ tr}, eval S c e ρ s = .error (.timeout q s₁ tr) → LoggedK S E s s₁ tr) :=
  eval_preserves (loggedK_preserves hR) e ⟨.inr hsf, .inl trivial⟩

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

/-- **T6 com chaves.** Sob contratos `restoring` para `E`, se o corpo (sem saga aninhada, agora
**com** chamadas com chave) de uma saga falha, e toda ferramenta que ficou em aberto no traço
(`Pending`) tem compensador total:

* as compensações são exatamente as operações que o corpo emitiu (aplicadas na hora ou em
  aberto), cada uma uma vez, da mais recente à mais antiga;
* nenhuma é recusada;
* o mundo compensado é `E`-equivalente ao de antes da saga — aplicada ou não cada operação em
  aberto (`S.applied`), o que o agente não sabe. -/
theorem T6_keys (hR : Restoring S E) {body : Exp} (hsf : sagaFree body = true)
    {ρ : List Val} {s : St} {q : Cause} {s₁ : St} {tr : List Ev}
    (hb : eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr))
    (htot : ∀ t k v, Ev.pending t k v ∈ tr → TotalUndo S E t) :
    undoTargets (compensate S s₁.comp s₁.w).evs = (opsOf S tr).reverse ∧
      (compensate S s₁.comp s₁.w).ok = true ∧ E (compensate S s₁.comp s₁.w).w s.w := by
  obtain ⟨new, hc, ha, hp, hw⟩ := (eval_loggedK hR body hsf).2 hb
  simp only [List.append_nil] at hc
  rw [hc]
  refine ⟨by rw [compensate_targets, ha], compensate_restoresK hR new hw fun e he hb' => ?_⟩
  obtain ⟨k, v, h⟩ := hp e he hb'
  exact htot e.1 k v h

/-- O T6 com chaves contém o T6: sem chamada com chave não há `Pending` e as operações são as
ações. -/
theorem opsOf_keyless {tr : List Ev} (h : ∀ t k v, Ev.pending t k v ∉ tr) : opsOf S tr = actsOf S tr := by
  induction tr with
  | nil => rfl
  | cons e l ih =>
    have ih' := ih (fun t k v hm => h t k v (List.mem_cons_of_mem _ hm))
    cases e with
    | pending t k v => exact absurd (List.mem_cons_self ..) (h t k v)
    | act t id v =>
      unfold opsOf actsOf at ih' ⊢
      simp only [List.filterMap_cons, ih']
    | _ =>
      unfold opsOf actsOf at ih' ⊢
      simp only [List.filterMap_cons, ih']

end

/-! ## Exemplo: a remediação como saga -/

namespace SagaExample

open Budget

/-- O mundo: a célula 0 guarda as réplicas de `api`, a célula 1 a saúde (1 = saudável).
A ferramenta 0 escala `api` em `n` réplicas, e a compensação desfaz exatamente isso. A
sonda 0 é `healthy` e a sonda 1 é a recuperação, que exige 3 réplicas. -/
def bump (n : Int) (w : World) : World := fun i => if i = 0 then w 0 + n else w i

def S : Sig where
  cost  := fun _ => 0
  mcost := fun _ => 0
  arg   := fun _ => .int
  effect := fun _ => 0
  pol   := fun _ _ => true
  step  := fun _ v w => match v with | .int n => bump n w | _ => w
  undo  := fun _ v w => match v with | .int n => some (bump (-n) w) | _ => some w
  probe := fun q w => if q = 0 then decide (w 1 = 1) else decide (w 0 = 3)

/-- A mesma assinatura com o provedor recusando a compensação. -/
def Sbroken : Sig := { S with undo := fun _ _ _ => none }

def cfg : Cfg := ⟨fun _ _ _ _ => .unit, 0, fun id => if id = 7 then [0] else []⟩

/-- 3 réplicas, sem saúde. -/
def w₀ : World := fun i => if i = 0 then 3 else 0

def s₀ : St := ⟨[], 0, 0, w₀, [], 100, []⟩

/-- ```
atomic recover replicas_ok {
  scale(cluster, 2)
  await healthy
} on failure { 1 } on compensation_failed { 0 }
``` -/
def saga : Exp :=
  .atomic (.let_ (.act 0 0 (.val (.int 2))) (.await 0)) 1 (.val (.int 1)) (.val (.int 0))

/-- `healthy` nunca confirma: a compensação desfaz a escala, a sonda vê 3 réplicas e o
tratador `on failure` roda. -/
example : (eval S cfg saga [.cap 7] s₀).map (fun o => (o.v, o.tr)) =
    .ok (.int 1, [.act 0 7 (.int 2), .fail (.probe 0), .undo 0 (.int 2) true, .recover 1 true]) := rfl

/-- Com a compensação recusada, o mundo fica com 5 réplicas, a sonda não confirma e o
programa entra em `CompensationFailed`. -/
example : (eval Sbroken cfg saga [.cap 7] s₀).map (fun o => (o.v, o.tr)) =
    .ok (.int 0, [.act 0 7 (.int 2), .fail (.probe 0), .undo 0 (.int 2) false, .recover 1 false]) := rfl

/-- A escala e a sua compensação são inversas exatas: o contrato `restoring` vale para a
igualdade. -/
theorem restoring : Restoring S Eq where
  refl _ := rfl
  trans h₁ h₂ := h₁.trans h₂
  step _ _ _ _ h := h ▸ rfl
  undo t v w w₀ h := by
    subst h
    cases v with
    | int n =>
      refine ⟨w₀, ?_, rfl⟩
      show some (bump (-n) (bump n w₀)) = some w₀
      congr 1
      funext i
      by_cases hi : i = 0
      · subst hi
        show (if (0 : Nat) = 0 then (if (0 : Nat) = 0 then w₀ 0 + n else w₀ 0) + -n else _) = w₀ 0
        simp only [if_true]
        omega
      · simp [bump, hi]
    | _ => exact ⟨w₀, rfl, rfl⟩
  inert _ _ _ h := by simp [S] at h

/-- T6 aplicado: para qualquer mundo inicial com 3 réplicas, se a saga termina por falha
do corpo, o mundo entregue ao tratador tem de novo 3 réplicas. -/
theorem saga_restores {ρ : List Val} {s : St} {o : Out} (hs : s.w 0 = 3)
    (h : eval S cfg saga ρ s = .ok o) :
    (∃ o₁, eval S cfg (.let_ (.act 0 0 (.val (.int 2))) (.await 0)) ρ { s with comp := [] } = .ok o₁ ∧
        o = ⟨o₁.v, exitSaga ρ saga { o₁.st with comp := s.comp }, o₁.tr⟩) ∨
    (∃ (q : Cause) (s₁ : St) (tr : List Ev) (o' : Out), eval S cfg (.let_ (.act 0 0 (.val (.int 2))) (.await 0)) ρ { s with comp := [] } =
        .error (.timeout q s₁ tr) ∧ (compensate S s₁.comp s₁.w).w 0 = 3 ∧
        undoTargets (compensate S s₁.comp s₁.w).evs = (actsOf S tr).reverse ∧
        o.tr = tr ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++
          [.recover 1 (S.probe 1 (compensate S s₁.comp s₁.w).w)] ++ o'.tr) := by
  rcases T6_saga (Inv := fun w => w 0 = 3) restoring (fun w w' h hw => h ▸ hw) rfl rfl hs h with
    hc | ⟨q, s₁, tr, o', hb, htg, _, hinv, _, htr⟩
  · exact .inl hc
  · exact .inr ⟨q, s₁, tr, o', hb, hinv, htg, htr⟩

end SagaExample

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

end KeyedSagaExample

end Aletheia
