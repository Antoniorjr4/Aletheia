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

structure Out where
  v  : Val
  st : St
  tr : List Ev

inductive Err where
  | stuck | noCap | reuse | budget
  /-- a pós-condição `q` não foi observada; o estado e os eventos até a falha -/
  | timeout (q : Nat) (s : St) (tr : List Ev)

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

theorem pre_eq_timeout {α : Type} {tr : List Ev} {r : Except Err α} {q : Nat} {s : St}
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
    {q : Nat} {s : St} {tr : List Ev} (h : bnd r f = .error (.timeout q s tr)) :
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

/-- O efeito de `act t`, depois de avaliado o argumento (saída `o`), com a autoridade `cv`.
A guarda confere, nesta ordem, a concessão do runtime para o efeito da ferramenta, a
linearidade e a reserva de orçamento. Só então o evento é emitido. -/
def doAct (S : Sig) (c : Cfg) (t : Nat) (cv : Option Val) (o : Out) : Except Err Out :=
  match cv with
  | some (.cap id) =>
    if S.effect t ∈ c.grants id then
      if id ∈ o.st.used then .error .reuse
      else if ble (o.st.spent + S.cost t) c.limit then
        .ok ⟨.unit, ⟨id :: o.st.used, o.st.spent + S.cost t, o.st.calls, S.step t o.v o.st.w,
          (t, o.v) :: o.st.comp⟩, o.tr ++ [.act t id o.v]⟩
      else .error .budget
    else .error .noCap
  | _ => .error .noCap

/-- `drop k`: consome a capacidade sem efeito e sem evento. -/
def doDrop (cv : Option Val) (s : St) : Except Err Out :=
  match cv with
  | some (.cap id) =>
    if id ∈ s.used then .error .reuse
    else .ok ⟨.unit, ⟨id :: s.used, s.spent, s.calls, s.w, s.comp⟩, []⟩
  | _ => .error .noCap

/-- O efeito de `infer m t`, depois de avaliado o prompt (saída `o`). -/
def doInfer (S : Sig) (c : Cfg) (m : Nat) (t : Ty) (o : Out) : Except Err Out :=
  if ble (o.st.spent + S.mcost m) c.limit then
    .ok ⟨c.oracle o.st.calls m t o.v, ⟨o.st.used, o.st.spent + S.mcost m, o.st.calls + 1, o.st.w,
      o.st.comp⟩, o.tr⟩
  else .error .budget

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
    if S.probe q s.w then .ok ⟨.unit, s, []⟩ else .error (.timeout q s [])
  | .atomic body r f g, ρ, s =>
    match eval S c body ρ { s with comp := [] } with
    | .ok o =>
      -- commit: os efeitos ficam; o registro da saga é descartado, como no ARX
      .ok ⟨o.v, { o.st with comp := s.comp }, o.tr⟩
    | .error (.timeout q s₁ tr) =>
      let k := compensate S s₁.comp s₁.w
      let s₂ : St := { s₁ with w := k.w, comp := s.comp }
      let p := S.probe r k.w
      let evs := tr ++ .fail q :: k.evs ++ [.recover r p]
      pre evs (bnd (cond (k.ok && p) (eval S c f ρ s₂) (eval S c g ρ s₂)) fun o' =>
        .ok ⟨o'.v, o'.st, evs ++ o'.tr⟩)
    | .error e => .error e

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
      ble (o.st.spent + S.cost t) c.limit = true ∧
      r = ⟨.unit, ⟨id :: o.st.used, o.st.spent + S.cost t, o.st.calls, S.step t o.v o.st.w,
        (t, o.v) :: o.st.comp⟩, o.tr ++ [.act t id o.v]⟩ := by
  unfold doAct at h
  split at h
  · rename_i id
    by_cases hg : S.effect t ∈ c.grants id
    · by_cases hu : id ∈ o.st.used
      · simp [hg, hu] at h
      · by_cases hb : ble (o.st.spent + S.cost t) c.limit = true
        · simp [hg, hu, hb] at h
          exact ⟨id, rfl, hg, hu, hb, h.symm⟩
        · simp [hg, hu, hb] at h
    · simp [hg] at h
  · simp at h

theorem doDrop_ok {cv : Option Val} {s : St} {r : Out} (h : doDrop cv s = .ok r) :
    ∃ id, cv = some (.cap id) ∧ id ∉ s.used ∧
      r = ⟨.unit, ⟨id :: s.used, s.spent, s.calls, s.w, s.comp⟩, []⟩ := by
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
        o.st.comp⟩, o.tr⟩ := by
  unfold doInfer at h
  by_cases hb : ble (o.st.spent + S.mcost m) c.limit = true
  · simp [hb] at h; exact ⟨hb, h.symm⟩
  · simp [hb] at h

theorem doAct_ne_timeout {S : Sig} {c : Cfg} {t : Nat} {cv : Option Val} {o : Out} {q : Nat}
    {s : St} {tr : List Ev} : doAct S c t cv o ≠ .error (.timeout q s tr) := by
  unfold doAct; split
  · split
    · split
      · nofun
      · split <;> nofun
    · nofun
  · nofun

theorem doDrop_ne_timeout {cv : Option Val} {s₀ : St} {q : Nat} {s : St} {tr : List Ev} :
    doDrop cv s₀ ≠ .error (.timeout q s tr) := by
  unfold doDrop; split
  · split <;> nofun
  · nofun

theorem doInfer_ne_timeout {S : Sig} {c : Cfg} {m : Nat} {t : Ty} {o : Out} {q : Nat} {s : St}
    {tr : List Ev} : doInfer S c m t o ≠ .error (.timeout q s tr) := by
  unfold doInfer; split <;> nofun

/-- Sem `atomic`: o fragmento em que uma saga não aninha outra. -/
def sagaFree : Exp → Bool
  | .let_ a b | .prim _ a b => sagaFree a && sagaFree b
  | .ite x t f => sagaFree x && sagaFree t && sagaFree f
  | .endorse _ e d => sagaFree e && sagaFree d
  | .act _ _ a | .infer _ _ a => sagaFree a
  | .for_ _ xs body => sagaFree xs && sagaFree body
  | .atomic .. => false
  | .val _ | .var _ | .drop _ | .await _ => true

/-- `e` é admissível para um invariante: o invariante cobre sagas (`sagas`), ou `e` não
tem `atomic`. -/
def Adm (sagas : Prop) (e : Exp) : Prop := sagas ∨ sagaFree e = true

theorem Adm.mono {P : Prop} {e e' : Exp} (h : Adm P e) (hs : sagaFree e = true → sagaFree e' = true) :
    Adm P e' := h.imp id hs

/-! ## Invariantes nos dois desfechos

Uma relação `R s s' tr` entre o estado de partida, o estado de chegada e os eventos
produzidos é **preservada** se vale para cada passo primitivo e se compõe. Então ela vale
para toda execução, tanto no sucesso quanto no `timeout` (com o estado e os eventos que a
saga captura). As propriedades por construção do avaliador (capacidades nunca reusadas,
efeitos sempre concedidos, endossos sempre pela política) são instâncias disto, com
`sagas = True`. Um invariante que só vale fora de sagas aninhadas (o registro de compensação
do T6) usa `sagas = False`: os campos `commit` e `abort` ficam vazios, e o teorema vale para
expressões sem `atomic`. -/

structure Preserves (S : Sig) (c : Cfg) (sagas : Prop) (R : St → St → List Ev → Prop) : Prop where
  refl    : ∀ s, R s s []
  trans   : ∀ {s₁ s₂ s₃ : St} {t₁ t₂ : List Ev}, R s₁ s₂ t₁ → R s₂ s₃ t₂ → R s₁ s₃ (t₁ ++ t₂)
  endorse : ∀ {s : St} {p : Nat} {v : Val}, S.pol p v = true → R s s [.endorse p (some v)]
  reject  : ∀ {s : St} {p : Nat}, R s s [.endorse p none]
  act     : ∀ {s : St} {t id : Nat} {v : Val}, S.effect t ∈ c.grants id → id ∉ s.used →
    R s ⟨id :: s.used, s.spent + S.cost t, s.calls, S.step t v s.w, (t, v) :: s.comp⟩ [.act t id v]
  drop    : ∀ {s : St} {id : Nat}, id ∉ s.used → R s ⟨id :: s.used, s.spent, s.calls, s.w, s.comp⟩ []
  infer   : ∀ {s : St} {m : Nat}, R s ⟨s.used, s.spent + S.mcost m, s.calls + 1, s.w, s.comp⟩ []
  commit  : sagas → ∀ {s s' : St} {t : List Ev}, R { s with comp := [] } s' t →
    R s { s' with comp := s.comp } t
  abort   : sagas → ∀ {s s₁ : St} {t : List Ev} (q r : Nat) (p : Bool), R { s with comp := [] } s₁ t →
    R s { s₁ with w := (compensate S s₁.comp s₁.w).w, comp := s.comp }
      (t ++ .fail q :: (compensate S s₁.comp s₁.w).evs ++ [.recover r p])

section
variable {S : Sig} {c : Cfg} {sagas : Prop} {R : St → St → List Ev → Prop}

theorem loop_preserves (hR : Preserves S c sagas R) {f : Val → St → Except Err Out}
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
theorem eval_preserves (hR : Preserves S c sagas R) : ∀ (e : Exp), Adm sagas e → ∀ {ρ : List Val} {s : St},
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
      exact hR.trans ((eval_preserves hR e₁ (hadm.mono (by simp_all [sagaFree]))).1 h₁) ((eval_preserves hR e₂ (hadm.mono (by simp_all [sagaFree]))).1 h₂)
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨o₁, t, h₁, h₂, rfl⟩
      · exact (eval_preserves hR e₁ (hadm.mono (by simp_all [sagaFree]))).2 h₀
      · rcases bnd_eq_timeout h₂ with h₃ | ⟨_, _, _, h₄, _⟩
        · exact hR.trans ((eval_preserves hR e₁ (hadm.mono (by simp_all [sagaFree]))).1 h₁) ((eval_preserves hR e₂ (hadm.mono (by simp_all [sagaFree]))).2 h₃)
        · cases h₄
  | .ite cnd t f, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨oc, hc, h⟩ := h
      split at h
      · obtain ⟨o', h', h⟩ := bnd_eq_ok.mp h
        cases h
        rcases cond_ok h' with ⟨_, h'⟩ | ⟨_, h'⟩
        · exact hR.trans ((eval_preserves hR cnd (hadm.mono (by simp_all [sagaFree]))).1 hc) ((eval_preserves hR t (hadm.mono (by simp_all [sagaFree]))).1 h')
        · exact hR.trans ((eval_preserves hR cnd (hadm.mono (by simp_all [sagaFree]))).1 hc) ((eval_preserves hR f (hadm.mono (by simp_all [sagaFree]))).1 h')
      · cases h
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨oc, tr, hc, h₁, rfl⟩
      · exact (eval_preserves hR cnd (hadm.mono (by simp_all [sagaFree]))).2 h₀
      · revert h₁; split
        · intro h₁
          rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
          · rcases cond_eq h₂ with ⟨_, h₄⟩ | ⟨_, h₄⟩
            · exact hR.trans ((eval_preserves hR cnd (hadm.mono (by simp_all [sagaFree]))).1 hc) ((eval_preserves hR t (hadm.mono (by simp_all [sagaFree]))).2 h₄)
            · exact hR.trans ((eval_preserves hR cnd (hadm.mono (by simp_all [sagaFree]))).1 hc) ((eval_preserves hR f (hadm.mono (by simp_all [sagaFree]))).2 h₄)
          · cases h₃
        · intro h₁; cases h₁
  | .prim op a b, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨o₁, h₁, o₂, h₂, h⟩ := h
      split at h
      · cases h; exact hR.trans ((eval_preserves hR a (hadm.mono (by simp_all [sagaFree]))).1 h₁) ((eval_preserves hR b (hadm.mono (by simp_all [sagaFree]))).1 h₂)
      · cases h
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨o₁, t, h₁, h₂, rfl⟩
      · exact (eval_preserves hR a (hadm.mono (by simp_all [sagaFree]))).2 h₀
      · rcases bnd_eq_timeout h₂ with h₃ | ⟨o₂, _, _, h₄, _⟩
        · exact hR.trans ((eval_preserves hR a (hadm.mono (by simp_all [sagaFree]))).1 h₁) ((eval_preserves hR b (hadm.mono (by simp_all [sagaFree]))).2 h₃)
        · revert h₄; split <;> intro h₄ <;> cases h₄
  | .endorse p e d, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨oe, he, h⟩ := h
      split at h
      · rename_i hp
        cases h
        exact hR.trans ((eval_preserves hR e (hadm.mono (by simp_all [sagaFree]))).1 he) (hR.endorse hp)
      · obtain ⟨od, hd, h⟩ := bnd_eq_ok.mp (pre_eq_ok.mp h)
        cases h
        exact hR.trans ((eval_preserves hR e (hadm.mono (by simp_all [sagaFree]))).1 he) (hR.trans hR.reject ((eval_preserves hR d (hadm.mono (by simp_all [sagaFree]))).1 hd))
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨oe, t, he, h₁, rfl⟩
      · exact (eval_preserves hR e (hadm.mono (by simp_all [sagaFree]))).2 h₀
      · revert h₁; split
        · intro h₁; cases h₁
        · intro h₁
          obtain ⟨t', h₂, rfl⟩ := pre_eq_timeout h₁
          rcases bnd_eq_timeout h₂ with h₃ | ⟨_, _, _, h₄, _⟩
          · exact hR.trans ((eval_preserves hR e (hadm.mono (by simp_all [sagaFree]))).1 he) (hR.trans hR.reject ((eval_preserves hR d (hadm.mono (by simp_all [sagaFree]))).2 h₃))
          · cases h₄
  | .act t k a, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨oa, ha, h⟩ := h
      obtain ⟨id, _, hg, hfree, _, rfl⟩ := doAct_ok h
      exact hR.trans ((eval_preserves hR a (hadm.mono (by simp_all [sagaFree]))).1 ha) (hR.act hg hfree)
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨_, _, _, h₁, _⟩
      · exact (eval_preserves hR a (hadm.mono (by simp_all [sagaFree]))).2 h₀
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
      (fun v s' => eval_preserves hR body (hadm.mono (by simp_all [sagaFree])))
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨ox, hx, h⟩ := h
      split at h
      · obtain ⟨r, hr, h⟩ := bnd_eq_ok.mp h
        cases h
        exact hR.trans ((eval_preserves hR xs (hadm.mono (by simp_all [sagaFree]))).1 hx) ((hl _ _).1 hr)
      · cases h
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨ox, t, hx, h₁, rfl⟩
      · exact (eval_preserves hR xs (hadm.mono (by simp_all [sagaFree]))).2 h₀
      · revert h₁; split
        · intro h₁
          rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
          · exact hR.trans ((eval_preserves hR xs (hadm.mono (by simp_all [sagaFree]))).1 hx) ((hl _ _).2 h₂)
          · cases h₃
        · intro h₁; cases h₁
  | .infer m t a, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval, bnd_eq_ok] at h
      obtain ⟨oa, ha, h⟩ := h
      obtain ⟨_, rfl⟩ := doInfer_ok h
      have := hR.trans ((eval_preserves hR a (hadm.mono (by simp_all [sagaFree]))).1 ha) (hR.infer (m := m))
      simpa using this
    · simp only [eval] at h
      rcases bnd_eq_timeout h with h₀ | ⟨_, _, _, h₁, _⟩
      · exact (eval_preserves hR a (hadm.mono (by simp_all [sagaFree]))).2 h₀
      · exact absurd h₁ doInfer_ne_timeout
  | .await q, hadm, ρ, s => by
    refine ⟨fun h => ?_, fun h => ?_⟩ <;> simp only [eval] at h <;> split at h
    · cases h; exact hR.refl s
    · cases h
    · cases h
    · cases h; exact hR.refl s
  | .atomic body r f g, hadm, ρ, s => by
    have hsg : sagas := hadm.resolve_right (by simp [sagaFree])
    refine ⟨fun h => ?_, fun h => ?_⟩
    · simp only [eval] at h
      split at h
      · rename_i o hb
        cases h
        exact hR.commit hsg ((eval_preserves hR body (hadm.mono (by simp_all [sagaFree]))).1 hb)
      · rename_i q s₁ tr hb
        obtain ⟨o', h', h⟩ := bnd_eq_ok.mp (pre_eq_ok.mp h)
        cases h
        have ha := hR.abort hsg (s := s) q r (S.probe r (compensate S s₁.comp s₁.w).w)
          ((eval_preserves hR body (hadm.mono (by simp_all [sagaFree]))).2 hb)
        rcases cond_ok h' with ⟨_, h'⟩ | ⟨_, h'⟩
        · exact hR.trans ha ((eval_preserves hR f (hadm.mono (by simp_all [sagaFree]))).1 h')
        · exact hR.trans ha ((eval_preserves hR g (hadm.mono (by simp_all [sagaFree]))).1 h')
      · cases h
    · simp only [eval] at h
      split at h
      · cases h
      · rename_i q s₁ tr hb
        obtain ⟨t', h₁, rfl⟩ := pre_eq_timeout h
        have ha := hR.abort hsg (s := s) q r (S.probe r (compensate S s₁.comp s₁.w).w)
          ((eval_preserves hR body (hadm.mono (by simp_all [sagaFree]))).2 hb)
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · rcases cond_eq h₂ with ⟨_, h₄⟩ | ⟨_, h₄⟩
          · exact hR.trans ha ((eval_preserves hR f (hadm.mono (by simp_all [sagaFree]))).2 h₄)
          · exact hR.trans ha ((eval_preserves hR g (hadm.mono (by simp_all [sagaFree]))).2 h₄)
        · cases h₃
      · rename_i hne _
        cases h
        exact absurd rfl (hne _ _ _)

end

end Aletheia
