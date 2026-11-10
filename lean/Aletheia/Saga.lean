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

/-- As ações de um traço, em ordem: `(ferramenta, argumento)`. -/
def actsOf (tr : List Ev) : List (Nat × Val) :=
  tr.filterMap fun
    | .act t _ v => some (t, v)
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

theorem eval_cond (b : Bool) (f g : Exp) (ρ : List Val) (s : St) :
    cond b (eval S c f ρ s) (eval S c g ρ s) = eval S c (cond b f g) ρ s := by
  cases b <;> rfl

/-- O caminho de falha de uma saga, por extenso: o corpo falhou, as compensações rodaram, a
sonda observou o mundo compensado, e o tratador foi escolhido por `ok && sonda`. O traço
registra o corpo até a falha, a falha, cada compensação, a observação da sonda e o
tratador. -/
theorem atomic_failed {body : Exp} {r : Nat} {f g : Exp} {ρ : List Val} {s : St} {o : Out}
    {q : Nat} {s₁ : St} {tr : List Ev}
    (hb : eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr))
    (h : eval S c (.atomic body r f g) ρ s = .ok o) :
    ∃ o', eval S c (cond ((compensate S s₁.comp s₁.w).ok && S.probe r (compensate S s₁.comp s₁.w).w) f g)
        ρ { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp } = .ok o' ∧
      o = ⟨o'.v, o'.st, tr ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++
        [.recover r (S.probe r (compensate S s₁.comp s₁.w).w)] ++ o'.tr⟩ := by
  simp only [eval, hb] at h
  obtain ⟨o', h', h⟩ := bnd_eq_ok.mp (pre_eq_ok.mp h)
  cases h
  exact ⟨o', by rw [← eval_cond]; exact h', rfl⟩

/-- **T6, sem mascaramento.** Para qualquer mundo, qualquer ferramenta e qualquer
compensação: se o corpo de uma saga falha, `on failure` roda exatamente quando todas as
compensações deram certo e a sonda de recuperação confirmou; em qualquer outro caso roda
`on compensation_failed`. -/
theorem T6_no_masking {body : Exp} {r : Nat} {f g : Exp} {ρ : List Val} {s : St} {o : Out}
    {q : Nat} {s₁ : St} {tr : List Ev}
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
    {o : Out} {q : Nat} {s₁ : St} {tr : List Ev}
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

variable {E : World → World → Prop}

theorem replay_congr (hR : Restoring S E) :
    ∀ (log : List (Nat × Val)) {w₁ w₂ : World}, E w₁ w₂ → E (replay S log w₁) (replay S log w₂)
  | [], _, _, h => h
  | (t, v) :: rest, _, _, h => hR.step t v (replay_congr hR rest h)

/-- O registro acompanha o mundo: o que a execução acrescentou ao registro são as suas
ações, da mais recente à mais antiga, e o mundo é `E`-equivalente a reaplicá-las sobre o
mundo de partida. -/
def Logged (S : Sig) (E : World → World → Prop) (s s' : St) (tr : List Ev) : Prop :=
  ∃ new, s'.comp = new ++ s.comp ∧ new = (actsOf tr).reverse ∧ E s'.w (replay S new s.w)

theorem logged_preserves (hR : Restoring S E) : Preserves S c False (Logged S E) where
  refl _ := ⟨[], rfl, rfl, hR.refl _⟩
  trans := by
    rintro s₁ s₂ s₃ t₁ t₂ ⟨n₁, hc₁, ha₁, hw₁⟩ ⟨n₂, hc₂, ha₂, hw₂⟩
    refine ⟨n₂ ++ n₁, by rw [hc₂, hc₁, List.append_assoc], ?_, ?_⟩
    · simp [actsOf, List.filterMap_append, ha₁, ha₂]
    · rw [replay_append]
      exact hR.trans hw₂ (replay_congr hR n₂ hw₁)
  endorse _ := ⟨[], rfl, rfl, hR.refl _⟩
  reject := ⟨[], rfl, rfl, hR.refl _⟩
  act _ _ := ⟨[_], rfl, rfl, hR.refl _⟩
  drop _ := ⟨[], rfl, rfl, hR.refl _⟩
  infer := ⟨[], rfl, rfl, hR.refl _⟩
  commit h := h.elim
  abort h := h.elim

/-- O registro acompanha o mundo em toda execução sem saga aninhada, no sucesso e no
`timeout`. -/
theorem eval_logged (hR : Restoring S E) (e : Exp) (hsf : sagaFree e = true) {ρ : List Val} {s : St} :
    (∀ {o : Out}, eval S c e ρ s = .ok o → Logged S E s o.st o.tr) ∧
    (∀ {q s₁ tr}, eval S c e ρ s = .error (.timeout q s₁ tr) → Logged S E s s₁ tr) :=
  eval_preserves (logged_preserves hR) e (.inr hsf)

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
theorem T6 (hR : Restoring S E) {body : Exp} (hsf : sagaFree body = true) {ρ : List Val} {s : St}
    {q : Nat} {s₁ : St} {tr : List Ev}
    (hb : eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr)) :
    undoTargets (compensate S s₁.comp s₁.w).evs = (actsOf tr).reverse ∧
      (compensate S s₁.comp s₁.w).ok = true ∧ E (compensate S s₁.comp s₁.w).w s.w := by
  obtain ⟨new, hc, ha, hw⟩ := (eval_logged hR body hsf).2 hb
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
    {body : Exp} {r : Nat} {f g : Exp} (hsf : sagaFree body = true) {ρ : List Val} {s : St} {o : Out}
    (hs : Inv s.w) (h : eval S c (.atomic body r f g) ρ s = .ok o) :
    (∃ o₁, eval S c body ρ { s with comp := [] } = .ok o₁ ∧
        o = ⟨o₁.v, { o₁.st with comp := s.comp }, o₁.tr⟩) ∨
    (∃ q s₁ tr o', eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr) ∧
        undoTargets (compensate S s₁.comp s₁.w).evs = (actsOf tr).reverse ∧
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
      obtain ⟨htg, hok, hw⟩ := T6 hR hsf hb
      obtain ⟨o', he, rfl⟩ := atomic_failed hb h
      rw [hok, Bool.true_and] at he
      exact .inr ⟨q, s₁, tr, o', rfl, htg, hok, hInv _ _ hw hs, he, rfl⟩
    | _ => simp [eval, hb] at h

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

def s₀ : St := ⟨[], 0, 0, w₀, []⟩

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
    .ok (.int 1, [.act 0 7 (.int 2), .fail 0, .undo 0 (.int 2) true, .recover 1 true]) := rfl

/-- Com a compensação recusada, o mundo fica com 5 réplicas, a sonda não confirma e o
programa entra em `CompensationFailed`. -/
example : (eval Sbroken cfg saga [.cap 7] s₀).map (fun o => (o.v, o.tr)) =
    .ok (.int 0, [.act 0 7 (.int 2), .fail 0, .undo 0 (.int 2) false, .recover 1 false]) := rfl

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

/-- T6 aplicado: para qualquer mundo inicial com 3 réplicas, se a saga termina por falha
do corpo, o mundo entregue ao tratador tem de novo 3 réplicas. -/
theorem saga_restores {ρ : List Val} {s : St} {o : Out} (hs : s.w 0 = 3)
    (h : eval S cfg saga ρ s = .ok o) :
    (∃ o₁, eval S cfg (.let_ (.act 0 0 (.val (.int 2))) (.await 0)) ρ { s with comp := [] } = .ok o₁ ∧
        o = ⟨o₁.v, { o₁.st with comp := s.comp }, o₁.tr⟩) ∨
    (∃ (q : Nat) (s₁ : St) (tr : List Ev) (o' : Out), eval S cfg (.let_ (.act 0 0 (.val (.int 2))) (.await 0)) ρ { s with comp := [] } =
        .error (.timeout q s₁ tr) ∧ (compensate S s₁.comp s₁.w).w 0 = 3 ∧
        undoTargets (compensate S s₁.comp s₁.w).evs = (actsOf tr).reverse ∧
        o.tr = tr ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++
          [.recover 1 (S.probe 1 (compensate S s₁.comp s₁.w).w)] ++ o'.tr) := by
  rcases T6_saga (Inv := fun w => w 0 = 3) restoring (fun w w' h hw => h ▸ hw) rfl hs h with
    hc | ⟨q, s₁, tr, o', hb, htg, _, hinv, _, htr⟩
  · exact .inl hc
  · exact .inr ⟨q, s₁, tr, o', hb, hinv, htg, htr⟩

end SagaExample

end Aletheia
