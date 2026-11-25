import Aletheia.Typing

/-!
# Semântica: o avaliador do núcleo

Um avaliador total de passo grande, no estilo dos interpretadores definicionais. Ele não
confia na tipagem: verifica em tempo de execução tudo o que o ARX verifica e falha com um
erro explícito. Os quatro erros são estes:

* `stuck`: operação sobre um valor de forma errada (variável livre, `if` sobre não
  booleano…);
* `noCap`: efeito sem capacidade (o índice de autoridade não guarda uma capacidade) ou sem
  concessão do runtime para a classe de efeito da ferramenta;
* `reuse`: reuso de uma capacidade linear já consumida (por `act` ou por `drop`);
* `budget`: o efeito ultrapassaria o teto de orçamento. Como na reserva do ARX, a
  verificação acontece **antes** do efeito.

O estado carrega as capacidades consumidas, o gasto acumulado, o número de chamadas ao
oráculo, o estado abstrato do mundo e o registro de compensação da saga corrente. A saída de
cada expressão carrega o valor, o estado final e os eventos que **ela** produziu. O Teorema
T1 (`Soundness.lean`) prova que um programa bem tipado nunca chega a nenhum desses erros.

Há um quinto erro, que não é de tipo: `timeout q s tr`, a pós-condição `await q` não
observada. Ele carrega o estado e os eventos até a falha, porque é ele que a saga (`atomic`)
captura para compensar. O `bnd` deste módulo propaga o erro acrescentando à frente os
eventos já produzidos, de modo que o traço de uma saga que falha registra tudo o que o corpo
fez antes da falha. O Teorema T6 (`Saga.lean`) é sobre esse caminho.
-/

namespace Aletheia

open Budget

structure St where
  used  : List Nat
  spent : Budget
  calls : Nat
  /-- o estado abstrato do mundo -/
  w     : World
  /-- o registro de compensação da saga corrente: `(ferramenta, argumento)` de cada ação
  executada, a mais recente primeiro -/
  comp  : List (Nat × Val)
  /-- o contador de cunhagem: a próxima chave de `mint` -/
  next  : Nat
  /-- as chaves aposentadas: cunhadas por um `mint` que já terminou; nenhuma delas identifica
  outra operação -/
  keys  : List Nat

structure Out where
  v  : Val
  st : St
  tr : List Ev

inductive Err where
  | stuck | noCap | reuse | budget
  /-- a falha que uma saga captura, pela causa `q` (`Cause`): a pós-condição não observada,
  a operação de chave recusada ou o encerramento pelo tratador de `TimeoutUnknown`; o estado
  e os eventos até a falha -/
  | timeout (q : Cause) (s : St) (tr : List Ev)

instance : Repr Err where
  reprPrec
    | .stuck, _ => "stuck"
    | .noCap, _ => "noCap"
    | .reuse, _ => "reuse"
    | .budget, _ => "budget"
    | .timeout q _ _, _ => "timeout " ++ repr q

/-- Os eventos que um resultado intermediário já produziu. -/
class HasTr (α : Type) where
  tr : α → List Ev

instance : HasTr Out := ⟨Out.tr⟩
instance : HasTr (St × List Ev) := ⟨Prod.snd⟩

/-- Acrescenta `tr` à frente dos eventos de um `timeout`; não toca nos outros resultados. -/
def pre {α : Type} (tr : List Ev) : Except Err α → Except Err α
  | .error (.timeout q s tr') => .error (.timeout q s (tr ++ tr'))
  | r => r

@[simp] theorem pre_ok {α : Type} (tr : List Ev) (a : α) : pre tr (.ok a) = .ok a := rfl

@[simp] theorem pre_eq_ok {α : Type} {tr : List Ev} {r : Except Err α} {a : α} :
    pre tr r = .ok a ↔ r = .ok a := by
  unfold pre; split <;> simp_all

theorem pre_eq_timeout {α : Type} {tr : List Ev} {r : Except Err α} {q : Cause} {s : St}
    {tr' : List Ev} (h : pre tr r = .error (.timeout q s tr')) :
    ∃ t, r = .error (.timeout q s t) ∧ tr' = tr ++ t := by
  unfold pre at h; split at h
  · cases h; exact ⟨_, rfl, rfl⟩
  · rename_i hn; subst h; exact absurd rfl (hn q s tr')

/-- O `bind` de `Except`: um `timeout` da continuação herda à frente os eventos do valor já
produzido. -/
def bnd {α β : Type} [HasTr α] (r : Except Err α) (f : α → Except Err β) : Except Err β :=
  match r with
  | .ok a => pre (HasTr.tr a) (f a)
  | .error e => .error e

@[simp] theorem bnd_ok {α β : Type} [HasTr α] (a : α) (f : α → Except Err β) :
    bnd (.ok a) f = pre (HasTr.tr a) (f a) := rfl
@[simp] theorem bnd_error {α β : Type} [HasTr α] (e : Err) (f : α → Except Err β) :
    bnd (.error e) f = .error e := rfl

theorem bnd_eq_ok {α β : Type} [HasTr α] {r : Except Err α} {f : α → Except Err β} {b : β} :
    bnd r f = .ok b ↔ ∃ a, r = .ok a ∧ f a = .ok b := by
  cases r <;> simp [bnd]

/-- Um `timeout` de `bnd r f` veio de `r`, ou de `f` depois de `r` produzir `a`. -/
theorem bnd_eq_timeout {α β : Type} [HasTr α] {r : Except Err α} {f : α → Except Err β}
    {q : Cause} {s : St} {tr : List Ev} (h : bnd r f = .error (.timeout q s tr)) :
    r = .error (.timeout q s tr) ∨
      ∃ a t, r = .ok a ∧ f a = .error (.timeout q s t) ∧ tr = HasTr.tr a ++ t := by
  cases r with
  | error e => exact .inl (by simpa [bnd] using h)
  | ok a =>
    obtain ⟨t, ht, rfl⟩ := pre_eq_timeout (by simpa [bnd] using h)
    exact .inr ⟨a, t, rfl, ht, rfl⟩

/-- Itera `f` sobre uma lista, encadeando o estado e concatenando os eventos. -/
def loop (f : Val → St → Except Err Out) : List Val → St → Except Err (St × List Ev)
  | [], s => .ok (s, [])
  | v :: vs, s => bnd (f v s) fun o => bnd (loop f vs o.st) fun r => .ok (r.1, o.tr ++ r.2)

/-- Uma ação `compensable` entra no registro de compensação da saga corrente; as outras
(reversíveis, leituras) não entram, como no ARX. -/
def logAct (S : Sig) (t : Nat) (v : Val) (comp : List (Nat × Val)) : List (Nat × Val) :=
  if S.compensable t then (t, v) :: comp else comp

/-! ## Chaves, `Pending[k]` e reconciliação

Uma ferramenta com chave roda por `call`. A chave da operação é a identidade da capacidade
linear que a autoriza: cada uma é consumida uma única vez (`eval_nodup`), então cada chave
nomeia uma única operação. A chamada pode ser aplicada, recusada ou ficar sem resposta
(`TimeoutUnknown`); no último caso nasce a obrigação `Pending[k]`, e a reconciliação tenta
até `fuel` vezes pelo protocolo da classe do provedor: `idempotent` reenvia com a mesma
chave, `queryable` consulta o status, `opaque` não tem protocolo. Uma resposta conclusiva
decide `Done` ou `Failed`. Sem ela, o tratador recebe a obrigação e precisa transferi-la.

Uma ação sem chave (`act`) é sempre aplicada: o efeito, o registro da saga e o evento. -/

/-- A reconciliação de `k` a partir da tentativa `i`, com `n` tentativas de combustível: os
eventos e o estado terminal. Sem resposta conclusiva, `Transferred`: o tratador recebe a
obrigação. O provedor `opaque` registra a tentativa sem protocolo e esgota na hora. -/
def reconcile (S : Sig) (t k : Nat) : Nat → Nat → List Ev × KEnd
  | _, 0 => ([], .transferred)
  | i, n + 1 =>
    if S.prov t = .opaque then ([.query k i none], .transferred)
    else
      match S.reply t k i with
      | .done => ([.query k i (some .done)], .done)
      | .failed => ([.query k i (some .failed)], .failed)
      | .unknown =>
        let r := reconcile S t k (i + 1) n
        (.query k i (some .unknown) :: r.1, r.2)

/-- O mundo depois de uma ação sem chave. -/
def actW (S : Sig) (t _k : Nat) (v : Val) (w : World) : World := S.step t v w

/-- O registro da saga depois de uma ação sem chave. -/
def actC (S : Sig) (t _k : Nat) (v : Val) (comp : List (Nat × Val)) : List (Nat × Val) :=
  logAct S t v comp

/-- Os eventos de uma ação sem chave. -/
def actEvs (_S : Sig) (t k : Nat) (v : Val) : List Ev := [.act t k v]

theorem reconcile_queries (S : Sig) (t k : Nat) :
    ∀ (i n : Nat), ∀ e ∈ (reconcile S t k i n).1, ∃ j r, e = .query k j r
  | _, 0, e, h => by simp [reconcile] at h
  | i, n + 1, e, h => by
    simp only [reconcile] at h
    split at h
    · simp at h; exact ⟨_, _, h⟩
    · split at h
      · simp at h; exact ⟨_, _, h⟩
      · simp at h; exact ⟨_, _, h⟩
      · simp only [List.mem_cons] at h
        rcases h with h | h
        · exact ⟨_, _, h⟩
        · exact reconcile_queries S t k (i + 1) n e h

theorem filterMap_queries {β : Type} (f : Ev → Option β) (hf : ∀ k i r, f (.query k i r) = none)
    {l : List Ev} {k : Nat} (h : ∀ e ∈ l, ∃ j r, e = .query k j r) : l.filterMap f = [] := by
  induction l with
  | nil => rfl
  | cons e l ih =>
    obtain ⟨j, r, rfl⟩ := h e (List.mem_cons_self ..)
    simp only [List.filterMap_cons, hf]
    exact ih fun x hx => h x (List.mem_cons_of_mem _ hx)

theorem filterMap_actEvs {β : Type} (f : Ev → Option β) (S : Sig) (t k : Nat) (v : Val) :
    (actEvs S t k v).filterMap f = (f (.act t k v)).toList := by
  cases hx : f (.act t k v) <;> simp [actEvs, List.filterMap_cons, hx]

theorem mem_actEvs_act {S : Sig} {t k t' k' : Nat} {v v' : Val} (h : Ev.act t' k' v' ∈ actEvs S t k v) :
    t' = t ∧ k' = k ∧ v' = v := by
  simp [actEvs] at h; exact ⟨h.1, h.2.1, h.2.2⟩

theorem endorse_not_mem_actEvs {S : Sig} {t k p : Nat} {v : Val} {o : Option Val} :
    Ev.endorse p o ∉ actEvs S t k v := by
  simp [actEvs]

theorem undo_not_mem_actEvs {S : Sig} {t k t' : Nat} {v v' : Val} {ok : Bool} :
    Ev.undo t' v' ok ∉ actEvs S t k v := by
  simp [actEvs]

/-- A chave de um evento do protocolo. -/
def keyOf : Ev → Option Nat
  | .pending _ k _ => some k
  | .query k _ _ => some k
  | .settle k _ => some k
  | _ => none

/-- Os eventos do protocolo de chaves, em ordem. -/
def kevs (tr : List Ev) : List Ev := tr.filter fun e => (keyOf e).isSome

/-- As entregas (`transfer`) de um traço. -/
def handoffs (tr : List Ev) : List Ev := tr.filter fun e => match e with | .handoff _ => true | _ => false

/-- A guarda na saída do tratador: ele entregou a obrigação `k` exatamente uma vez e não
produziu nenhum evento de chave. Um tratador que não transfere, que transfere duas vezes ou
que mexe em outra chave termina em erro de linearidade, e a obrigação nunca some em silêncio. -/
def transferOnly (k : Nat) (tr : List Ev) : Bool :=
  (kevs tr).isEmpty &&
    match handoffs tr with
    | [.handoff k'] => k' == k
    | _ => false

/-- O estado depois de uma chamada com chave cuja operação existe no registro da saga: a
capacidade `id` (autoridade) e a chave `kid` (identidade) consumidas, o custo, e o mundo com
o efeito se `b` (aplicado). -/
def callSt (S : Sig) (t id kid : Nat) (v : Val) (st : St) (b : Bool) : St :=
  ⟨kid :: id :: st.used, st.spent + S.acost t, st.calls, if b then S.step t v st.w else st.w,
    logAct S t v st.comp, st.next, st.keys⟩

/-- O registro depois de uma reconciliação conclusiva. A operação que a reconciliação
confirmou **não aplicada** (`Failed`, o `R0611` do ARX) sai do registro: não há efeito a
estornar, e compensá-la seria um efeito novo sem causa. As outras continuam nele. -/
def recLog (S : Sig) (t : Nat) (v : Val) (e : KEnd) (comp : List (Nat × Val)) : List (Nat × Val) :=
  match e with
  | .failed => comp
  | _ => logAct S t v comp

/-- O estado depois de `TimeoutUnknown` e de uma reconciliação que terminou em `e`: a
capacidade e a chave consumidas, o custo, o mundo com a verdade oculta da aplicação e o
registro segundo `recLog`. -/
def recSt (S : Sig) (t id kid : Nat) (v : Val) (st : St) (e : KEnd) : St :=
  ⟨kid :: id :: st.used, st.spent + S.acost t, st.calls,
    if S.applied t kid then S.step t v st.w else st.w, recLog S t v e st.comp, st.next, st.keys⟩

@[simp] theorem recSt_done (S : Sig) (t id kid : Nat) (v : Val) (st : St) :
    recSt S t id kid v st .done = callSt S t id kid v st (S.applied t kid) := rfl

/-- O estado depois de uma chamada recusada na hora: a capacidade e a chave consumidas e o
custo; nada no mundo, nada no registro. -/
def callSt0 (S : Sig) (t id kid : Nat) (st : St) : St :=
  ⟨kid :: id :: st.used, st.spent + S.acost t, st.calls, st.w, st.comp, st.next, st.keys⟩

/-- A aposentadoria da chave `k` na saída do seu `mint`: ela sai do conjunto consumido e
entra no das chaves aposentadas, que nenhuma chamada aceita de novo. -/
def retire (k : Nat) (st : St) : St :=
  { st with used := st.used.filter (· ≠ k), keys := k :: st.keys }

/-- O desfecho de `mint`: o evento de cunhagem à frente e a chave aposentada no estado. -/
def mintOut (k : Nat) : Except Err Out → Except Err Out
  | .ok o => .ok ⟨o.v, retire k o.st, .mint k :: o.tr⟩
  | .error (.timeout q s tr) => .error (.timeout q (retire k s) (.mint k :: tr))
  | .error e => .error e

/-- O efeito de `act t`, depois de avaliado o argumento (saída `o`), com a autoridade `cv`.
A guarda confere, nesta ordem, a concessão do runtime para o efeito da ferramenta, a
linearidade e a reserva de orçamento. Só então o evento é emitido. -/
def doAct (S : Sig) (c : Cfg) (t : Nat) (cv : Option Val) (o : Out) : Except Err Out :=
  match cv with
  | some (.cap id) =>
    if S.effect t ∈ c.grants id then
      if id ∈ o.st.used then .error .reuse
      else if ble (o.st.spent + S.acost t) c.limit then
        .ok ⟨.unit, ⟨id :: o.st.used, o.st.spent + S.acost t, o.st.calls, actW S t id o.v o.st.w,
          actC S t id o.v o.st.comp, o.st.next, o.st.keys⟩, o.tr ++ actEvs S t id o.v⟩
      else .error .budget
    else .error .noCap
  | _ => .error .noCap

/-- O efeito de `call t`, depois de avaliado o argumento (saída `o`), com a autoridade `cv`.
As guardas são as de `act`. A resposta do provedor decide: aplicada (`Done`), recusada
(`Failed`, uma falha que a saga captura) ou sem resposta, quando nasce `Pending[k]` e roda a
reconciliação. Esgotado o combustível, o tratador `h` recebe a obrigação; ele precisa
transferi-la, e o seu valor encerra o evento (`abort`). A operação em aberto entra no
registro da saga (compensar é total sobre "aconteceu" e "não aconteceu", como no ARX), a
menos que a reconciliação confirme que ela não foi aplicada (`recLog`). -/
def doCall (S : Sig) (c : Cfg) (t : Nat) (cv kv : Option Val) (n : Nat)
    (h : Nat → St → Except Err Out) (o : Out) : Except Err Out :=
  match cv, kv with
  | some (.cap id), some (.cap kid) =>
    if S.effect t ∈ c.grants id then
      if id ∈ o.st.used then .error .reuse
      else if kid ∈ id :: o.st.used ∨ kid ∈ o.st.keys then .error .reuse
      else if ble (o.st.spent + S.acost t) c.limit then
        match S.reply t kid 0 with
        | .done =>
          .ok ⟨.unit, callSt S t id kid o.v o.st true, o.tr ++ [.act t id o.v, .settle kid .done]⟩
        | .failed => .error (.timeout (.refused kid) (callSt0 S t id kid o.st) [.settle kid .failed])
        | .unknown =>
          match (reconcile S t kid 1 n).2 with
          | .done =>
            .ok ⟨.unit, recSt S t id kid o.v o.st .done,
              o.tr ++ (.pending t kid o.v :: ((reconcile S t kid 1 n).1 ++ [.settle kid .done]))⟩
          | .failed =>
            .error (.timeout (.refused kid) (recSt S t id kid o.v o.st .failed)
              (.pending t kid o.v :: ((reconcile S t kid 1 n).1 ++ [.settle kid .failed])))
          | .transferred =>
            match h kid (callSt S t id kid o.v o.st (S.applied t kid)) with
            | .ok oh =>
              if transferOnly kid oh.tr then
                .error (.timeout (.abort oh.v) oh.st
                  (.pending t kid o.v :: ((reconcile S t kid 1 n).1 ++ (oh.tr ++ [.settle kid .transferred]))))
              else .error .reuse
            | .error _ => .error .reuse
      else .error .budget
    else .error .noCap
  | _, _ => .error .noCap

/-- `drop k`: consome a capacidade sem efeito e sem evento. -/
def doDrop (cv : Option Val) (s : St) : Except Err Out :=
  match cv with
  | some (.cap id) =>
    if id ∈ s.used then .error .reuse
    else .ok ⟨.unit, ⟨id :: s.used, s.spent, s.calls, s.w, s.comp, s.next, s.keys⟩, []⟩
  | _ => .error .noCap

/-- O efeito de `infer m t`, depois de avaliado o prompt (saída `o`). -/
def doInfer (S : Sig) (c : Cfg) (m : Nat) (t : Ty) (o : Out) : Except Err Out :=
  if ble (o.st.spent + S.mcost m) c.limit then
    .ok ⟨c.oracle o.st.calls m t o.v, ⟨o.st.used, o.st.spent + S.mcost m, o.st.calls + 1, o.st.w,
      o.st.comp, o.st.next, o.st.keys⟩, o.tr⟩
  else .error .budget

/-- Sem repetição, preservando a primeira ocorrência de trás para a frente. -/
def dedup : List Nat → List Nat
  | [] => []
  | x :: xs => if x ∈ dedup xs then dedup xs else x :: dedup xs

theorem mem_dedup : ∀ {xs : List Nat} {x : Nat}, x ∈ dedup xs ↔ x ∈ xs
  | [], _ => by simp [dedup]
  | y :: ys, x => by
    unfold dedup
    split
    · rename_i h; rw [mem_dedup]; constructor
      · exact List.mem_cons_of_mem _
      · intro hx; rcases List.mem_cons.mp hx with rfl | hx
        · exact mem_dedup.mp h
        · exact hx
    · simp [mem_dedup]

theorem nodup_dedup : ∀ (xs : List Nat), (dedup xs).Nodup
  | [] => List.nodup_nil
  | y :: ys => by
    unfold dedup
    split
    · exact nodup_dedup ys
    · rename_i h; exact List.nodup_cons.mpr ⟨h, nodup_dedup ys⟩

/-- As identidades das capacidades do ambiente que a contagem `f` consome uma vez. -/
def capsOf (ρ : List Val) (f : Nat → Option Nat) : List Nat :=
  (List.range ρ.length).filterMap fun k =>
    match ρ[k]? with
    | some (Val.cap id) => if f k = some 1 then some id else none
    | _ => none

theorem mem_capsOf {ρ : List Val} {f : Nat → Option Nat} {x : Nat} :
    x ∈ capsOf ρ f ↔ ∃ k, ρ[k]? = some (.cap x) ∧ f k = some 1 := by
  simp only [capsOf, List.mem_filterMap, List.mem_range]
  constructor
  · rintro ⟨k, _, hk⟩
    split at hk
    · rename_i id hρ
      split at hk
      · cases hk; exact ⟨k, hρ, by assumption⟩
      · cases hk
    · cases hk
  · rintro ⟨k, hρ, h1⟩
    refine ⟨k, ?_, by rw [hρ]; simp [h1]⟩
    exact (List.getElem?_eq_some_iff.mp hρ).1

/-- A revogação na saída de uma saga: as capacidades que a saga conta como consumidas e que
o caminho percorrido não consumiu. Assim todo caminho consome exatamente a contagem da
saga, inclusive o de confirmação (sem tratador) e o de falha (corpo interrompido). -/
def revoke (ρ : List Val) (f : Nat → Option Nat) (u : List Nat) : List Nat :=
  dedup ((capsOf ρ f).filter fun x => x ∉ u)

theorem revoke_nodup (ρ : List Val) (f : Nat → Option Nat) (u : List Nat) : (revoke ρ f u).Nodup :=
  nodup_dedup _

theorem mem_revoke {ρ : List Val} {f : Nat → Option Nat} {u : List Nat} {x : Nat} :
    x ∈ revoke ρ f u ↔ (∃ k, ρ[k]? = some (.cap x) ∧ f k = some 1) ∧ x ∉ u := by
  simp [revoke, mem_dedup, List.mem_filter, mem_capsOf]

/-- A saída de uma saga: revoga o que ela não consumiu no caminho percorrido. -/
def exitSaga (ρ : List Val) (e : Exp) (st : St) : St :=
  { st with used := revoke ρ (consumes e) st.used ++ st.used }

/-- O resultado das compensações de uma saga. -/
structure Comp where
  w   : World
  evs : List Ev
  ok  : Bool

/-- Compensa o registro da mais recente à mais antiga ação. Cada compensação roda uma vez e
como obrigação, sem reserva de orçamento. Uma compensação recusada deixa o mundo como está e
marca a falha, e as seguintes ainda rodam, como no ARX. -/
def compensate (S : Sig) : List (Nat × Val) → World → Comp
  | [], w => ⟨w, [], true⟩
  | (t, v) :: rest, w =>
    match S.undo t v w with
    | some w' =>
      let k := compensate S rest w'
      ⟨k.w, .undo t v true :: k.evs, k.ok⟩
    | none =>
      let k := compensate S rest w
      ⟨k.w, .undo t v false :: k.evs, false⟩

/-- As compensações só produzem eventos `undo`. -/
theorem compensate_undo (S : Sig) :
    ∀ (log : List (Nat × Val)) (w : World), ∀ ev ∈ (compensate S log w).evs, ∃ t v b, ev = .undo t v b
  | [], _, ev, h => by simp [compensate] at h
  | (t, v) :: rest, w, ev, h => by
    simp only [compensate] at h
    split at h
    · rcases List.mem_cons.mp h with rfl | h
      · exact ⟨t, v, true, rfl⟩
      · exact compensate_undo S rest _ ev h
    · rcases List.mem_cons.mp h with rfl | h
      · exact ⟨t, v, false, rfl⟩
      · exact compensate_undo S rest _ ev h

def eval (S : Sig) (c : Cfg) : Exp → List Val → St → Except Err Out
  | .val v, _, s => .ok ⟨v, s, []⟩
  | .var k, ρ, s =>
    match ρ[k]? with
    | some v => .ok ⟨v, s, []⟩
    | none => .error .stuck
  | .let_ e₁ e₂, ρ, s =>
    bnd (eval S c e₁ ρ s) fun o₁ =>
    bnd (eval S c e₂ (o₁.v :: ρ) o₁.st) fun o₂ =>
    .ok ⟨o₂.v, o₂.st, o₁.tr ++ o₂.tr⟩
  | .ite cnd t f, ρ, s =>
    bnd (eval S c cnd ρ s) fun o =>
    match o.v with
    | .bool b =>
      bnd (cond b (eval S c t ρ o.st) (eval S c f ρ o.st)) fun o' => .ok ⟨o'.v, o'.st, o.tr ++ o'.tr⟩
    | _ => .error .stuck
  | .prim op a b, ρ, s =>
    bnd (eval S c a ρ s) fun o₁ =>
    bnd (eval S c b ρ o₁.st) fun o₂ =>
    match o₁.v, o₂.v with
    | .int x, .int y => .ok ⟨op.apply x y, o₂.st, o₁.tr ++ o₂.tr⟩
    | _, _ => .error .stuck
  | .endorse p e d, ρ, s =>
    bnd (eval S c e ρ s) fun o =>
    if S.pol p o.v then .ok ⟨o.v, o.st, o.tr ++ [.endorse p (some o.v)]⟩
    else pre [.endorse p none] (bnd (eval S c d ρ o.st) fun o' =>
      .ok ⟨o'.v, o'.st, o.tr ++ .endorse p none :: o'.tr⟩)
  | .act t k a, ρ, s =>
    bnd (eval S c a ρ s) fun o => doAct S c t ρ[k]? o
  | .drop k, ρ, s => doDrop ρ[k]? s
  | .for_ _ xs body, ρ, s =>
    bnd (eval S c xs ρ s) fun o =>
    match o.v with
    | .list vs =>
      bnd (loop (fun v s' => eval S c body (v :: ρ) s') vs o.st) fun r =>
      .ok ⟨.unit, r.1, o.tr ++ r.2⟩
    | _ => .error .stuck
  | .infer m t a, ρ, s =>
    bnd (eval S c a ρ s) fun o => doInfer S c m t o
  | .await q, _, s =>
    if S.probe q s.w then .ok ⟨.unit, s, []⟩ else .error (.timeout (.probe q) s [])
  | .atomic body r f g, ρ, s =>
    match eval S c body ρ { s with comp := [] } with
    | .ok o =>
      -- commit: os efeitos ficam; o registro da saga é descartado, como no ARX; as
      -- capacidades da saga que os tratadores usariam são revogadas
      .ok ⟨o.v, exitSaga ρ (.atomic body r f g) { o.st with comp := s.comp }, o.tr⟩
    | .error (.timeout q s₁ tr) =>
      let k := compensate S s₁.comp s₁.w
      let s₂ : St := { s₁ with w := k.w, comp := s.comp }
      let p := S.probe r k.w
      let evs := tr ++ .fail q :: k.evs ++ [.recover r p]
      -- o encerramento pelo tratador de `TimeoutUnknown` atravessa a saga recuperada sem
      -- rodar `on failure`, como no ARX; sem recuperação, `on compensation_failed` roda
      if q.isAbort && (k.ok && p) then pre evs (.error (.timeout q s₂ []))
      else
        pre evs (bnd (cond (k.ok && p) (eval S c f ρ s₂) (eval S c g ρ s₂)) fun o' =>
          .ok ⟨o'.v, exitSaga ρ (.atomic body r f g) o'.st, evs ++ o'.tr⟩)
    | .error e => .error e
  | .call t j κ a n e, ρ, s =>
    bnd (eval S c a ρ s) fun o =>
      doCall S c t ρ[j]? ρ[κ]? n (fun id s' => eval S c e [.pend t id] s') o
  -- a chave fresca é o contador; a guarda confere que ela nunca foi usada nem aposentada
  | .mint e, ρ, s =>
    if s.next ∈ s.used ∨ s.next ∈ s.keys then .error .reuse
    else mintOut s.next (eval S c e (.cap s.next :: ρ) { s with next := s.next + 1 })
  | .transfer p, ρ, s =>
    match ρ[p]? with
    | some (.pend _ k) => .ok ⟨.unit, s, [.handoff k]⟩
    | _ => .error .stuck

theorem cond_ok {α : Type} {b : Bool} {x y : Except Err α} {r : α} (h : cond b x y = .ok r) :
    (b = true ∧ x = .ok r) ∨ (b = false ∧ y = .ok r) := by
  cases b
  · exact .inr ⟨rfl, h⟩
  · exact .inl ⟨rfl, h⟩

theorem cond_eq {α : Type} {b : Bool} {x y r : Except Err α} (h : cond b x y = r) :
    (b = true ∧ x = r) ∨ (b = false ∧ y = r) := by
  cases b
  · exact .inr ⟨rfl, h⟩
  · exact .inl ⟨rfl, h⟩

theorem doAct_ok {S : Sig} {c : Cfg} {t : Nat} {cv : Option Val} {o r : Out}
    (h : doAct S c t cv o = .ok r) :
    ∃ id, cv = some (.cap id) ∧ S.effect t ∈ c.grants id ∧ id ∉ o.st.used ∧
      ble (o.st.spent + S.acost t) c.limit = true ∧
      r = ⟨.unit, ⟨id :: o.st.used, o.st.spent + S.acost t, o.st.calls, actW S t id o.v o.st.w,
        actC S t id o.v o.st.comp, o.st.next, o.st.keys⟩, o.tr ++ actEvs S t id o.v⟩ := by
  unfold doAct at h
  split at h
  · rename_i id
    by_cases hg : S.effect t ∈ c.grants id
    · by_cases hu : id ∈ o.st.used
      · simp [hg, hu] at h
      · by_cases hb : ble (o.st.spent + S.acost t) c.limit = true
        · simp [hg, hu, hb] at h
          exact ⟨id, rfl, hg, hu, hb, h.symm⟩
        · simp [hg, hu, hb] at h
    · simp [hg] at h
  · simp at h

theorem doDrop_ok {cv : Option Val} {s : St} {r : Out} (h : doDrop cv s = .ok r) :
    ∃ id, cv = some (.cap id) ∧ id ∉ s.used ∧
      r = ⟨.unit, ⟨id :: s.used, s.spent, s.calls, s.w, s.comp, s.next, s.keys⟩, []⟩ := by
  unfold doDrop at h
  split at h
  · rename_i id
    by_cases hu : id ∈ s.used
    · simp [hu] at h
    · simp [hu] at h
      exact ⟨id, rfl, hu, h.symm⟩
  · simp at h

theorem doInfer_ok {S : Sig} {c : Cfg} {m : Nat} {t : Ty} {o r : Out}
    (h : doInfer S c m t o = .ok r) :
    ble (o.st.spent + S.mcost m) c.limit = true ∧
      r = ⟨c.oracle o.st.calls m t o.v, ⟨o.st.used, o.st.spent + S.mcost m, o.st.calls + 1, o.st.w,
        o.st.comp, o.st.next, o.st.keys⟩, o.tr⟩ := by
  unfold doInfer at h
  by_cases hb : ble (o.st.spent + S.mcost m) c.limit = true
  · simp [hb] at h; exact ⟨hb, h.symm⟩
  · simp [hb] at h

theorem doAct_ne_timeout {S : Sig} {c : Cfg} {t : Nat} {cv : Option Val} {o : Out} {q : Cause}
    {s : St} {tr : List Ev} : doAct S c t cv o ≠ .error (.timeout q s tr) := by
  unfold doAct; split
  · split
    · split
      · nofun
      · split <;> nofun
    · nofun
  · nofun

theorem doDrop_ne_timeout {cv : Option Val} {s₀ : St} {q : Cause} {s : St} {tr : List Ev} :
    doDrop cv s₀ ≠ .error (.timeout q s tr) := by
  unfold doDrop; split
  · split <;> nofun
  · nofun

theorem doInfer_ne_timeout {S : Sig} {c : Cfg} {m : Nat} {t : Ty} {o : Out} {q : Cause} {s : St}
    {tr : List Ev} : doInfer S c m t o ≠ .error (.timeout q s tr) := by
  unfold doInfer; split <;> nofun

theorem doCall_ok {S : Sig} {c : Cfg} {t : Nat} {cv kv : Option Val} {n : Nat}
    {h : Nat → St → Except Err Out} {o r : Out} (hc : doCall S c t cv kv n h o = .ok r) :
    ∃ id kid, cv = some (.cap id) ∧ kv = some (.cap kid) ∧ S.effect t ∈ c.grants id ∧
      id ∉ o.st.used ∧ kid ∉ id :: o.st.used ∧ kid ∉ o.st.keys ∧
      ble (o.st.spent + S.acost t) c.limit = true ∧
      ((S.reply t kid 0 = .done ∧
          r = ⟨.unit, callSt S t id kid o.v o.st true, o.tr ++ [.act t id o.v, .settle kid .done]⟩) ∨
       (S.reply t kid 0 = .unknown ∧ (reconcile S t kid 1 n).2 = .done ∧
          r = ⟨.unit, callSt S t id kid o.v o.st (S.applied t kid),
            o.tr ++ (.pending t kid o.v :: ((reconcile S t kid 1 n).1 ++ [.settle kid .done]))⟩)) := by
  unfold doCall at hc
  split at hc
  · rename_i id kid
    by_cases hg : S.effect t ∈ c.grants id
    · rw [if_pos hg] at hc
      by_cases hu : id ∈ o.st.used
      · rw [if_pos hu] at hc; cases hc
      · rw [if_neg hu] at hc
        by_cases hk : kid ∈ id :: o.st.used ∨ kid ∈ o.st.keys
        · rw [if_pos hk] at hc; cases hc
        · rw [if_neg hk] at hc
          obtain ⟨hk₁, hk₂⟩ := not_or.mp hk
          by_cases hb : ble (o.st.spent + S.acost t) c.limit = true
          · rw [if_pos hb] at hc
            refine ⟨id, kid, rfl, rfl, hg, hu, hk₁, hk₂, hb, ?_⟩
            split at hc
            · rename_i h0; cases hc; exact .inl ⟨h0, rfl⟩
            · cases hc
            · rename_i h0
              split at hc
              · rename_i hr; cases hc; exact .inr ⟨h0, hr, rfl⟩
              · cases hc
              · split at hc
                · split at hc <;> cases hc
                · cases hc
          · rw [if_neg hb] at hc; cases hc
    · rw [if_neg hg] at hc; cases hc
  · cases hc

theorem doCall_timeout {S : Sig} {c : Cfg} {t : Nat} {cv kv : Option Val} {n : Nat}
    {h : Nat → St → Except Err Out} {o : Out} {q : Cause} {s₁ : St} {tr : List Ev}
    (hc : doCall S c t cv kv n h o = .error (.timeout q s₁ tr)) :
    ∃ id kid, cv = some (.cap id) ∧ kv = some (.cap kid) ∧ S.effect t ∈ c.grants id ∧
      id ∉ o.st.used ∧ kid ∉ id :: o.st.used ∧ kid ∉ o.st.keys ∧
      ble (o.st.spent + S.acost t) c.limit = true ∧
      ((S.reply t kid 0 = .failed ∧ q = .refused kid ∧ s₁ = callSt0 S t id kid o.st ∧
          tr = [.settle kid .failed]) ∨
       (S.reply t kid 0 = .unknown ∧ (reconcile S t kid 1 n).2 = .failed ∧ q = .refused kid ∧
          s₁ = recSt S t id kid o.v o.st .failed ∧
          tr = .pending t kid o.v :: ((reconcile S t kid 1 n).1 ++ [.settle kid .failed])) ∨
       (S.reply t kid 0 = .unknown ∧ (reconcile S t kid 1 n).2 = .transferred ∧
          ∃ oh, h kid (callSt S t id kid o.v o.st (S.applied t kid)) = .ok oh ∧
            transferOnly kid oh.tr = true ∧ q = .abort oh.v ∧ s₁ = oh.st ∧
            tr = .pending t kid o.v :: ((reconcile S t kid 1 n).1 ++
              (oh.tr ++ [.settle kid .transferred])))) := by
  unfold doCall at hc
  split at hc
  · rename_i id kid
    by_cases hg : S.effect t ∈ c.grants id
    · rw [if_pos hg] at hc
      by_cases hu : id ∈ o.st.used
      · rw [if_pos hu] at hc; cases hc
      · rw [if_neg hu] at hc
        by_cases hk : kid ∈ id :: o.st.used ∨ kid ∈ o.st.keys
        · rw [if_pos hk] at hc; cases hc
        · rw [if_neg hk] at hc
          obtain ⟨hk₁, hk₂⟩ := not_or.mp hk
          by_cases hb : ble (o.st.spent + S.acost t) c.limit = true
          · rw [if_pos hb] at hc
            refine ⟨id, kid, rfl, rfl, hg, hu, hk₁, hk₂, hb, ?_⟩
            split at hc
            · cases hc
            · rename_i h0; cases hc; exact .inl ⟨h0, rfl, rfl, rfl⟩
            · rename_i h0
              split at hc
              · cases hc
              · rename_i hr; cases hc; exact .inr (.inl ⟨h0, hr, rfl, rfl, rfl⟩)
              · rename_i hr
                split at hc
                · rename_i oh hoh
                  split at hc
                  · rename_i hto; cases hc
                    exact .inr (.inr ⟨h0, hr, oh, hoh, hto, rfl, rfl, rfl⟩)
                  · cases hc
                · cases hc
          · rw [if_neg hb] at hc; cases hc
    · rw [if_neg hg] at hc; cases hc
  · cases hc

theorem mintOut_ok {k : Nat} {r : Except Err Out} {o : Out} (h : mintOut k r = .ok o) :
    ∃ o', r = .ok o' ∧ o = ⟨o'.v, retire k o'.st, .mint k :: o'.tr⟩ := by
  unfold mintOut at h; split at h
  · cases h; exact ⟨_, rfl, rfl⟩
  · cases h
  · cases h

theorem mintOut_timeout {k : Nat} {r : Except Err Out} {q : Cause} {s : St} {tr : List Ev}
    (h : mintOut k r = .error (.timeout q s tr)) :
    ∃ s' tr', r = .error (.timeout q s' tr') ∧ s = retire k s' ∧ tr = .mint k :: tr' := by
  unfold mintOut at h; split at h
  · cases h
  · cases h; exact ⟨_, _, rfl, rfl, rfl⟩
  · rename_i hne; cases h; exact absurd rfl (hne _ _ _)

theorem eval_mint_ok {S : Sig} {c : Cfg} {e : Exp} {ρ : List Val} {s : St} {o : Out}
    (h : eval S c (.mint e) ρ s = .ok o) :
    s.next ∉ s.used ∧ s.next ∉ s.keys ∧
      ∃ o', eval S c e (.cap s.next :: ρ) { s with next := s.next + 1 } = .ok o' ∧
        o = ⟨o'.v, retire s.next o'.st, .mint s.next :: o'.tr⟩ := by
  simp only [eval] at h
  split at h
  · cases h
  · rename_i hg
    obtain ⟨a, b⟩ := not_or.mp hg
    exact ⟨a, b, mintOut_ok h⟩

theorem eval_mint_timeout {S : Sig} {c : Cfg} {e : Exp} {ρ : List Val} {s : St} {q : Cause}
    {s₁ : St} {tr : List Ev} (h : eval S c (.mint e) ρ s = .error (.timeout q s₁ tr)) :
    s.next ∉ s.used ∧ s.next ∉ s.keys ∧
      ∃ s' tr', eval S c e (.cap s.next :: ρ) { s with next := s.next + 1 } =
        .error (.timeout q s' tr') ∧ s₁ = retire s.next s' ∧ tr = .mint s.next :: tr' := by
  simp only [eval] at h
  split at h
  · cases h
  · rename_i hg
    obtain ⟨a, b⟩ := not_or.mp hg
    exact ⟨a, b, mintOut_timeout h⟩

/-- Sem `atomic`: o fragmento em que uma saga não aninha outra. -/
def sagaFree : Exp → Bool
  | .let_ a b | .prim _ a b => sagaFree a && sagaFree b
  | .ite x t f => sagaFree x && sagaFree t && sagaFree f
  | .endorse _ e d => sagaFree e && sagaFree d
  | .act _ _ a | .infer _ _ a => sagaFree a
  | .for_ _ xs body => sagaFree xs && sagaFree body
  | .atomic .. => false
  | .call _ _ _ a _ e => sagaFree a && sagaFree e
  | .mint e => sagaFree e
  | .val _ | .var _ | .drop _ | .await _ | .transfer _ => true

/-- `await` só dentro do corpo de uma saga (o E0601 do `athc`). `sg` diz se a posição está
dentro do corpo de alguma saga; os tratadores herdam a posição da própria saga. -/
def guarded : Bool → Exp → Bool
  | sg, .let_ a b | sg, .prim _ a b => guarded sg a && guarded sg b
  | sg, .ite x t f => guarded sg x && guarded sg t && guarded sg f
  | sg, .endorse _ e d => guarded sg e && guarded sg d
  | sg, .act _ _ a | sg, .infer _ _ a => guarded sg a
  | sg, .for_ _ xs body => guarded sg xs && guarded sg body
  | sg, .await _ => sg
  | sg, .atomic body _ f g => guarded true body && guarded sg f && guarded sg g
  | sg, .call _ _ _ a _ e => guarded sg a && guarded false e
  | sg, .mint e => guarded sg e
  | _, .val _ | _, .var _ | _, .drop _ | _, .transfer _ => true

/-- Um programa em que todo `await` está dentro de uma saga. -/
def Guarded (e : Exp) : Prop := guarded false e = true

/-- Sem `call`: o fragmento das versões anteriores do núcleo. -/
def keyless : Exp → Bool
  | .let_ a b | .prim _ a b => keyless a && keyless b
  | .ite x t f => keyless x && keyless t && keyless f
  | .endorse _ e d => keyless e && keyless d
  | .act _ _ a | .infer _ _ a => keyless a
  | .for_ _ xs body => keyless xs && keyless body
  | .atomic body _ f g => keyless body && keyless f && keyless g
  | .call .. => false
  | .mint e => keyless e
  | .val _ | .var _ | .drop _ | .await _ | .transfer _ => true

/-- O fragmento de uma instância de `Preserves`: sagas aninhadas só se `sagas`, chamadas com
chave só se `keys`. -/
def Adm (sagas keys : Prop) (e : Exp) : Prop := (sagas ∨ sagaFree e = true) ∧ (keys ∨ keyless e = true)

theorem Adm.mono {P K : Prop} {e e' : Exp} (h : Adm P K e)
    (hs : (sagaFree e = true → sagaFree e' = true) ∧ (keyless e = true → keyless e' = true)) :
    Adm P K e' := ⟨h.1.imp id hs.1, h.2.imp id hs.2⟩

theorem eval_cond {S : Sig} {c : Cfg} (b : Bool) (f g : Exp) (ρ : List Val) (s : St) :
    cond b (eval S c f ρ s) (eval S c g ρ s) = eval S c (cond b f g) ρ s := by
  cases b <;> rfl

/-- A saga que confirma. -/
theorem eval_atomic_ok {S : Sig} {c : Cfg} {body : Exp} {r : Nat} {f g : Exp} {ρ : List Val}
    {s : St} {o : Out} (h : eval S c body ρ { s with comp := [] } = .ok o) :
    eval S c (.atomic body r f g) ρ s =
      .ok ⟨o.v, exitSaga ρ (.atomic body r f g) { o.st with comp := s.comp }, o.tr⟩ := by
  simp only [eval, h]

/-- A saga cujo corpo falha: compensa, observa e roda o tratador escolhido. -/
theorem eval_atomic_fail {S : Sig} {c : Cfg} {body : Exp} {r : Nat} {f g : Exp} {ρ : List Val}
    {s : St} {q : Cause} {s₁ : St} {tr : List Ev}
    (h : eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr))
    (hn : (q.isAbort && ((compensate S s₁.comp s₁.w).ok && S.probe r (compensate S s₁.comp s₁.w).w)) = false) :
    eval S c (.atomic body r f g) ρ s =
      pre (tr ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++
          [.recover r (S.probe r (compensate S s₁.comp s₁.w).w)])
        (bnd (eval S c (cond ((compensate S s₁.comp s₁.w).ok && S.probe r (compensate S s₁.comp s₁.w).w) f g)
            ρ { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp }) fun o' =>
          .ok ⟨o'.v, exitSaga ρ (.atomic body r f g) o'.st,
            (tr ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++
              [.recover r (S.probe r (compensate S s₁.comp s₁.w).w)]) ++ o'.tr⟩) := by
  simp only [eval, h, hn, Bool.false_eq_true, if_false]
  rw [eval_cond]

/-- O encerramento pelo tratador atravessa a saga que recuperou: compensa, observa e sobe. -/
theorem eval_atomic_reraise {S : Sig} {c : Cfg} {body : Exp} {r : Nat} {f g : Exp} {ρ : List Val}
    {s : St} {q : Cause} {s₁ : St} {tr : List Ev}
    (h : eval S c body ρ { s with comp := [] } = .error (.timeout q s₁ tr))
    (hy : (q.isAbort && ((compensate S s₁.comp s₁.w).ok && S.probe r (compensate S s₁.comp s₁.w).w)) = true) :
    eval S c (.atomic body r f g) ρ s =
      .error (.timeout q { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp }
        ((tr ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++
          [.recover r (S.probe r (compensate S s₁.comp s₁.w).w)]) ++ [])) := by
  simp only [eval, h, hy, if_true, pre]

/-! ## Fora de saga, nenhuma falha de pós-condição escapa

Com todo `await` dentro de uma saga, uma pós-condição não observada é sempre capturada. As
outras duas causas de falha não são erros de programa: a recusa de uma operação com chave
(`refused`) depende do provedor, e o encerramento pelo tratador (`abort`) é um desfecho
previsto. O T1 trata as três como desfechos (`Soundness.lean`). -/

section
variable {S : Sig} {c : Cfg}

theorem loop_no_timeout {f : Val → St → Except Err Out}
    (hf : ∀ v s q s₁ tr, f v s ≠ .error (.timeout (.probe q) s₁ tr)) :
    ∀ (vs : List Val) (s : St) (q : Nat) (s₁ : St) (tr : List Ev),
      loop f vs s ≠ .error (.timeout (.probe q) s₁ tr)
  | [], s, q, s₁, tr, h => by simp [loop] at h
  | v :: vs, s, q, s₁, tr, h => by
    simp only [loop] at h
    rcases bnd_eq_timeout h with h₀ | ⟨o, t, _, h₁, _⟩
    · exact hf v s q s₁ tr h₀
    · rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
      · exact loop_no_timeout hf vs _ q s₁ _ h₂
      · cases h₃

/-- Com todo `await` dentro de uma saga, a falha de uma pós-condição é sempre capturada pela
saga que a contém: a execução nunca termina em falha de pós-condição. -/
theorem guarded_no_timeout : ∀ (e : Exp), guarded false e = true →
    ∀ (ρ : List Val) (s : St) (q : Nat) (s₁ : St) (tr : List Ev),
      eval S c e ρ s ≠ .error (.timeout (.probe q) s₁ tr)
  | .val _, _, ρ, s, q, s₁, tr, h => by simp [eval] at h
  | .var k, _, ρ, s, q, s₁, tr, h => by
    simp only [eval] at h; split at h <;> cases h
  | .let_ e₁ e₂, hg, ρ, s, q, s₁, tr, h => by
    simp only [guarded, Bool.and_eq_true] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨o₁, t, _, h₁, _⟩
    · exact guarded_no_timeout e₁ hg.1 _ _ _ _ _ h₀
    · rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
      · exact guarded_no_timeout e₂ hg.2 _ _ _ _ _ h₂
      · cases h₃
  | .ite x t f, hg, ρ, s, q, s₁, tr, h => by
    simp only [guarded, Bool.and_eq_true] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨o, tr', _, h₁, _⟩
    · exact guarded_no_timeout x hg.1.1 _ _ _ _ _ h₀
    · revert h₁; split
      · intro h₁
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · rcases cond_eq h₂ with ⟨_, h₄⟩ | ⟨_, h₄⟩
          · exact guarded_no_timeout t hg.1.2 _ _ _ _ _ h₄
          · exact guarded_no_timeout f hg.2 _ _ _ _ _ h₄
        · cases h₃
      · intro h₁; cases h₁
  | .prim op a b, hg, ρ, s, q, s₁, tr, h => by
    simp only [guarded, Bool.and_eq_true] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨o₁, t, _, h₁, _⟩
    · exact guarded_no_timeout a hg.1 _ _ _ _ _ h₀
    · rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
      · exact guarded_no_timeout b hg.2 _ _ _ _ _ h₂
      · revert h₃; split <;> intro h₃ <;> cases h₃
  | .endorse p e d, hg, ρ, s, q, s₁, tr, h => by
    simp only [guarded, Bool.and_eq_true] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨oe, t, _, h₁, _⟩
    · exact guarded_no_timeout e hg.1 _ _ _ _ _ h₀
    · revert h₁; split
      · intro h₁; cases h₁
      · intro h₁
        obtain ⟨t', h₂, _⟩ := pre_eq_timeout h₁
        rcases bnd_eq_timeout h₂ with h₃ | ⟨_, _, _, h₄, _⟩
        · exact guarded_no_timeout d hg.2 _ _ _ _ _ h₃
        · cases h₄
  | .act t k a, hg, ρ, s, q, s₁, tr, h => by
    simp only [guarded] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨_, _, _, h₁, _⟩
    · exact guarded_no_timeout a hg _ _ _ _ _ h₀
    · exact doAct_ne_timeout h₁
  | .drop k, _, ρ, s, q, s₁, tr, h => by
    simp only [eval] at h; exact doDrop_ne_timeout h
  | .for_ n xs body, hg, ρ, s, q, s₁, tr, h => by
    simp only [guarded, Bool.and_eq_true] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨ox, t, _, h₁, _⟩
    · exact guarded_no_timeout xs hg.1 _ _ _ _ _ h₀
    · revert h₁; split
      · intro h₁
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · exact loop_no_timeout (fun v s' => guarded_no_timeout body hg.2 (v :: ρ) s') _ _ _ _ _ h₂
        · cases h₃
      · intro h₁; cases h₁
  | .infer m t a, hg, ρ, s, q, s₁, tr, h => by
    simp only [guarded] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨_, _, _, h₁, _⟩
    · exact guarded_no_timeout a hg _ _ _ _ _ h₀
    · exact doInfer_ne_timeout h₁
  | .await q', hg, ρ, s, q, s₁, tr, h => by simp [guarded] at hg
  | .atomic body r f g, hg, ρ, s, q, s₁, tr, h => by
    simp only [guarded, Bool.and_eq_true] at hg
    simp only [eval] at h
    split at h
    · cases h
    · rename_i q₀ _ _ _
      split at h
      · rename_i hy
        simp only [pre] at h
        cases h
        simp [Cause.isAbort] at hy
      · obtain ⟨t', h₁, _⟩ := pre_eq_timeout h
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · rcases cond_eq h₂ with ⟨_, h₄⟩ | ⟨_, h₄⟩
          · exact guarded_no_timeout f hg.1.2 _ _ _ _ _ h₄
          · exact guarded_no_timeout g hg.2 _ _ _ _ _ h₄
        · cases h₃
    · rename_i hne _
      cases h
      exact absurd rfl (hne _ _ _)
  | .call t j κ a n e, hg, ρ, s, q, s₁, tr, h => by
    simp only [guarded, Bool.and_eq_true] at hg
    simp only [eval] at h
    rcases bnd_eq_timeout h with h₀ | ⟨o, t', _, h₁, _⟩
    · exact guarded_no_timeout a hg.1 _ _ _ _ _ h₀
    · obtain ⟨id, kid, -, -, -, -, -, -, -, h₂⟩ := doCall_timeout h₁
      rcases h₂ with ⟨-, hq, -⟩ | ⟨-, -, hq, -⟩ | ⟨-, -, oh, -, -, hq, -⟩ <;> cases hq
  | .mint e, hg, ρ, s, q, s₁, tr, h => by
    simp only [guarded] at hg
    obtain ⟨_, _, s', tr', h₃, _⟩ := eval_mint_timeout h
    exact guarded_no_timeout e hg _ _ _ _ _ h₃
  | .transfer p, _, ρ, s, q, s₁, tr, h => by
    simp only [eval] at h; split at h <;> cases h

end

/-! ## Invariantes nos dois desfechos

Uma relação `R s s' tr` entre o estado de partida, o estado de chegada e os eventos
produzidos é **preservada** se vale para cada passo primitivo e se compõe. Então ela vale
para toda execução, tanto no sucesso quanto no `timeout` (com o estado e os eventos que a
saga captura). As propriedades por construção do avaliador (capacidades nunca reusadas,
efeitos sempre concedidos, endossos sempre pela política) são instâncias disto, com
`sagas = True`. Um invariante que só vale fora de sagas aninhadas (o registro de compensação
do T6) usa `sagas = False`: os campos `commit` e `abort` ficam vazios, e o teorema vale para
expressões sem `atomic`. -/

structure Preserves (S : Sig) (c : Cfg) (sagas keys : Prop) (R : St → St → List Ev → Prop) : Prop where
  refl    : ∀ s, R s s []
  trans   : ∀ {s₁ s₂ s₃ : St} {t₁ t₂ : List Ev}, R s₁ s₂ t₁ → R s₂ s₃ t₂ → R s₁ s₃ (t₁ ++ t₂)
  endorse : ∀ {s : St} {p : Nat} {v : Val}, S.pol p v = true → R s s [.endorse p (some v)]
  reject  : ∀ {s : St} {p : Nat}, R s s [.endorse p none]
  act     : ∀ {s : St} {t id : Nat} {v : Val}, S.effect t ∈ c.grants id → id ∉ s.used →
    R s ⟨id :: s.used, s.spent + S.acost t, s.calls, actW S t id v s.w, actC S t id v s.comp, s.next,
      s.keys⟩ (actEvs S t id v)
  drop    : ∀ {s : St} {id : Nat}, id ∉ s.used →
    R s ⟨id :: s.used, s.spent, s.calls, s.w, s.comp, s.next, s.keys⟩ []
  infer   : ∀ {s : St} {m : Nat}, R s ⟨s.used, s.spent + S.mcost m, s.calls + 1, s.w, s.comp, s.next, s.keys⟩ []
  commit  : sagas → ∀ {s s' : St} {t : List Ev}, R { s with comp := [] } s' t →
    R s { s' with comp := s.comp } t
  /-- a revogação na saída da saga: identidades novas, sem repetição -/
  revoke  : sagas → ∀ {s s' : St} {t : List Ev} (ids : List Nat), ids.Nodup → (∀ x ∈ ids, x ∉ s'.used) →
    R s s' t → R s { s' with used := ids ++ s'.used } t
  abort   : sagas → ∀ {s s₁ : St} {t : List Ev} (q : Cause) (r : Nat) (p : Bool), R { s with comp := [] } s₁ t →
    R s { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp }
      (t ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++ [.recover r p])
  /-- a chamada com chave aplicada na hora: o efeito e o fechamento `Done`. A autoridade é
  `id`; a identidade, a chave `kid`, que nunca foi usada nem aposentada -/
  callDone : keys → ∀ {s : St} {t id kid : Nat} {v : Val}, S.effect t ∈ c.grants id → id ∉ s.used →
    kid ∉ id :: s.used → kid ∉ s.keys →
    R s (callSt S t id kid v s true) [.act t id v, .settle kid .done]
  /-- a chamada recusada na hora: `Failed` -/
  callFailed : keys → ∀ {s : St} {t id kid : Nat}, S.effect t ∈ c.grants id → id ∉ s.used →
    kid ∉ id :: s.used → kid ∉ s.keys →
    R s (callSt0 S t id kid s) [.settle kid .failed]
  /-- `TimeoutUnknown` seguido de reconciliação conclusiva -/
  callRec : keys → ∀ {s : St} {t id kid n : Nat} {v : Val} {e : KEnd}, S.effect t ∈ c.grants id →
    id ∉ s.used → kid ∉ id :: s.used → kid ∉ s.keys →
    (reconcile S t kid 1 n).2 = e → e ≠ .transferred →
    R s (recSt S t id kid v s e)
      (.pending t kid v :: ((reconcile S t kid 1 n).1 ++ [.settle kid e]))
  /-- `TimeoutUnknown`, combustível esgotado e o tratador, que transferiu a obrigação -/
  callAbort : keys → ∀ {s s₃ : St} {t id kid n : Nat} {v : Val} {tr : List Ev}, S.effect t ∈ c.grants id →
    id ∉ s.used → kid ∉ id :: s.used → kid ∉ s.keys → (reconcile S t kid 1 n).2 = .transferred →
    R (callSt S t id kid v s (S.applied t kid)) s₃ tr → transferOnly kid tr = true →
    R s s₃ (.pending t kid v :: ((reconcile S t kid 1 n).1 ++ (tr ++ [.settle kid .transferred])))
  /-- `mint`: a chave fresca no corpo e, na saída, aposentada -/
  mint : ∀ {s s' : St} {t : List Ev}, s.next ∉ s.used → s.next ∉ s.keys →
    R { s with next := s.next + 1 } s' t → R s (retire s.next s') (.mint s.next :: t)
  /-- `transfer`: a entrega da obrigação, sem efeito -/
  transfer : ∀ {s : St} {k : Nat}, R s s [.handoff k]

section
variable {S : Sig} {c : Cfg} {sagas keys : Prop} {R : St → St → List Ev → Prop}

theorem loop_preserves (hR : Preserves S c sagas keys R) {f : Val → St → Except Err Out}
    (hf : ∀ v s, (∀ {o : Out}, f v s = .ok o → R s o.st o.tr) ∧
      (∀ {q s₁ tr}, f v s = .error (.timeout q s₁ tr) → R s s₁ tr)) :
    ∀ (vs : List Val) (s : St),
      (∀ {r}, loop f vs s = .ok r → R s r.1 r.2) ∧
      (∀ {q s₁ tr}, loop f vs s = .error (.timeout q s₁ tr) → R s s₁ tr)
  | [], s => ⟨fun h => by simp [loop] at h; subst h; exact hR.refl s, fun h => by simp [loop] at h⟩
  | v :: vs, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [loop, bnd_eq_ok] at h
      obtain ⟨o, ho, r', hr', h⟩ := h
      cases h
      exact hR.trans ((hf v s).1 ho) ((loop_preserves hR hf vs o.st).1 hr')
    · simp only [loop] at h
      rcases bnd_eq_timeout h with h₀ | ⟨o, t, ho, h₁, rfl⟩
      · exact (hf v s).2 h₀
      · rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · exact hR.trans ((hf v s).1 ho) ((loop_preserves hR hf vs o.st).2 h₂)
        · cases h₃

/-- Toda relação preservada vale em toda execução de uma expressão admissível, no sucesso e no
`timeout`. -/
theorem eval_preserves (hR : Preserves S c sagas keys R) : ∀ (e : Exp), Adm sagas keys e → ∀ {ρ : List Val} {s : St},
    (∀ {o : Out}, eval S c e ρ s = .ok o → R s o.st o.tr) ∧
    (∀ {q s₁ tr}, eval S c e ρ s = .error (.timeout q s₁ tr) → R s s₁ tr)
  | .val _, hadm, ρ, s => ⟨fun h => by simp [eval] at h; subst h; exact hR.refl s,
      fun h => by simp [eval] at h⟩
  | .var k, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩ <;> simp only [eval] at h <;> split at h <;>
      first | (cases h; exact hR.refl s) | cases h
  | .let_ e₁ e₂, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨o₁, h₁, o₂, h₂, h⟩ := h
      cases h
      exact hR.trans ((eval_preserves hR e₁ (hadm.mono (by simp_all [sagaFree, keyless]))).1 h₁) ((eval_preserves hR e₂ (hadm.mono (by simp_all [sagaFree, keyless]))).1 h₂)
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨o₁, t, h₁, h₂, rfl⟩
      · exact (eval_preserves hR e₁ (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₀
      · rcases bnd_eq_timeout h₂ with h₃ | ⟨_, _, _, h₄, _⟩
        · exact hR.trans ((eval_preserves hR e₁ (hadm.mono (by simp_all [sagaFree, keyless]))).1 h₁) ((eval_preserves hR e₂ (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₃)
        · cases h₄
  | .ite cnd t f, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨oc, hc, h⟩ := h
      split at h
      · obtain ⟨o', h', h⟩ := bnd_eq_ok.mp h
        cases h
        rcases cond_ok h' with ⟨_, h'⟩ | ⟨_, h'⟩
        · exact hR.trans ((eval_preserves hR cnd (hadm.mono (by simp_all [sagaFree, keyless]))).1 hc) ((eval_preserves hR t (hadm.mono (by simp_all [sagaFree, keyless]))).1 h')
        · exact hR.trans ((eval_preserves hR cnd (hadm.mono (by simp_all [sagaFree, keyless]))).1 hc) ((eval_preserves hR f (hadm.mono (by simp_all [sagaFree, keyless]))).1 h')
      · cases h
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨oc, tr, hc, h₁, rfl⟩
      · exact (eval_preserves hR cnd (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₀
      · revert h₁; split
        · intro h₁
          rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
          · rcases cond_eq h₂ with ⟨_, h₄⟩ | ⟨_, h₄⟩
            · exact hR.trans ((eval_preserves hR cnd (hadm.mono (by simp_all [sagaFree, keyless]))).1 hc) ((eval_preserves hR t (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₄)
            · exact hR.trans ((eval_preserves hR cnd (hadm.mono (by simp_all [sagaFree, keyless]))).1 hc) ((eval_preserves hR f (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₄)
          · cases h₃
        · intro h₁; cases h₁
  | .prim op a b, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨o₁, h₁, o₂, h₂, h⟩ := h
      split at h
      · cases h; exact hR.trans ((eval_preserves hR a (hadm.mono (by simp_all [sagaFree, keyless]))).1 h₁) ((eval_preserves hR b (hadm.mono (by simp_all [sagaFree, keyless]))).1 h₂)
      · cases h
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨o₁, t, h₁, h₂, rfl⟩
      · exact (eval_preserves hR a (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₀
      · rcases bnd_eq_timeout h₂ with h₃ | ⟨o₂, _, _, h₄, _⟩
        · exact hR.trans ((eval_preserves hR a (hadm.mono (by simp_all [sagaFree, keyless]))).1 h₁) ((eval_preserves hR b (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₃)
        · revert h₄; split <;> intro h₄ <;> cases h₄
  | .endorse p e d, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨oe, he, h⟩ := h
      split at h
      · rename_i hp
        cases h
        exact hR.trans ((eval_preserves hR e (hadm.mono (by simp_all [sagaFree, keyless]))).1 he) (hR.endorse hp)
      · obtain ⟨od, hd, h⟩ := bnd_eq_ok.mp (pre_eq_ok.mp h)
        cases h
        exact hR.trans ((eval_preserves hR e (hadm.mono (by simp_all [sagaFree, keyless]))).1 he) (hR.trans hR.reject ((eval_preserves hR d (hadm.mono (by simp_all [sagaFree, keyless]))).1 hd))
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨oe, t, he, h₁, rfl⟩
      · exact (eval_preserves hR e (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₀
      · revert h₁; split
        · intro h₁; cases h₁
        · intro h₁
          obtain ⟨t', h₂, rfl⟩ := pre_eq_timeout h₁
          rcases bnd_eq_timeout h₂ with h₃ | ⟨_, _, _, h₄, _⟩
          · exact hR.trans ((eval_preserves hR e (hadm.mono (by simp_all [sagaFree, keyless]))).1 he) (hR.trans hR.reject ((eval_preserves hR d (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₃))
          · cases h₄
  | .act t k a, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨oa, ha, h⟩ := h
      obtain ⟨id, _, hg, hfree, _, rfl⟩ := doAct_ok h
      exact hR.trans ((eval_preserves hR a (hadm.mono (by simp_all [sagaFree, keyless]))).1 ha) (hR.act hg hfree)
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨_, _, _, h₁, _⟩
      · exact (eval_preserves hR a (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₀
      · exact absurd h₁ doAct_ne_timeout
  | .drop k, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval] at h
      obtain ⟨id, _, hfree, rfl⟩ := doDrop_ok h
      exact hR.drop hfree
    · simp only [eval] at h
      exact absurd h doDrop_ne_timeout
  | .for_ n xs body, hadm, ρ, s => by
    have hl := loop_preserves hR (f := fun v s' => eval S c body (v :: ρ) s')
      (fun v s' => eval_preserves hR body (hadm.mono (by simp_all [sagaFree, keyless])))
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨ox, hx, h⟩ := h
      split at h
      · obtain ⟨r, hr, h⟩ := bnd_eq_ok.mp h
        cases h
        exact hR.trans ((eval_preserves hR xs (hadm.mono (by simp_all [sagaFree, keyless]))).1 hx) ((hl _ _).1 hr)
      · cases h
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨ox, t, hx, h₁, rfl⟩
      · exact (eval_preserves hR xs (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₀
      · revert h₁; split
        · intro h₁
          rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
          · exact hR.trans ((eval_preserves hR xs (hadm.mono (by simp_all [sagaFree, keyless]))).1 hx) ((hl _ _).2 h₂)
          · cases h₃
        · intro h₁; cases h₁
  | .infer m t a, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨oa, ha, h⟩ := h
      obtain ⟨_, rfl⟩ := doInfer_ok h
      have := hR.trans ((eval_preserves hR a (hadm.mono (by simp_all [sagaFree, keyless]))).1 ha) (hR.infer (m := m))
      simpa using this
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨_, _, _, h₁, _⟩
      · exact (eval_preserves hR a (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₀
      · exact absurd h₁ doInfer_ne_timeout
  | .await q, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩ <;> simp only [eval] at h <;> split at h
    · cases h; exact hR.refl s
    · cases h
    · cases h
    · cases h; exact hR.refl s
  | .atomic body r f g, hadm, ρ, s => by
    have hsg : sagas := hadm.1.resolve_right (by simp [sagaFree])
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval] at h
      split at h
      · rename_i o hb
        cases h
        exact hR.revoke hsg _ (revoke_nodup _ _ _) (fun x hx => (mem_revoke.mp hx).2)
          (hR.commit hsg ((eval_preserves hR body (hadm.mono (by simp_all [sagaFree, keyless]))).1 hb))
      · rename_i q s₁ tr hb
        split at h
        · simp [pre] at h
        obtain ⟨o', h', h⟩ := bnd_eq_ok.mp (pre_eq_ok.mp h)
        cases h
        have ha := hR.abort hsg (s := s) q r (S.probe r (compensate S s₁.comp s₁.w).w)
          ((eval_preserves hR body (hadm.mono (by simp_all [sagaFree, keyless]))).2 hb)
        refine hR.revoke hsg _ (revoke_nodup _ _ _) (fun x hx => (mem_revoke.mp hx).2) ?_
        rcases cond_ok h' with ⟨_, h'⟩ | ⟨_, h'⟩
        · exact hR.trans ha ((eval_preserves hR f (hadm.mono (by simp_all [sagaFree, keyless]))).1 h')
        · exact hR.trans ha ((eval_preserves hR g (hadm.mono (by simp_all [sagaFree, keyless]))).1 h')
      · cases h
    · simp only [eval] at h
      split at h
      · cases h
      · rename_i q s₁ tr hb
        have ha := hR.abort hsg (s := s) q r (S.probe r (compensate S s₁.comp s₁.w).w)
          ((eval_preserves hR body (hadm.mono (by simp_all [sagaFree, keyless]))).2 hb)
        split at h
        · simp only [pre] at h
          cases h
          simpa using ha
        obtain ⟨t', h₁, rfl⟩ := pre_eq_timeout h
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · rcases cond_eq h₂ with ⟨_, h₄⟩ | ⟨_, h₄⟩
          · exact hR.trans ha ((eval_preserves hR f (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₄)
          · exact hR.trans ha ((eval_preserves hR g (hadm.mono (by simp_all [sagaFree, keyless]))).2 h₄)
        · cases h₃
      · rename_i hne _
        cases h
        exact absurd rfl (hne _ _ _)
  | .call t j κ a n e, hadm, ρ, s => by
    have hsg : keys := hadm.2.resolve_right (by simp [keyless])
    have iha := eval_preserves hR a (hadm.mono (by simp_all [sagaFree, keyless])) (ρ := ρ) (s := s)
    have ihe : ∀ {ρ' : List Val} {s' : St} {o' : Out}, eval S c e ρ' s' = .ok o' → R s' o'.st o'.tr :=
      fun h => (eval_preserves hR e (hadm.mono (by simp_all [sagaFree, keyless]))).1 h
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨oa, ha, h⟩ := h
      obtain ⟨id, kid, _, _, hg, hfree, hk₁, hk₂, _, hcase⟩ := doCall_ok h
      rcases hcase with ⟨_, rfl⟩ | ⟨_, hr, rfl⟩
      · exact hR.trans (iha.1 ha) (hR.callDone hsg hg hfree hk₁ hk₂)
      · exact hR.trans (iha.1 ha) (hR.callRec hsg hg hfree hk₁ hk₂ hr (by simp))
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨oa, t', ha, h₁, rfl⟩
      · exact iha.2 h₀
      · obtain ⟨id, kid, _, _, hg, hfree, hk₁, hk₂, _, hcase⟩ := doCall_timeout h₁
        rcases hcase with ⟨_, _, rfl, rfl⟩ | ⟨_, hr, _, rfl, rfl⟩ | ⟨_, hr, oh, hoh, hto, _, rfl, rfl⟩
        · exact hR.trans (iha.1 ha) (hR.callFailed hsg hg hfree hk₁ hk₂)
        · exact hR.trans (iha.1 ha) (hR.callRec hsg hg hfree hk₁ hk₂ hr (by simp))
        · exact hR.trans (iha.1 ha) (hR.callAbort hsg hg hfree hk₁ hk₂ hr (ihe hoh) hto)
  | .mint e, hadm, ρ, s => by
    have ih := eval_preserves hR e (hadm.mono (by simp_all [sagaFree, keyless]))
      (ρ := .cap s.next :: ρ) (s := { s with next := s.next + 1 })
    refine ⟨fun h => ?_, fun h => ?_⟩
    · obtain ⟨h₁, h₂, o', ho, rfl⟩ := eval_mint_ok h
      exact hR.mint h₁ h₂ (ih.1 ho)
    · obtain ⟨h₁, h₂, s', tr', ho, rfl, rfl⟩ := eval_mint_timeout h
      exact hR.mint h₁ h₂ (ih.2 ho)
  | .transfer p, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩ <;> simp only [eval] at h <;> split at h
    · cases h; exact hR.transfer
    · cases h
    · cases h
    · cases h

end

end Aletheia
