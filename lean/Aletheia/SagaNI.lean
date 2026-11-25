import Aletheia.Noninterference

/-!
# T3 com sagas: não-interferência nos dois desfechos

O T3 de `Noninterference.lean` compara duas execuções bem-sucedidas de um programa sem
sagas. Com sagas, o fluxo de controle passa a depender também do **mundo**: uma pós-condição
(`await`) que falha dispara compensações, e a sonda de recuperação escolhe o tratador. Este
arquivo estende o T3 a todo programa bem tipado, com ou sem sagas.

O modelo de ameaça é o mesmo: as duas execuções partem de ambientes baixo-equivalentes, com
entradas `Untrusted` e oráculos arbitrários. O mundo externo e o registro da saga são do
host, não do atacante, então as duas começam do mesmo mundo (`SameW`). O que o atacante muda
são os dados, e o teorema mostra que eles não alcançam o mundo senão por um endosso.

A comparação é entre **desfechos** (`NIR`): um valor ou um `timeout`. Há três casos:

* as duas terminam com valor, com o mesmo traço, o mesmo mundo e o mesmo valor `⊒ Endorsed`;
* as duas param na mesma pós-condição, com o mesmo traço e o mesmo mundo;
* os traços coincidem até uma decisão de endosso e divergem nela.

Não há quarto caso. Em particular, uma execução não confirma uma saga enquanto a outra a
compensa, a menos que antes os traços tenham divergido num endosso: a escolha entre
confirmar e compensar, e entre os tratadores `f` e `g`, depende só do mundo, que dados
`Untrusted` não alcançam.
-/

namespace Aletheia

open Lab Budget Classical

/-! ## Desfechos -/

/-- O traço de um desfecho: o de um valor ou o de um `timeout`. Os outros erros não têm. -/
def rtr {α : Type} [HasTr α] : Except Err α → List Ev
  | .ok a => HasTr.tr a
  | .error (.timeout _ _ tr) => tr
  | .error _ => []

/-- Um desfecho: um valor ou um `timeout`. Os outros erros o T1 exclui. -/
def Settled {α : Type} : Except Err α → Prop
  | .ok _ => True
  | .error (.timeout _ _ _) => True
  | .error _ => False

/-- O que as duas execuções veem do host: o mundo, o registro da saga e o contador de chaves
(o alocador de identidades é do runtime, não do atacante). -/
def SameW (s₁ s₂ : St) : Prop := s₁.w = s₂.w ∧ s₁.comp = s₂.comp ∧ s₁.next = s₂.next

/-- Não-interferência entre dois desfechos. -/
def NIR {α : Type} [HasTr α] (P : α → α → Prop) (r₁ r₂ : Except Err α) : Prop :=
  (∃ a₁ a₂, r₁ = .ok a₁ ∧ r₂ = .ok a₂ ∧ HasTr.tr a₁ = HasTr.tr a₂ ∧ P a₁ a₂) ∨
  (∃ q t₁ t₂ tr, r₁ = .error (.timeout q t₁ tr) ∧ r₂ = .error (.timeout q t₂ tr) ∧ SameW t₁ t₂) ∨
  Diverge (rtr r₁) (rtr r₂)

/-- Duas saídas concordam no mundo e, se `ι ⊒ Endorsed`, no valor. -/
def OutEq (ι : Lab) (o₁ o₂ : Out) : Prop := SameW o₁.st o₂.st ∧ (low ι → o₁.v = o₂.v)

theorem NIR.mono {α : Type} [HasTr α] {P Q : α → α → Prop} {r₁ r₂ : Except Err α}
    (h : NIR P r₁ r₂) (hPQ : ∀ a₁ a₂, P a₁ a₂ → Q a₁ a₂) : NIR Q r₁ r₂ := by
  rcases h with ⟨a₁, a₂, h₁, h₂, ht, hp⟩ | h | h
  · exact .inl ⟨a₁, a₂, h₁, h₂, ht, hPQ _ _ hp⟩
  · exact .inr (.inl h)
  · exact .inr (.inr h)

theorem Settled.of_bnd {α β : Type} [HasTr α] {r : Except Err α} {K : α → Except Err β}
    (h : Settled (bnd r K)) : Settled r := by
  cases r with
  | ok _ => trivial
  | error e => cases e <;> first | trivial | exact h

theorem Settled.of_pre {α : Type} {t : List Ev} {r : Except Err α} (h : Settled (pre t r)) :
    Settled r := by
  cases r with
  | ok _ => trivial
  | error e => cases e <;> first | trivial | exact h

/-! ## Extensão de um desfecho

`Ext t g r r'`: `r'` é `r` com `t` à frente do traço e a saída transformada por `g`. É a
forma de toda continuação do avaliador depois que a primeira parte produziu um valor. -/

def Ext {α β : Type} [HasTr α] [HasTr β] (t : List Ev) (g : α → β) (r : Except Err α)
    (r' : Except Err β) : Prop :=
  (∀ b, r = .ok b → Settled r' → r' = .ok (g b) ∧ HasTr.tr (g b) = t ++ HasTr.tr b) ∧
  (∀ q s tr, r = .error (.timeout q s tr) → r' = .error (.timeout q s (t ++ tr))) ∧
  (Settled r' → Settled r)

theorem Ext.rtr {α β : Type} [HasTr α] [HasTr β] {t : List Ev} {g : α → β}
    {r : Except Err α} {r' : Except Err β} (h : Ext t g r r') (hs : Settled r') :
    Aletheia.rtr r' = t ++ Aletheia.rtr r := by
  have hr := h.2.2 hs
  cases r with
  | ok b =>
    obtain ⟨he, ht⟩ := h.1 b rfl hs
    rw [he]; exact ht
  | error e =>
    cases e with
    | timeout q s tr => rw [h.2.1 q s tr rfl]; rfl
    | stuck => exact (hr : False).elim
    | noCap => exact (hr : False).elim
    | reuse => exact (hr : False).elim
    | budget => exact (hr : False).elim

theorem ext_pre_bnd' {α β : Type} [HasTr α] [HasTr β] {t : List Ev} {g : α → β}
    {K : α → Except Err β} (R : Except Err α) (hK : ∀ b, K b = .ok (g b) ∨ K b = .error .stuck)
    (hg : ∀ b, HasTr.tr (g b) = t ++ HasTr.tr b) : Ext t g R (pre t (bnd R K)) := by
  refine ⟨fun b hb hs => ?_, fun q s tr hb => ?_, fun hs => (Settled.of_pre hs).of_bnd⟩
  · subst hb
    rcases hK b with h | h
    · refine ⟨?_, hg b⟩
      show pre t (pre (HasTr.tr b) (K b)) = _
      rw [h]; rfl
    · have : Settled (pre t (pre (HasTr.tr b) (K b))) := hs
      rw [h] at this
      exact (this : False).elim
  · subst hb; rfl

theorem ext_pre_bnd {α β : Type} [HasTr α] [HasTr β] {t : List Ev} {g : α → β}
    (R : Except Err α) (hg : ∀ b, HasTr.tr (g b) = t ++ HasTr.tr b) :
    Ext t g R (pre t (bnd R fun b => .ok (g b))) :=
  ext_pre_bnd' R (fun _ => .inl rfl) hg

theorem rtr_pre_bnd {α β : Type} [HasTr α] [HasTr β] {t : List Ev} {g : α → β}
    {R : Except Err α} (hs : Settled (pre t (bnd R fun b => .ok (g b))))
    (hg : ∀ b, HasTr.tr (g b) = t ++ HasTr.tr b) :
    rtr (pre t (bnd R fun b => .ok (g b))) = t ++ rtr R :=
  (ext_pre_bnd R hg).rtr hs

theorem ext_stuck {α β : Type} [HasTr α] [HasTr β] {t : List Ev} {g : α → β} :
    Ext t g (.error .stuck : Except Err α) (.error .stuck : Except Err β) :=
  ⟨fun _ hb _ => (by cases hb), fun _ _ _ hb => (by cases hb), fun hs => hs⟩

/-- Um prefixo `u` (a decisão de um endosso recusado) entre a parte já feita e o resto. -/
theorem ext_pre2 (t u : List Ev) (D : Except Err Out) :
    Ext t (fun b : Out => (⟨b.v, b.st, t ++ b.tr⟩ : Out))
      (pre u (bnd D fun b => .ok ⟨b.v, b.st, u ++ b.tr⟩))
      (pre t (pre u (bnd D fun b => .ok ⟨b.v, b.st, t ++ (u ++ b.tr)⟩))) := by
  refine ⟨fun b hb hs => ?_, fun q s tr hb => ?_, fun hs => ?_⟩
  · cases D with
    | ok d => cases hb; exact ⟨rfl, rfl⟩
    | error e => cases e <;> cases hb
  · cases D with
    | ok d => cases hb
    | error e =>
      cases e with
      | timeout q' s' tr' => cases hb; rfl
      | _ => cases hb
  · cases D with
    | ok d => trivial
    | error e => cases e <;> first | trivial | exact hs

/-- A saída de `act` sem o traço anterior, recolocado à frente. -/
theorem ext_doAct (S : Sig) (c : Cfg) (t : Nat) (cv : Option Val) (o : Out) :
    Ext o.tr (fun b : Out => (⟨b.v, b.st, o.tr ++ b.tr⟩ : Out))
      (doAct S c t cv ⟨o.v, o.st, []⟩) (pre o.tr (doAct S c t cv o)) := by
  unfold doAct
  split
  · rename_i id
    by_cases hg : S.effect t ∈ c.grants id
    · by_cases hu : id ∈ o.st.used
      · simp only [hg, hu, if_true]; exact ⟨fun _ hb _ => (by cases hb), fun _ _ _ hb => (by cases hb), fun hs => hs⟩
      · by_cases hb : ble (o.st.spent + S.acost t) c.limit = true
        · simp only [hg, hu, hb, if_true, if_false]
          exact ⟨fun b h _ => (by cases h; exact ⟨by simp [pre], rfl⟩), fun _ _ _ h => (by cases h),
            fun _ => trivial⟩
        · simp only [hg, hu, hb, if_true, if_false, Bool.false_eq_true]
          exact ⟨fun _ h _ => (by cases h), fun _ _ _ h => (by cases h), fun hs => hs⟩
    · simp only [hg, if_false]
      exact ⟨fun _ h _ => (by cases h), fun _ _ _ h => (by cases h), fun hs => hs⟩
  · exact ⟨fun _ h _ => (by cases h), fun _ _ _ h => (by cases h), fun hs => hs⟩

theorem ext_doInfer (S : Sig) (c : Cfg) (m : Nat) (t : Ty) (o : Out) :
    Ext o.tr (fun b : Out => (⟨b.v, b.st, o.tr ++ b.tr⟩ : Out))
      (doInfer S c m t ⟨o.v, o.st, []⟩) (pre o.tr (doInfer S c m t o)) := by
  unfold doInfer
  by_cases hb : ble (o.st.spent + S.mcost m) c.limit = true
  · simp only [hb, if_true]
    exact ⟨fun b h _ => (by cases h; exact ⟨by simp [pre], rfl⟩), fun _ _ _ h => (by cases h),
      fun _ => trivial⟩
  · simp only [hb, if_false, Bool.false_eq_true]
    exact ⟨fun _ h _ => (by cases h), fun _ _ _ h => (by cases h), fun hs => hs⟩

/-- A saída com o traço `t` à frente. -/
def withTr (t : List Ev) : Except Err Out → Except Err Out
  | .ok b => .ok ⟨b.v, b.st, t ++ b.tr⟩
  | .error e => .error e

/-- `call` só usa o traço anterior para prefixá-lo ao valor; uma falha não o carrega (quem o
prefixa é `bnd`). -/
theorem doCall_withTr (S : Sig) (c : Cfg) (t : Nat) (cv kv : Option Val) (n : Nat)
    (h : Nat → St → Except Err Out) (o : Out) :
    doCall S c t cv kv n h o = withTr o.tr (doCall S c t cv kv n h ⟨o.v, o.st, []⟩) := by
  unfold doCall
  split
  · rename_i id kid
    by_cases hg : S.effect t ∈ c.grants id
    · by_cases hu : id ∈ o.st.used
      · simp [hg, hu, withTr]
      · by_cases hk : kid ∈ id :: o.st.used ∨ kid ∈ o.st.keys
        · simp only [hg, hu, hk, if_true, if_false]; rfl
        · by_cases hb : ble (o.st.spent + S.acost t) c.limit = true
          · simp only [hg, hu, hk, hb, if_true, if_false]
            cases hr : S.reply t kid 0
            · simp [withTr]
            · simp [withTr]
            · cases hrc : (reconcile S t kid 1 n).2
              · simp [withTr]
              · simp [withTr]
              · cases hh : h kid (callSt S t id kid o.v o.st (S.applied t kid))
                · simp [withTr]
                · rename_i oh
                  cases ht : transferOnly kid oh.tr <;> simp [withTr, ht]
          · simp only [hg, hu, hk, hb, if_true, if_false, Bool.false_eq_true]; rfl
    · simp [hg, withTr]
  · rfl

theorem ext_err {t : List Ev} {g : Out → Out} (e : Err) :
    Ext t g (.error e : Except Err Out) (pre t (.error e)) := by
  refine ⟨fun _ hb _ => (by cases hb), fun q s tr hb => ?_, fun hs => Settled.of_pre hs⟩
  cases hb; rfl

theorem ext_doCall (S : Sig) (c : Cfg) (t : Nat) (cv kv : Option Val) (n : Nat)
    (h : Nat → St → Except Err Out) (o : Out) :
    Ext o.tr (fun b : Out => (⟨b.v, b.st, o.tr ++ b.tr⟩ : Out))
      (doCall S c t cv kv n h ⟨o.v, o.st, []⟩) (pre o.tr (doCall S c t cv kv n h o)) := by
  rw [doCall_withTr S c t cv kv n h o]
  cases doCall S c t cv kv n h ⟨o.v, o.st, []⟩ with
  | ok b => exact ⟨fun b' hb _ => (by cases hb; exact ⟨rfl, rfl⟩), fun _ _ _ hb => (by cases hb),
      fun _ => trivial⟩
  | error e => exact ext_err e

/-- Um tratador puro não lê o estado nem a configuração: as duas execuções obtêm dele o mesmo
valor e o mesmo traço, e ele não muda o estado. -/
theorem pure_eval {S : Sig} : ∀ (e : Exp), pureH e = true → ∀ (c₁ c₂ : Cfg) (ρ : List Val)
    (s₁ s₂ : St),
    (∃ v tr, eval S c₁ e ρ s₁ = .ok ⟨v, s₁, tr⟩ ∧ eval S c₂ e ρ s₂ = .ok ⟨v, s₂, tr⟩) ∨
      (eval S c₁ e ρ s₁ = .error .stuck ∧ eval S c₂ e ρ s₂ = .error .stuck) := by
  intro e
  induction e with
  | val v => intro _ c₁ c₂ ρ s₁ s₂; exact .inl ⟨v, [], rfl, rfl⟩
  | var k =>
    intro _ c₁ c₂ ρ s₁ s₂
    cases hk : ρ[k]? with
    | none => exact .inr ⟨by simp [eval, hk], by simp [eval, hk]⟩
    | some v => exact .inl ⟨v, [], by simp [eval, hk], by simp [eval, hk]⟩
  | transfer p =>
    intro _ c₁ c₂ ρ s₁ s₂
    cases hp : ρ[p]? with
    | none => exact .inr ⟨by simp [eval, hp], by simp [eval, hp]⟩
    | some v =>
      cases v with
      | pend t k => exact .inl ⟨.unit, [.handoff k], by simp [eval, hp], by simp [eval, hp]⟩
      | _ => exact .inr ⟨by simp [eval, hp], by simp [eval, hp]⟩
  | let_ a b iha ihb =>
    intro hp c₁ c₂ ρ s₁ s₂
    simp only [pureH, Bool.and_eq_true] at hp
    rcases iha hp.1 c₁ c₂ ρ s₁ s₂ with ⟨v, tr, h₁, h₂⟩ | ⟨h₁, h₂⟩
    · rcases ihb hp.2 c₁ c₂ (v :: ρ) s₁ s₂ with ⟨v', tr', h₁', h₂'⟩ | ⟨h₁', h₂'⟩
      · exact .inl ⟨v', tr ++ tr', by simp [eval, h₁, h₁', pre], by simp [eval, h₂, h₂', pre]⟩
      · exact .inr ⟨by simp [eval, h₁, h₁', pre], by simp [eval, h₂, h₂', pre]⟩
    · exact .inr ⟨by simp [eval, h₁], by simp [eval, h₂]⟩
  | prim op a b iha ihb =>
    intro hp c₁ c₂ ρ s₁ s₂
    simp only [pureH, Bool.and_eq_true] at hp
    rcases iha hp.1 c₁ c₂ ρ s₁ s₂ with ⟨v, tr, h₁, h₂⟩ | ⟨h₁, h₂⟩
    · rcases ihb hp.2 c₁ c₂ ρ s₁ s₂ with ⟨v', tr', h₁', h₂'⟩ | ⟨h₁', h₂'⟩
      · cases v with
        | int x =>
          cases v' with
          | int y =>
            exact .inl ⟨op.apply x y, tr ++ tr', by simp [eval, h₁, h₁', pre],
              by simp [eval, h₂, h₂', pre]⟩
          | _ => exact .inr ⟨by simp [eval, h₁, h₁', pre], by simp [eval, h₂, h₂', pre]⟩
        | _ => exact .inr ⟨by simp [eval, h₁, h₁', pre], by simp [eval, h₂, h₂', pre]⟩
      · exact .inr ⟨by simp [eval, h₁, h₁', pre], by simp [eval, h₂, h₂', pre]⟩
    · exact .inr ⟨by simp [eval, h₁], by simp [eval, h₂]⟩
  | ite x t f ihx iht ihf =>
    intro hp c₁ c₂ ρ s₁ s₂
    simp only [pureH, Bool.and_eq_true] at hp
    rcases ihx hp.1.1 c₁ c₂ ρ s₁ s₂ with ⟨v, tr, h₁, h₂⟩ | ⟨h₁, h₂⟩
    · cases v with
      | bool b =>
        cases b
        · rcases ihf hp.2 c₁ c₂ ρ s₁ s₂ with ⟨v', tr', h₁', h₂'⟩ | ⟨h₁', h₂'⟩
          · exact .inl ⟨v', tr ++ tr', by simp [eval, h₁, h₁', pre], by simp [eval, h₂, h₂', pre]⟩
          · exact .inr ⟨by simp [eval, h₁, h₁', pre], by simp [eval, h₂, h₂', pre]⟩
        · rcases iht hp.1.2 c₁ c₂ ρ s₁ s₂ with ⟨v', tr', h₁', h₂'⟩ | ⟨h₁', h₂'⟩
          · exact .inl ⟨v', tr ++ tr', by simp [eval, h₁, h₁', pre], by simp [eval, h₂, h₂', pre]⟩
          · exact .inr ⟨by simp [eval, h₁, h₁', pre], by simp [eval, h₂, h₂', pre]⟩
      | _ => exact .inr ⟨by simp [eval, h₁, pre], by simp [eval, h₂, pre]⟩
    · exact .inr ⟨by simp [eval, h₁], by simp [eval, h₂]⟩
  | _ => intro hp; simp [pureH] at hp

theorem settled_cases {r : Except Err Out} (hs : Settled r) :
    (∃ o, r = .ok o) ∨ (∃ q s tr, r = .error (.timeout q s tr)) := by
  cases r with
  | ok o => exact .inl ⟨o, rfl⟩
  | error e =>
    cases e with
    | timeout q s tr => exact .inr ⟨q, s, tr, rfl⟩
    | _ => exact (hs : False).elim

/-- Uma chamada com chave, com a mesma capacidade, o mesmo argumento e o mesmo mundo nas duas
execuções, termina igual nas duas: o provedor responde pela chave, a reconciliação depende
só da chave e da classe, e o tratador é puro. -/
theorem doCall_ni {S : Sig} {c₁ c₂ : Cfg} {t n : Nat} {cv kv : Option Val} {e : Exp}
    (hpure : pureH e = true) {o₁ o₂ : Out} (hw : SameW o₁.st o₂.st) (hv : o₁.v = o₂.v)
    (h₁ : Settled (doCall S c₁ t cv kv n (fun id s' => eval S c₁ e [.pend t id] s') ⟨o₁.v, o₁.st, []⟩))
    (h₂ : Settled (doCall S c₂ t cv kv n (fun id s' => eval S c₂ e [.pend t id] s') ⟨o₂.v, o₂.st, []⟩)) :
    NIR (OutEq .T) (doCall S c₁ t cv kv n (fun id s' => eval S c₁ e [.pend t id] s') ⟨o₁.v, o₁.st, []⟩)
      (doCall S c₂ t cv kv n (fun id s' => eval S c₂ e [.pend t id] s') ⟨o₂.v, o₂.st, []⟩) := by
  rcases o₁ with ⟨v, st₁, tr₁⟩
  rcases o₂ with ⟨v', st₂, tr₂⟩
  simp only at hv hw h₁ h₂ ⊢
  subst hv
  have hcs : ∀ (id kid : Nat) (b : Bool), SameW (callSt S t id kid v st₁ b) (callSt S t id kid v st₂ b) :=
    fun id kid b => ⟨by simp [callSt, hw.1], by simp [callSt, hw.2.1], by simp [callSt, hw.2.2]⟩
  have hrs : ∀ (id kid : Nat) (e : KEnd), SameW (recSt S t id kid v st₁ e) (recSt S t id kid v st₂ e) :=
    fun id kid e => ⟨by simp [recSt, hw.1], by simp [recSt, hw.2.1], by simp [recSt, hw.2.2]⟩
  rcases settled_cases h₁ with ⟨r₁, e₁⟩ | ⟨q₁, u₁, t₁, e₁⟩ <;>
    rcases settled_cases h₂ with ⟨r₂, e₂⟩ | ⟨q₂, u₂, t₂, e₂⟩
  · obtain ⟨id, kid, hid, hkid, _, _, _, _, _, hc₁⟩ := doCall_ok e₁
    obtain ⟨id', kid', hid', hkid', _, _, _, _, _, hc₂⟩ := doCall_ok e₂
    rw [hid] at hid'; cases hid'
    rw [hkid] at hkid'; cases hkid'
    rw [e₁, e₂]
    rcases hc₁ with ⟨hr₁, rfl⟩ | ⟨hr₁, _, rfl⟩ <;> rcases hc₂ with ⟨hr₂, rfl⟩ | ⟨hr₂, _, rfl⟩
    · exact .inl ⟨_, _, rfl, rfl, rfl, hcs id kid true, fun _ => rfl⟩
    · rw [hr₁] at hr₂; cases hr₂
    · rw [hr₁] at hr₂; cases hr₂
    · exact .inl ⟨_, _, rfl, rfl, rfl, hcs id kid _, fun _ => rfl⟩
  · exfalso
    obtain ⟨id, kid, hid, hkid, _, _, _, _, _, hc₁⟩ := doCall_ok e₁
    obtain ⟨id', kid', hid', hkid', _, _, _, _, _, hc₂⟩ := doCall_timeout e₂
    rw [hid] at hid'; cases hid'
    rw [hkid] at hkid'; cases hkid'
    rcases hc₁ with ⟨hr₁, _⟩ | ⟨hr₁, hk₁, _⟩ <;>
      rcases hc₂ with ⟨hr₂, _⟩ | ⟨hr₂, hk₂, _⟩ | ⟨hr₂, hk₂, _⟩ <;>
      first | (rw [hr₁] at hr₂; cases hr₂; done) | (rw [hk₁] at hk₂; cases hk₂; done)
  · exfalso
    obtain ⟨id, kid, hid, hkid, _, _, _, _, _, hc₁⟩ := doCall_timeout e₁
    obtain ⟨id', kid', hid', hkid', _, _, _, _, _, hc₂⟩ := doCall_ok e₂
    rw [hid] at hid'; cases hid'
    rw [hkid] at hkid'; cases hkid'
    rcases hc₂ with ⟨hr₂, _⟩ | ⟨hr₂, hk₂, _⟩ <;>
      rcases hc₁ with ⟨hr₁, _⟩ | ⟨hr₁, hk₁, _⟩ | ⟨hr₁, hk₁, _⟩ <;>
      first | (rw [hr₁] at hr₂; cases hr₂; done) | (rw [hk₁] at hk₂; cases hk₂; done)
  · obtain ⟨id, kid, hid, hkid, _, _, _, _, _, hc₁⟩ := doCall_timeout e₁
    obtain ⟨id', kid', hid', hkid', _, _, _, _, _, hc₂⟩ := doCall_timeout e₂
    rw [hid] at hid'; cases hid'
    rw [hkid] at hkid'; cases hkid'
    rw [e₁, e₂]
    rcases hc₁ with ⟨hr₁, rfl, rfl, rfl⟩ | ⟨hr₁, hk₁, rfl, rfl, rfl⟩ |
        ⟨hr₁, hk₁, oh₁, hoh₁, _, rfl, rfl, rfl⟩ <;>
      rcases hc₂ with ⟨hr₂, rfl, rfl, rfl⟩ | ⟨hr₂, hk₂, rfl, rfl, rfl⟩ |
        ⟨hr₂, hk₂, oh₂, hoh₂, _, rfl, rfl, rfl⟩ <;>
      first
      | (rw [hr₁] at hr₂; cases hr₂; done)
      | (rw [hk₁] at hk₂; cases hk₂; done)
      | skip
    · exact .inr (.inl ⟨_, _, _, _, rfl, rfl, hw⟩)
    · exact .inr (.inl ⟨_, _, _, _, rfl, rfl, hrs id kid _⟩)
    · dsimp only at hoh₁ hoh₂
      rcases pure_eval e hpure c₁ c₂ [.pend t kid] (callSt S t id kid v st₁ (S.applied t kid))
          (callSt S t id kid v st₂ (S.applied t kid)) with ⟨vh, trh, hp₁, hp₂⟩ | ⟨hp₁, _⟩
      · rw [hp₁] at hoh₁; rw [hp₂] at hoh₂
        cases hoh₁; cases hoh₂
        exact .inr (.inl ⟨_, _, _, _, rfl, rfl, hcs id kid _⟩)
      · rw [hp₁] at hoh₁; cases hoh₁

/-! ## Composição -/

theorem NIR.ext {α β : Type} [HasTr α] [HasTr β] {Pi : α → α → Prop} {Q : β → β → Prop}
    {t₁ t₂ : List Ev} {g₁ g₂ : α → β} {I₁ I₂ : Except Err α} {r₁ r₂ : Except Err β}
    (h : NIR Pi I₁ I₂) (hE₁ : Ext t₁ g₁ I₁ r₁) (hE₂ : Ext t₂ g₂ I₂ r₂) (ht : t₁ = t₂)
    (hQ : ∀ b₁ b₂, I₁ = .ok b₁ → I₂ = .ok b₂ → HasTr.tr b₁ = HasTr.tr b₂ → Pi b₁ b₂ →
      Q (g₁ b₁) (g₂ b₂))
    (hs₁ : Settled r₁) (hs₂ : Settled r₂) : NIR Q r₁ r₂ := by
  subst ht
  rcases h with ⟨b₁, b₂, h₁, h₂, hb, hp⟩ | ⟨q, u₁, u₂, tr, h₁, h₂, hw⟩ | hd
  · obtain ⟨e₁, x₁⟩ := hE₁.1 b₁ h₁ hs₁
    obtain ⟨e₂, x₂⟩ := hE₂.1 b₂ h₂ hs₂
    exact .inl ⟨_, _, e₁, e₂, by rw [x₁, x₂, hb], hQ b₁ b₂ h₁ h₂ hb hp⟩
  · exact .inr (.inl ⟨q, u₁, u₂, t₁ ++ tr, hE₁.2.1 _ _ _ h₁, hE₂.2.1 _ _ _ h₂, hw⟩)
  · refine .inr (.inr ?_)
    rw [hE₁.rtr hs₁, hE₂.rtr hs₂]
    exact hd.prepend t₁

theorem rtr_bnd {β γ : Type} [HasTr β] [HasTr γ] {r : Except Err Out}
    {K : Out → Except Err β} {I : Out → Except Err γ} {g : Out → γ → β}
    (hE : ∀ a, Ext a.tr (g a) (I a) (pre a.tr (K a))) (hs : Settled (bnd r K)) :
    ∃ X, rtr (bnd r K) = rtr r ++ X := by
  cases r with
  | ok a => exact ⟨_, (hE a).rtr hs⟩
  | error e =>
    cases e with
    | timeout q s tr => exact ⟨[], by simp [rtr]⟩
    | stuck => exact (hs : False).elim
    | noCap => exact (hs : False).elim
    | reuse => exact (hs : False).elim
    | budget => exact (hs : False).elim

/-- Composição sequencial: a primeira parte concorda, para na mesma pós-condição ou já
divergiu num endosso; a continuação herda. -/
theorem NIR.seq {α β : Type} [HasTr α] [HasTr β] {P : Out → Out → Prop} {Pi : α → α → Prop}
    {Q : β → β → Prop} {r₁ r₂ : Except Err Out} {K₁ K₂ : Out → Except Err β}
    {I₁ I₂ : Out → Except Err α} {g₁ g₂ : Out → α → β}
    (h : NIR P r₁ r₂)
    (hE₁ : ∀ a, Ext a.tr (g₁ a) (I₁ a) (pre a.tr (K₁ a)))
    (hE₂ : ∀ a, Ext a.tr (g₂ a) (I₂ a) (pre a.tr (K₂ a)))
    (hI : ∀ a₁ a₂, a₁.tr = a₂.tr → P a₁ a₂ → Settled (I₁ a₁) → Settled (I₂ a₂) →
      NIR Pi (I₁ a₁) (I₂ a₂))
    (hQ : ∀ a₁ a₂ b₁ b₂, P a₁ a₂ → I₁ a₁ = .ok b₁ → I₂ a₂ = .ok b₂ →
      HasTr.tr b₁ = HasTr.tr b₂ → Pi b₁ b₂ → Q (g₁ a₁ b₁) (g₂ a₂ b₂))
    (hs₁ : Settled (bnd r₁ K₁)) (hs₂ : Settled (bnd r₂ K₂)) : NIR Q (bnd r₁ K₁) (bnd r₂ K₂) := by
  rcases h with ⟨a₁, a₂, rfl, rfl, ht, hp⟩ | ⟨q, u₁, u₂, tr, rfl, rfl, hw⟩ | hd
  · have hs₁' : Settled (pre a₁.tr (K₁ a₁)) := hs₁
    have hs₂' : Settled (pre a₂.tr (K₂ a₂)) := hs₂
    exact NIR.ext (hI a₁ a₂ ht hp ((hE₁ a₁).2.2 hs₁') ((hE₂ a₂).2.2 hs₂')) (hE₁ a₁) (hE₂ a₂) ht
      (fun b₁ b₂ h₁ h₂ hb hpi => hQ a₁ a₂ b₁ b₂ hp h₁ h₂ hb hpi) hs₁' hs₂'
  · exact .inr (.inl ⟨q, u₁, u₂, tr, rfl, rfl, hw⟩)
  · refine .inr (.inr ?_)
    obtain ⟨X₁, hX₁⟩ := rtr_bnd hE₁ hs₁
    obtain ⟨X₂, hX₂⟩ := rtr_bnd hE₂ hs₂
    rw [hX₁, hX₂]
    exact hd.append _ _

/-! ## `mint` -/

theorem Settled.of_mintOut {k : Nat} {r : Except Err Out} (h : Settled (mintOut k r)) : Settled r := by
  cases r with
  | ok _ => trivial
  | error e => cases e <;> first | trivial | exact h

theorem SameW.retire {s₁ s₂ : St} (k : Nat) (h : SameW s₁ s₂) : SameW (retire k s₁) (retire k s₂) := h

/-- A cunhagem prefixa o mesmo evento aos dois desfechos e aposenta a mesma chave. -/
theorem NIR.mintOut {ι : Lab} {k : Nat} {r₁ r₂ : Except Err Out} (h : NIR (OutEq ι) r₁ r₂) :
    NIR (OutEq ι) (mintOut k r₁) (mintOut k r₂) := by
  rcases h with ⟨a₁, a₂, rfl, rfl, ht, hw, hv⟩ | ⟨q, u₁, u₂, tr, rfl, rfl, hw⟩ | hd
  · exact .inl ⟨_, _, rfl, rfl, by show Ev.mint k :: a₁.tr = Ev.mint k :: a₂.tr; rw [show a₁.tr = a₂.tr from ht],
      hw.retire k, hv⟩
  · exact .inr (.inl ⟨q, _, _, _, rfl, rfl, hw.retire k⟩)
  · refine .inr (.inr ?_)
    have hrt : ∀ r : Except Err Out, rtr r ≠ [] → rtr (Aletheia.mintOut k r) = .mint k :: rtr r := by
      intro r hne
      cases r with
      | ok o => rfl
      | error e =>
        cases e with
        | timeout q s tr => rfl
        | _ => exact absurd rfl hne
    obtain ⟨pre, p, o₁, o₂, t₁, t₂, _, e₁, e₂⟩ := hd
    have hn₁ : rtr r₁ ≠ [] := by rw [e₁]; simp
    have hn₂ : rtr r₂ ≠ [] := by rw [e₂]; simp
    rw [hrt r₁ hn₁, hrt r₂ hn₂]
    exact (show Diverge (rtr r₁) (rtr r₂) from ⟨pre, p, o₁, o₂, t₁, t₂, ‹_›, e₁, e₂⟩).prepend [.mint k]

/-! ## Laços -/

theorem loop_ni {f₁ f₂ : Val → St → Except Err Out}
    (hf : ∀ v s₁ s₂, SameW s₁ s₂ → Settled (f₁ v s₁) → Settled (f₂ v s₂) →
      NIR (fun o₁ o₂ : Out => SameW o₁.st o₂.st) (f₁ v s₁) (f₂ v s₂)) :
    ∀ (vs : List Val) (s₁ s₂ : St), SameW s₁ s₂ → Settled (loop f₁ vs s₁) →
      Settled (loop f₂ vs s₂) →
      NIR (fun r₁ r₂ : St × List Ev => SameW r₁.1 r₂.1) (loop f₁ vs s₁) (loop f₂ vs s₂)
  | [], s₁, s₂, hw, _, _ => .inl ⟨_, _, rfl, rfl, rfl, hw⟩
  | v :: vs, s₁, s₂, hw, hs₁, hs₂ => by
    simp only [loop] at hs₁ hs₂ ⊢
    exact NIR.seq (hf v s₁ s₂ hw hs₁.of_bnd hs₂.of_bnd)
      (I₁ := fun o => loop f₁ vs o.st) (g₁ := fun o r => (r.1, o.tr ++ r.2))
      (I₂ := fun o => loop f₂ vs o.st) (g₂ := fun o r => (r.1, o.tr ++ r.2))
      (fun _ => ext_pre_bnd _ fun _ => rfl) (fun _ => ext_pre_bnd _ fun _ => rfl)
      (fun o₁ o₂ _ hp h₁ h₂ => loop_ni hf vs o₁.st o₂.st hp h₁ h₂)
      (fun _ _ _ _ _ _ _ _ hpi => hpi) hs₁ hs₂

theorem loop_confine_ok {f : Val → St → Except Err Out}
    (hf : ∀ v (s : St) o, f v s = .ok o → o.tr = [] ∧ o.st.w = s.w ∧ o.st.comp = s.comp) :
    ∀ (vs : List Val) (s : St) r, loop f vs s = .ok r →
      r.2 = [] ∧ r.1.w = s.w ∧ r.1.comp = s.comp
  | [], s, r, h => by simp [loop] at h; subst h; exact ⟨rfl, rfl, rfl⟩
  | v :: vs, s, r, h => by
    simp only [loop, bnd_eq_ok] at h
    obtain ⟨o, ho, r', hr', h⟩ := h
    cases h
    obtain ⟨t₁, w₁, k₁⟩ := hf v s o ho
    obtain ⟨t₂, w₂, k₂⟩ := loop_confine_ok hf vs o.st r' hr'
    exact ⟨by simp [t₁, t₂], w₂.trans w₁, k₂.trans k₁⟩

theorem loop_confine {f : Val → St → Except Err Out}
    (hf : ∀ v (s : St), Settled (f v s) →
      ∃ o, f v s = .ok o ∧ o.tr = [] ∧ o.st.w = s.w ∧ o.st.comp = s.comp ∧ o.st.next = s.next) :
    ∀ (vs : List Val) (s : St), Settled (loop f vs s) →
      ∃ r, loop f vs s = .ok r ∧ r.2 = [] ∧ r.1.w = s.w ∧ r.1.comp = s.comp ∧ r.1.next = s.next
  | [], s, _ => ⟨(s, []), rfl, rfl, rfl, rfl, rfl⟩
  | v :: vs, s, hs => by
    simp only [loop] at hs ⊢
    obtain ⟨o, ho, t₁, w₁, k₁, n₁⟩ := hf v s hs.of_bnd
    rw [ho] at hs ⊢
    have hs' : Settled (bnd (loop f vs o.st) fun r => .ok (r.1, o.tr ++ r.2)) :=
      Settled.of_pre (t := HasTr.tr o) hs
    obtain ⟨r, hr, t₂, w₂, k₂, n₂⟩ := loop_confine hf vs o.st hs'.of_bnd
    exact ⟨(r.1, o.tr ++ r.2), by simp only [bnd_ok, hr, pre_ok], by simp [t₁, t₂],
      w₂.trans w₁, k₂.trans k₁, n₂.trans n₁⟩

theorem loop_no_timeout_all {f : Val → St → Except Err Out}
    (hf : ∀ v s q s₁ tr, f v s ≠ .error (.timeout q s₁ tr)) :
    ∀ (vs : List Val) (s : St) (q : Cause) (s₁ : St) (tr : List Ev),
      loop f vs s ≠ .error (.timeout q s₁ tr)
  | [], s, q, s₁, tr, h => by simp [loop] at h
  | v :: vs, s, q, s₁, tr, h => by
    simp only [loop] at h
    rcases bnd_eq_timeout h with h₀ | ⟨o, t, _, h₁, _⟩
    · exact hf v s q s₁ tr h₀
    · rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
      · exact loop_no_timeout_all hf vs _ q s₁ _ h₂
      · cases h₃

/-! ## Confinamento com o mundo -/

section
variable {S : Sig}

/-- Sob `pc` `Untrusted` não há eventos, o mundo e o registro da saga não mudam, e nenhuma
pós-condição falha (`await` exige `pc ⊒ Endorsed`). -/
theorem confine_w {c : Cfg} {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} (h : HasType S Γ pc e τ ι β) :
    ¬ low pc → ∀ {ρ : List Val} {s : St},
      (∀ o, eval S c e ρ s = .ok o → o.tr = [] ∧ o.st.w = s.w ∧ o.st.comp = s.comp) ∧
      (∀ q s' tr, eval S c e ρ s ≠ .error (.timeout q s' tr)) := by
  induction h with
  | val _ =>
    intro _ ρ s
    exact ⟨fun o he => by simp [eval] at he; subst he; exact ⟨rfl, rfl, rfl⟩,
      fun q s' tr he => by simp [eval] at he⟩
  | var _ =>
    intro _ ρ s
    refine ⟨fun o he => ?_, fun q s' tr he => ?_⟩
    · simp only [eval] at he
      split at he
      · cases he; exact ⟨rfl, rfl, rfl⟩
      · cases he
    · simp only [eval] at he
      split at he <;> cases he
  | let_ _ _ ih₁ ih₂ =>
    intro hpc ρ s
    refine ⟨fun o he => ?_, fun q s' tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨o₁, h₁, o₂, h₂, he⟩ := he
      cases he
      obtain ⟨t₁, w₁, k₁⟩ := (ih₁ hpc).1 o₁ h₁
      obtain ⟨t₂, w₂, k₂⟩ := (ih₂ hpc).1 o₂ h₂
      exact ⟨by simp [t₁, t₂], w₂.trans w₁, k₂.trans k₁⟩
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨o₁, t, _, h₂, _⟩
      · exact (ih₁ hpc).2 _ _ _ h₀
      · rcases bnd_eq_timeout h₂ with h₃ | ⟨_, _, _, h₄, _⟩
        · exact (ih₂ hpc).2 _ _ _ h₃
        · cases h₄
  | @ite Γ pc cnd t f τ ιc ιt ιf βc βt βf _ _ _ ihc iht ihf =>
    intro hpc ρ s
    have hpc' : ¬ low (pc ⊓ ιc) := fun h => hpc (low_meet.mp h).1
    refine ⟨fun o he => ?_, fun q s' tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨oc, hc, he⟩ := he
      split at he
      · obtain ⟨o', h', he⟩ := bnd_eq_ok.mp he
        cases he
        obtain ⟨t₁, w₁, k₁⟩ := (ihc hpc).1 oc hc
        rcases cond_ok h' with ⟨_, h'⟩ | ⟨_, h'⟩
        · obtain ⟨t₂, w₂, k₂⟩ := (iht hpc').1 o' h'
          exact ⟨by simp [t₁, t₂], w₂.trans w₁, k₂.trans k₁⟩
        · obtain ⟨t₂, w₂, k₂⟩ := (ihf hpc').1 o' h'
          exact ⟨by simp [t₁, t₂], w₂.trans w₁, k₂.trans k₁⟩
      · cases he
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨oc, tr', _, h₁, _⟩
      · exact (ihc hpc).2 _ _ _ h₀
      · revert h₁; split
        · intro h₁
          rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
          · rcases cond_eq h₂ with ⟨_, h₄⟩ | ⟨_, h₄⟩
            · exact (iht hpc').2 _ _ _ h₄
            · exact (ihf hpc').2 _ _ _ h₄
          · cases h₃
        · intro h₁; cases h₁
  | prim _ _ iha ihb =>
    intro hpc ρ s
    refine ⟨fun o he => ?_, fun q s' tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨o₁, h₁, o₂, h₂, he⟩ := he
      obtain ⟨t₁, w₁, k₁⟩ := (iha hpc).1 o₁ h₁
      obtain ⟨t₂, w₂, k₂⟩ := (ihb hpc).1 o₂ h₂
      split at he
      · cases he; exact ⟨by simp [t₁, t₂], w₂.trans w₁, k₂.trans k₁⟩
      · cases he
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨o₁, t, _, h₂, _⟩
      · exact (iha hpc).2 _ _ _ h₀
      · rcases bnd_eq_timeout h₂ with h₃ | ⟨_, _, _, h₄, _⟩
        · exact (ihb hpc).2 _ _ _ h₃
        · revert h₄; split <;> intro h₄ <;> cases h₄
  | endorse hl _ _ _ _ => intro hpc; exact absurd hl hpc
  | act hl _ _ _ _ _ => intro hpc; exact absurd hl hpc
  | drop _ =>
    intro _ ρ s
    refine ⟨fun o he => ?_, fun q s' tr he => ?_⟩
    · simp only [eval] at he
      obtain ⟨_, _, _, rfl⟩ := doDrop_ok he
      exact ⟨rfl, rfl, rfl⟩
    · simp only [eval] at he; exact doDrop_ne_timeout he
  | @for_ Γ pc n xs body τ τb ιx ιb βx βb _ _ ihx ihb =>
    intro hpc ρ s
    have hpc' : ¬ low (pc ⊓ ιx) := fun h => hpc (low_meet.mp h).1
    refine ⟨fun o he => ?_, fun q s' tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨ox, hx, he⟩ := he
      split at he
      · obtain ⟨r, hr, he⟩ := bnd_eq_ok.mp he
        cases he
        obtain ⟨t₁, w₁, k₁⟩ := (ihx hpc).1 ox hx
        obtain ⟨t₂, w₂, k₂⟩ := loop_confine_ok (fun v s o ho => (ihb hpc').1 o ho) _ _ r hr
        exact ⟨by simp [t₁, t₂], w₂.trans w₁, k₂.trans k₁⟩
      · cases he
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨ox, t, _, h₁, _⟩
      · exact (ihx hpc).2 _ _ _ h₀
      · revert h₁; split
        · intro h₁
          rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
          · exact loop_no_timeout_all (fun v s' => (ihb hpc').2) _ _ _ _ _ h₂
          · cases h₃
        · intro h₁; cases h₁
  | infer _ iha =>
    intro hpc ρ s
    refine ⟨fun o he => ?_, fun q s' tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨oa, ha, he⟩ := he
      obtain ⟨_, rfl⟩ := doInfer_ok he
      exact (iha hpc).1 oa ha
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨_, _, _, h₁, _⟩
      · exact (iha hpc).2 _ _ _ h₀
      · exact doInfer_ne_timeout h₁
  | await hl => intro hpc; exact absurd hl hpc
  | atomic hl _ _ _ _ _ _ => intro hpc; exact absurd hl hpc
  | call hl _ _ _ _ _ _ _ _ _ => intro hpc; exact absurd hl hpc
  | mint hl _ _ _ => intro hpc; exact absurd hl hpc
  | transfer hl _ => intro hpc; exact absurd hl hpc

theorem confine_settled {c : Cfg} {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} (h : HasType S Γ pc e τ ι β) (hpc : ¬ low pc) {ρ : List Val} {s : St}
    (hs : Settled (eval S c e ρ s)) :
    ∃ o, eval S c e ρ s = .ok o ∧ o.tr = [] ∧ o.st.w = s.w ∧ o.st.comp = s.comp ∧
      o.st.next = s.next := by
  have hc := confine_w (c := c) h hpc (ρ := ρ) (s := s)
  revert hs
  cases hr : eval S c e ρ s with
  | ok o =>
    intro _
    obtain ⟨ht, hw, hk⟩ := hc.1 o hr
    refine ⟨o, rfl, ht, hw, hk, ?_⟩
    have := eval_next e hr
    rw [ht] at this; simpa [mints] using this
  | error err =>
    cases err with
    | timeout q s' tr => exact absurd hr (hc.2 q s' tr)
    | stuck => intro hs; exact (hs : False).elim
    | noCap => intro hs; exact (hs : False).elim
    | reuse => intro hs; exact (hs : False).elim
    | budget => intro hs; exact (hs : False).elim

/-! ## O teorema -/

/-- O valor de uma primitiva, total. -/
def primv (op : Op) : Val → Val → Val
  | .int x, .int y => op.apply x y
  | _, _ => .unit

theorem ok_of_settled {r : Except Err Out} (hs : Settled r)
    (hn : ∀ q s tr, r ≠ .error (.timeout q s tr)) : ∃ o, r = .ok o := by
  cases r with
  | ok o => exact ⟨o, rfl⟩
  | error e =>
    cases e with
    | timeout q s tr => exact absurd rfl (hn q s tr)
    | stuck => exact (hs : False).elim
    | noCap => exact (hs : False).elim
    | reuse => exact (hs : False).elim
    | budget => exact (hs : False).elim

theorem eval_atomic_settled {c : Cfg} {body : Exp} {r : Nat} {f g : Exp} {ρ : List Val} {s : St}
    (hs : Settled (eval S c (.atomic body r f g) ρ s)) :
    Settled (eval S c body ρ { s with comp := [] }) := by
  revert hs
  cases hb : eval S c body ρ { s with comp := [] } with
  | ok o => intro _; trivial
  | error e =>
    cases e with
    | timeout q s' tr => intro _; trivial
    | _ => intro hs; simp only [eval, hb] at hs; exact hs

theorem ex_append (a b c d : List Ev) : ∃ X, ((a ++ b) ++ c) ++ d = a ++ X :=
  ⟨b ++ c ++ d, by simp⟩

/-- O traço de uma saga estende o do seu corpo. -/
theorem atomic_extends {c : Cfg} {body : Exp} {r : Nat} {f g : Exp} {ρ : List Val} {s : St}
    (hs : Settled (eval S c (.atomic body r f g) ρ s)) :
    ∃ X, rtr (eval S c (.atomic body r f g) ρ s) = rtr (eval S c body ρ { s with comp := [] }) ++ X := by
  cases hb : eval S c body ρ { s with comp := [] } with
  | ok o => rw [eval_atomic_ok hb]; exact ⟨[], (List.append_nil _).symm⟩
  | error e =>
    cases e with
    | timeout q s₁ tr =>
      by_cases hy : (q.isAbort && ((compensate S s₁.comp s₁.w).ok &&
          S.probe r (compensate S s₁.comp s₁.w).w)) = true
      · rw [eval_atomic_reraise hb hy]
        exact ex_append tr _ _ _
      · rw [eval_atomic_fail hb (by simpa using hy)] at hs ⊢
        rw [rtr_pre_bnd hs (fun _ => rfl)]
        exact ex_append tr _ _ _
    | _ =>
      have := eval_atomic_settled hs
      rw [hb] at this
      exact (this : False).elim

/-- **Teorema T3 com sagas.** Para todo programa bem tipado, com ou sem sagas, duas
execuções a partir de ambientes baixo-equivalentes e do mesmo mundo, com oráculos e tetos
arbitrários, terminam com o mesmo traço, o mesmo mundo e o mesmo resultado `⊒ Endorsed`; ou
param na mesma pós-condição com o mesmo traço e o mesmo mundo; ou divergem numa decisão de
endosso. -/
theorem noninterference_saga {c₁ c₂ : Cfg} {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty}
    {ι : Lab} {β : Budget} (h : HasType S Γ pc e τ ι β) :
    ∀ {ρ₁ ρ₂ : List Val} {s₁ s₂ : St}, LowEq Γ ρ₁ ρ₂ → SameW s₁ s₂ →
      Settled (eval S c₁ e ρ₁ s₁) → Settled (eval S c₂ e ρ₂ s₂) →
      NIR (OutEq ι) (eval S c₁ e ρ₁ s₁) (eval S c₂ e ρ₂ s₂) := by
  induction h with
  | val _ =>
    intro ρ₁ ρ₂ s₁ s₂ _ hw _ _
    exact .inl ⟨_, _, rfl, rfl, rfl, hw, fun _ => rfl⟩
  | @var Γ pc k τ ι hk =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    cases h₁ : ρ₁[k]? with
    | none => simp [eval, h₁, Settled] at hs₁
    | some v₁ =>
      cases h₂ : ρ₂[k]? with
      | none => simp [eval, h₂, Settled] at hs₂
      | some v₂ =>
        simp only [eval, h₁, h₂]
        refine .inl ⟨_, _, rfl, rfl, rfl, hw, fun hι => ?_⟩
        have := hl.data_eq hk hι
        rw [h₁, h₂] at this
        exact Option.some.inj this
  | @let_ Γ pc e₁ e₂ τ₁ τ₂ ι₁ ι₂ β₁ β₂ _ _ ih₁ ih₂ =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    simp only [eval] at hs₁ hs₂ ⊢
    exact NIR.seq (ih₁ hl hw hs₁.of_bnd hs₂.of_bnd)
      (I₁ := fun a => eval S c₁ e₂ (a.v :: ρ₁) a.st) (g₁ := fun a b => ⟨b.v, b.st, a.tr ++ b.tr⟩)
      (I₂ := fun a => eval S c₂ e₂ (a.v :: ρ₂) a.st) (g₂ := fun a b => ⟨b.v, b.st, a.tr ++ b.tr⟩)
      (fun _ => ext_pre_bnd _ fun _ => rfl) (fun _ => ext_pre_bnd _ fun _ => rfl)
      (fun _ _ _ hp h₁ h₂ => ih₂ (.data hp.2 hl) hp.1 h₁ h₂)
      (fun _ _ _ _ _ _ _ _ hpi => hpi) hs₁ hs₂
  | @ite Γ pc cnd t f τ ιc ιt ιf βc βt βf _ ht hf ihc iht ihf =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    simp only [eval] at hs₁ hs₂ ⊢
    refine NIR.seq (ihc hl hw hs₁.of_bnd hs₂.of_bnd)
      (I₁ := fun a => match a.v with
        | .bool b => cond b (eval S c₁ t ρ₁ a.st) (eval S c₁ f ρ₁ a.st)
        | _ => .error .stuck)
      (g₁ := fun a b => ⟨b.v, b.st, a.tr ++ b.tr⟩)
      (I₂ := fun a => match a.v with
        | .bool b => cond b (eval S c₂ t ρ₂ a.st) (eval S c₂ f ρ₂ a.st)
        | _ => .error .stuck)
      (g₂ := fun a b => ⟨b.v, b.st, a.tr ++ b.tr⟩) (Pi := OutEq (ιc ⊓ (ιt ⊓ ιf))) ?_ ?_ ?_
      (fun _ _ _ _ _ _ _ _ hpi => hpi) hs₁ hs₂
    · intro a; rcases a with ⟨v, st, tr⟩
      cases v <;> first | exact ext_pre_bnd _ fun _ => rfl | exact ext_stuck
    · intro a; rcases a with ⟨v, st, tr⟩
      cases v <;> first | exact ext_pre_bnd _ fun _ => rfl | exact ext_stuck
    · intro a₁ a₂ htr hp h₁ h₂
      obtain ⟨hw', hv⟩ := hp
      rcases a₁ with ⟨v₁, st₁, tr₁⟩; rcases a₂ with ⟨v₂, st₂, tr₂⟩
      cases v₁ <;> try exact (h₁ : False).elim
      cases v₂ <;> try exact (h₂ : False).elim
      rename_i b₁ b₂
      by_cases hlc : low ιc
      · have hb : Val.bool b₁ = Val.bool b₂ := hv hlc
        cases hb
        cases b₁
        · exact (ihf hl hw' h₁ h₂).mono fun _ _ hp =>
            ⟨hp.1, fun hι => hp.2 (low_meet.mp (low_meet.mp hι).2).2⟩
        · exact (iht hl hw' h₁ h₂).mono fun _ _ hp =>
            ⟨hp.1, fun hι => hp.2 (low_meet.mp (low_meet.mp hι).2).1⟩
      · have hpc : ¬ low (pc ⊓ ιc) := fun h => hlc (low_meet.mp h).2
        have key : ∀ (b : Bool) (c' : Cfg) (ρ : List Val) (st : St),
            Settled (cond b (eval S c' t ρ st) (eval S c' f ρ st)) →
            ∃ o, cond b (eval S c' t ρ st) (eval S c' f ρ st) = .ok o ∧ o.tr = [] ∧
              o.st.w = st.w ∧ o.st.comp = st.comp ∧ o.st.next = st.next := by
          intro b c' ρ st hs
          cases b
          · exact confine_settled hf hpc hs
          · exact confine_settled ht hpc hs
        obtain ⟨o₁, e₁, t₁, w₁, k₁, n₁⟩ := key b₁ c₁ ρ₁ st₁ h₁
        obtain ⟨o₂, e₂, t₂, w₂, k₂, n₂⟩ := key b₂ c₂ ρ₂ st₂ h₂
        exact .inl ⟨o₁, o₂, e₁, e₂, by show o₁.tr = o₂.tr; rw [t₁, t₂],
          ⟨by rw [w₁, w₂]; exact hw'.1, by rw [k₁, k₂]; exact hw'.2.1, by rw [n₁, n₂]; exact hw'.2.2⟩,
          fun hι => absurd (low_meet.mp hι).1 hlc⟩
  | @prim Γ pc op a b ιa ιb βa βb _ _ iha ihb =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    simp only [eval] at hs₁ hs₂ ⊢
    have hK : ∀ (o o₂ : Out), (match o.v, o₂.v with
        | .int x, .int y => (.ok ⟨op.apply x y, o₂.st, o.tr ++ o₂.tr⟩ : Except Err Out)
        | _, _ => .error .stuck) = .ok ⟨primv op o.v o₂.v, o₂.st, o.tr ++ o₂.tr⟩ ∨
        (match o.v, o₂.v with
        | .int x, .int y => (.ok ⟨op.apply x y, o₂.st, o.tr ++ o₂.tr⟩ : Except Err Out)
        | _, _ => .error .stuck) = .error .stuck := by
      intro o o₂
      rcases o with ⟨v, _, _⟩; rcases o₂ with ⟨v₂, _, _⟩
      cases v <;> cases v₂ <;> first | exact .inl rfl | exact .inr rfl
    exact NIR.seq (iha hl hw hs₁.of_bnd hs₂.of_bnd)
      (I₁ := fun o => eval S c₁ b ρ₁ o.st)
      (g₁ := fun o o₂ => ⟨primv op o.v o₂.v, o₂.st, o.tr ++ o₂.tr⟩)
      (I₂ := fun o => eval S c₂ b ρ₂ o.st)
      (g₂ := fun o o₂ => ⟨primv op o.v o₂.v, o₂.st, o.tr ++ o₂.tr⟩)
      (Pi := OutEq ιb)
      (fun o => ext_pre_bnd' _ (hK o) fun _ => rfl) (fun o => ext_pre_bnd' _ (hK o) fun _ => rfl)
      (fun _ _ _ hp h₁ h₂ => ihb hl hp.1 h₁ h₂)
      (fun a₁ a₂ b₁ b₂ hp _ _ _ hpi => ⟨hpi.1, fun hι => by
        show primv op a₁.v b₁.v = primv op a₂.v b₂.v
        rw [hp.2 (low_meet.mp hι).1, hpi.2 (low_meet.mp hι).2]⟩) hs₁ hs₂
  | @endorse Γ pc p e d τ ιe ιd βe βd _ _ _ ihe ihd =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    simp only [eval] at hs₁ hs₂ ⊢
    have hE : ∀ (c' : Cfg) (ρ : List Val) (o : Out),
        Ext o.tr (fun b : Out => (⟨b.v, b.st, o.tr ++ b.tr⟩ : Out))
          (if S.pol p o.v then .ok ⟨o.v, o.st, [.endorse p (some o.v)]⟩
            else pre [.endorse p none]
              (bnd (eval S c' d ρ o.st) fun b => .ok ⟨b.v, b.st, [.endorse p none] ++ b.tr⟩))
          (pre o.tr (if S.pol p o.v then .ok ⟨o.v, o.st, o.tr ++ [.endorse p (some o.v)]⟩
            else pre [.endorse p none] (bnd (eval S c' d ρ o.st) fun o' =>
              .ok ⟨o'.v, o'.st, o.tr ++ .endorse p none :: o'.tr⟩))) := by
      intro c' ρ o
      by_cases hp : S.pol p o.v = true
      · rw [if_pos hp, if_pos hp]
        exact ⟨fun b hb _ => (by cases hb; exact ⟨rfl, rfl⟩), fun _ _ _ hb => (by cases hb),
          fun _ => trivial⟩
      · rw [if_neg hp, if_neg hp]
        exact ext_pre2 o.tr [.endorse p none] _
    refine NIR.seq (ihe hl hw hs₁.of_bnd hs₂.of_bnd)
      (I₁ := fun o => if S.pol p o.v then .ok ⟨o.v, o.st, [.endorse p (some o.v)]⟩
        else pre [.endorse p none]
          (bnd (eval S c₁ d ρ₁ o.st) fun b => .ok ⟨b.v, b.st, [.endorse p none] ++ b.tr⟩))
      (g₁ := fun o b => ⟨b.v, b.st, o.tr ++ b.tr⟩)
      (I₂ := fun o => if S.pol p o.v then .ok ⟨o.v, o.st, [.endorse p (some o.v)]⟩
        else pre [.endorse p none]
          (bnd (eval S c₂ d ρ₂ o.st) fun b => .ok ⟨b.v, b.st, [.endorse p none] ++ b.tr⟩))
      (g₂ := fun o b => ⟨b.v, b.st, o.tr ++ b.tr⟩)
      (Pi := OutEq (.E ⊓ ιd)) (hE c₁ ρ₁) (hE c₂ ρ₂) ?_ (fun _ _ _ _ _ _ _ _ hpi => hpi) hs₁ hs₂
    intro a₁ a₂ _ hp h₁ h₂
    obtain ⟨hw', _⟩ := hp
    dsimp only at h₁ h₂ ⊢
    have hD : ∀ (c' : Cfg) (ρ : List Val) (st : St),
        Ext [.endorse p none] (fun b : Out => (⟨b.v, b.st, [.endorse p none] ++ b.tr⟩ : Out))
          (eval S c' d ρ st)
          (pre [.endorse p none] (bnd (eval S c' d ρ st) fun b =>
            .ok ⟨b.v, b.st, [.endorse p none] ++ b.tr⟩)) :=
      fun c' ρ st => ext_pre_bnd _ fun _ => rfl
    by_cases hp₁ : S.pol p a₁.v = true <;> by_cases hp₂ : S.pol p a₂.v = true
    · rw [if_pos hp₁] at h₁ ⊢; rw [if_pos hp₂] at h₂ ⊢
      by_cases hv : a₁.v = a₂.v
      · exact .inl ⟨_, _, rfl, rfl, by show [Ev.endorse p (some a₁.v)] = [Ev.endorse p (some a₂.v)]; rw [hv],
          hw', fun _ => hv⟩
      · exact .inr (.inr ⟨[], p, some a₁.v, some a₂.v, [], [], by simpa using hv, rfl, rfl⟩)
    · rw [if_pos hp₁] at h₁ ⊢; rw [if_neg hp₂] at h₂ ⊢
      refine .inr (.inr ⟨[], p, some a₁.v, none, [], rtr (eval S c₂ d ρ₂ a₂.st), by simp, rfl, ?_⟩)
      rw [(hD c₂ ρ₂ a₂.st).rtr h₂]; rfl
    · rw [if_neg hp₁] at h₁ ⊢; rw [if_pos hp₂] at h₂ ⊢
      refine .inr (.inr ⟨[], p, none, some a₂.v, rtr (eval S c₁ d ρ₁ a₁.st), [], by simp, ?_, rfl⟩)
      rw [(hD c₁ ρ₁ a₁.st).rtr h₁]; rfl
    · rw [if_neg hp₁] at h₁ ⊢; rw [if_neg hp₂] at h₂ ⊢
      exact NIR.ext (ihd hl hw' ((hD c₁ ρ₁ a₁.st).2.2 h₁) ((hD c₂ ρ₂ a₂.st).2.2 h₂))
        (hD c₁ ρ₁ a₁.st) (hD c₂ ρ₂ a₂.st) rfl
        (fun _ _ _ _ _ hpi => ⟨hpi.1, fun hι => hpi.2 (low_meet.mp hι).2⟩) h₁ h₂
  | @act Γ pc t k a ιa βa E _ hk _ _ hla iha =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    simp only [eval] at hs₁ hs₂ ⊢
    refine NIR.seq (iha hl hw hs₁.of_bnd hs₂.of_bnd)
      (I₁ := fun o => doAct S c₁ t ρ₁[k]? ⟨o.v, o.st, []⟩) (g₁ := fun o b => ⟨b.v, b.st, o.tr ++ b.tr⟩)
      (I₂ := fun o => doAct S c₂ t ρ₂[k]? ⟨o.v, o.st, []⟩) (g₂ := fun o b => ⟨b.v, b.st, o.tr ++ b.tr⟩)
      (Pi := OutEq .T) (fun o => ext_doAct S c₁ t _ o) (fun o => ext_doAct S c₂ t _ o) ?_
      (fun _ _ _ _ _ _ _ _ hpi => hpi) hs₁ hs₂
    intro o₁ o₂ _ hp h₁ h₂
    obtain ⟨hw', hv⟩ := hp
    obtain ⟨r₁, e₁⟩ := ok_of_settled h₁ fun _ _ _ => doAct_ne_timeout
    obtain ⟨r₂, e₂⟩ := ok_of_settled h₂ fun _ _ _ => doAct_ne_timeout
    rw [e₁, e₂]
    obtain ⟨id₁, hid₁, _, _, _, rfl⟩ := doAct_ok e₁
    obtain ⟨id₂, hid₂, _, _, _, rfl⟩ := doAct_ok e₂
    have hcap := hl.cap_eq hk
    rw [hid₁, hid₂] at hcap
    cases hcap
    have hv' : o₁.v = o₂.v := hv hla
    refine .inl ⟨_, _, rfl, rfl, by simp [HasTr.tr, hv'], ⟨?_, ?_, hw'.2.2⟩, fun _ => rfl⟩
    · show actW S t id₁ o₁.v o₁.st.w = actW S t id₁ o₂.v o₂.st.w
      rw [hv', hw'.1]
    · show actC S t id₁ o₁.v o₁.st.comp = actC S t id₁ o₂.v o₂.st.comp
      rw [hv', hw'.2.1]
  | @drop Γ pc k E hk =>
    intro ρ₁ ρ₂ s₁ s₂ _ hw hs₁ hs₂
    obtain ⟨r₁, e₁⟩ := ok_of_settled hs₁ fun _ _ _ => doDrop_ne_timeout
    obtain ⟨r₂, e₂⟩ := ok_of_settled hs₂ fun _ _ _ => doDrop_ne_timeout
    rw [e₁, e₂]
    obtain ⟨_, _, _, rfl⟩ := doDrop_ok e₁
    obtain ⟨_, _, _, rfl⟩ := doDrop_ok e₂
    exact .inl ⟨_, _, rfl, rfl, rfl, hw, fun _ => rfl⟩
  | @for_ Γ pc n xs body τ τb ιx ιb βx βb _ hb ihx ihb =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    simp only [eval] at hs₁ hs₂ ⊢
    refine NIR.seq (ihx hl hw hs₁.of_bnd hs₂.of_bnd)
      (Pi := fun r₁ r₂ : St × List Ev => SameW r₁.1 r₂.1)
      (I₁ := fun o => match o.v with
        | .list vs => loop (fun v s' => eval S c₁ body (v :: ρ₁) s') vs o.st
        | _ => .error .stuck)
      (g₁ := fun o r => ⟨.unit, r.1, o.tr ++ r.2⟩)
      (I₂ := fun o => match o.v with
        | .list vs => loop (fun v s' => eval S c₂ body (v :: ρ₂) s') vs o.st
        | _ => .error .stuck)
      (g₂ := fun o r => ⟨.unit, r.1, o.tr ++ r.2⟩) ?_ ?_ ?_
      (fun _ _ _ _ _ _ _ _ hpi => ⟨hpi, fun _ => rfl⟩) hs₁ hs₂
    · intro a; rcases a with ⟨v, st, tr⟩
      cases v <;> first | exact ext_pre_bnd _ fun _ => rfl | exact ext_stuck
    · intro a; rcases a with ⟨v, st, tr⟩
      cases v <;> first | exact ext_pre_bnd _ fun _ => rfl | exact ext_stuck
    · intro a₁ a₂ htr hp h₁ h₂
      obtain ⟨hw', hv⟩ := hp
      rcases a₁ with ⟨v₁, st₁, tr₁⟩; rcases a₂ with ⟨v₂, st₂, tr₂⟩
      cases v₁ <;> try exact (h₁ : False).elim
      cases v₂ <;> try exact (h₂ : False).elim
      rename_i vs₁ vs₂
      by_cases hlx : low ιx
      · have hvs : Val.list vs₁ = Val.list vs₂ := hv hlx
        cases hvs
        exact loop_ni (f₁ := fun v s' => eval S c₁ body (v :: ρ₁) s')
          (f₂ := fun v s' => eval S c₂ body (v :: ρ₂) s') (fun v s₁' s₂' hw'' h₁' h₂' =>
          (ihb (.data (fun _ => rfl) hl) hw'' h₁' h₂').mono fun _ _ hp => hp.1) vs₁ st₁ st₂ hw' h₁ h₂
      · have hpc : ¬ low (pc ⊓ ιx) := fun h => hlx (low_meet.mp h).2
        obtain ⟨r₁, e₁, t₁, w₁, k₁, n₁⟩ :=
          loop_confine (fun v s' hs => confine_settled hb hpc hs) vs₁ st₁ h₁
        obtain ⟨r₂, e₂, t₂, w₂, k₂, n₂⟩ :=
          loop_confine (fun v s' hs => confine_settled hb hpc hs) vs₂ st₂ h₂
        exact .inl ⟨r₁, r₂, e₁, e₂, by show r₁.2 = r₂.2; rw [t₁, t₂],
          ⟨by rw [w₁, w₂]; exact hw'.1, by rw [k₁, k₂]; exact hw'.2.1, by rw [n₁, n₂]; exact hw'.2.2⟩⟩
  | @infer Γ pc m t a τa ιa βa _ iha =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    simp only [eval] at hs₁ hs₂ ⊢
    refine NIR.seq (iha hl hw hs₁.of_bnd hs₂.of_bnd)
      (I₁ := fun o => doInfer S c₁ m t ⟨o.v, o.st, []⟩) (g₁ := fun o b => ⟨b.v, b.st, o.tr ++ b.tr⟩)
      (I₂ := fun o => doInfer S c₂ m t ⟨o.v, o.st, []⟩) (g₂ := fun o b => ⟨b.v, b.st, o.tr ++ b.tr⟩)
      (Pi := OutEq .U) (fun o => ext_doInfer S c₁ m t o) (fun o => ext_doInfer S c₂ m t o) ?_
      (fun _ _ _ _ _ _ _ _ hpi => hpi) hs₁ hs₂
    intro o₁ o₂ _ hp h₁ h₂
    obtain ⟨hw', _⟩ := hp
    obtain ⟨r₁, e₁⟩ := ok_of_settled h₁ fun _ _ _ => doInfer_ne_timeout
    obtain ⟨r₂, e₂⟩ := ok_of_settled h₂ fun _ _ _ => doInfer_ne_timeout
    rw [e₁, e₂]
    obtain ⟨_, rfl⟩ := doInfer_ok e₁
    obtain ⟨_, rfl⟩ := doInfer_ok e₂
    exact .inl ⟨_, _, rfl, rfl, rfl, hw', fun hι => absurd hι not_low_U⟩
  | @await Γ pc q _ =>
    intro ρ₁ ρ₂ s₁ s₂ _ hw _ _
    simp only [eval]
    rw [hw.1]
    cases S.probe q s₂.w
    · exact .inr (.inl ⟨.probe q, s₁, s₂, [], rfl, rfl, hw⟩)
    · exact .inl ⟨_, _, rfl, rfl, rfl, hw, fun _ => rfl⟩
  | @atomic Γ pc body r f g τ ιb ιf ιg βb βf βg _ _ _ _ ihb ihf ihg =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    have hb := ihb (s₁ := { s₁ with comp := [] }) (s₂ := { s₂ with comp := [] }) hl ⟨hw.1, rfl, hw.2.2⟩
      (eval_atomic_settled hs₁) (eval_atomic_settled hs₂)
    rcases hb with ⟨b₁, b₂, he₁, he₂, ht, hw', hv⟩ | ⟨q, u₁, u₂, tr, he₁, he₂, hw'⟩ | hd
    · rw [eval_atomic_ok he₁, eval_atomic_ok he₂]
      exact .inl ⟨_, _, rfl, rfl, ht, ⟨hw'.1, hw.2.1, hw'.2.2⟩, fun hι => hv (low_meet.mp hι).1⟩
    · by_cases hy : (q.isAbort && ((compensate S u₂.comp u₂.w).ok &&
          S.probe r (compensate S u₂.comp u₂.w).w)) = true
      · -- o encerramento pelo tratador atravessa as duas sagas igualmente
        have hy₁ : (q.isAbort && ((compensate S u₁.comp u₁.w).ok &&
            S.probe r (compensate S u₁.comp u₁.w).w)) = true := by rw [hw'.1, hw'.2.1]; exact hy
        rw [eval_atomic_reraise he₁ hy₁, eval_atomic_reraise he₂ hy]
        refine .inr (.inl ⟨q, { u₁ with w := (compensate S u₁.comp u₁.w).w, comp := s₁.comp },
          { u₂ with w := (compensate S u₂.comp u₂.w).w, comp := s₂.comp },
          (tr ++ .fail q :: (compensate S u₂.comp u₂.w).evs ++
            [.recover r (S.probe r (compensate S u₂.comp u₂.w).w)]) ++ [], ?_, rfl, ?_⟩)
        · rw [hw'.1, hw'.2.1]
        · exact ⟨by simp only [hw'.1, hw'.2.1], hw.2.1, hw'.2.2⟩
      have hn₂ : (q.isAbort && ((compensate S u₂.comp u₂.w).ok &&
          S.probe r (compensate S u₂.comp u₂.w).w)) = false := by simpa using hy
      have hn₁ : (q.isAbort && ((compensate S u₁.comp u₁.w).ok &&
          S.probe r (compensate S u₁.comp u₁.w).w)) = false := by rw [hw'.1, hw'.2.1]; exact hn₂
      rw [eval_atomic_fail he₁ hn₁] at hs₁ ⊢
      rw [eval_atomic_fail he₂ hn₂] at hs₂ ⊢
      rw [hw'.1, hw'.2.1] at hs₁ ⊢
      have hfin : ∀ (hd : Exp) (ιh : Lab), (low (ιb ⊓ (ιf ⊓ ιg)) → low ιh) →
          (∀ {s₁' s₂' : St}, SameW s₁' s₂' → Settled (eval S c₁ hd ρ₁ s₁') →
            Settled (eval S c₂ hd ρ₂ s₂') → NIR (OutEq ιh) (eval S c₁ hd ρ₁ s₁') (eval S c₂ hd ρ₂ s₂')) →
          ∀ (evs : List Ev) (w : World),
            Settled (pre evs (bnd (eval S c₁ hd ρ₁ { u₁ with w := w, comp := s₁.comp }) fun o' =>
              (.ok ⟨o'.v, exitSaga ρ₁ (.atomic body r f g) o'.st, evs ++ o'.tr⟩ : Except Err Out))) →
            Settled (pre evs (bnd (eval S c₂ hd ρ₂ { u₂ with w := w, comp := s₂.comp }) fun o' =>
              (.ok ⟨o'.v, exitSaga ρ₂ (.atomic body r f g) o'.st, evs ++ o'.tr⟩ : Except Err Out))) →
            NIR (OutEq (ιb ⊓ (ιf ⊓ ιg)))
              (pre evs (bnd (eval S c₁ hd ρ₁ { u₁ with w := w, comp := s₁.comp }) fun o' =>
                .ok ⟨o'.v, exitSaga ρ₁ (.atomic body r f g) o'.st, evs ++ o'.tr⟩))
              (pre evs (bnd (eval S c₂ hd ρ₂ { u₂ with w := w, comp := s₂.comp }) fun o' =>
                .ok ⟨o'.v, exitSaga ρ₂ (.atomic body r f g) o'.st, evs ++ o'.tr⟩)) := by
        intro hd ιh hι ih evs w h₁ h₂
        have hE₁ := ext_pre_bnd (t := evs)
          (g := fun o' : Out => (⟨o'.v, exitSaga ρ₁ (.atomic body r f g) o'.st, evs ++ o'.tr⟩ : Out))
          (eval S c₁ hd ρ₁ { u₁ with w := w, comp := s₁.comp }) fun _ => rfl
        have hE₂ := ext_pre_bnd (t := evs)
          (g := fun o' : Out => (⟨o'.v, exitSaga ρ₂ (.atomic body r f g) o'.st, evs ++ o'.tr⟩ : Out))
          (eval S c₂ hd ρ₂ { u₂ with w := w, comp := s₂.comp }) fun _ => rfl
        exact NIR.ext (ih (s₁' := { u₁ with w := w, comp := s₁.comp })
          (s₂' := { u₂ with w := w, comp := s₂.comp }) ⟨rfl, hw.2.1, hw'.2.2⟩ (hE₁.2.2 h₁) (hE₂.2.2 h₂)) hE₁ hE₂ rfl
          (fun _ _ _ _ _ hpi => ⟨hpi.1, fun h => hpi.2 (hι h)⟩) h₁ h₂
      revert hs₁ hs₂
      cases (compensate S u₂.comp u₂.w).ok && S.probe r (compensate S u₂.comp u₂.w).w
      · intro hs₂ hs₁
        exact hfin g ιg (fun h => (low_meet.mp (low_meet.mp h).2).2) (fun hw h₁ h₂ => ihg hl hw h₁ h₂)
          _ _ hs₁ hs₂
      · intro hs₂ hs₁
        exact hfin f ιf (fun h => (low_meet.mp (low_meet.mp h).2).1) (fun hw h₁ h₂ => ihf hl hw h₁ h₂)
          _ _ hs₁ hs₂
    · refine .inr (.inr ?_)
      obtain ⟨X₁, hX₁⟩ := atomic_extends hs₁
      obtain ⟨X₂, hX₂⟩ := atomic_extends hs₂
      rw [hX₁, hX₂]
      exact hd.append _ _
  | @call Γ pc t k κ a n e ιa βa E ιe βe _ hk _ hκ _ hla _ hpure _ iha _ =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    simp only [eval] at hs₁ hs₂ ⊢
    have hcap := hl.cap_eq hk
    have hkey := hl.key_eq hκ
    rw [← hcap, ← hkey] at hs₂ ⊢
    refine NIR.seq (iha hl hw hs₁.of_bnd hs₂.of_bnd)
      (I₁ := fun o => doCall S c₁ t ρ₁[k]? ρ₁[κ]? n (fun id s' => eval S c₁ e [.pend t id] s')
        ⟨o.v, o.st, []⟩)
      (g₁ := fun o b => ⟨b.v, b.st, o.tr ++ b.tr⟩)
      (I₂ := fun o => doCall S c₂ t ρ₁[k]? ρ₁[κ]? n (fun id s' => eval S c₂ e [.pend t id] s')
        ⟨o.v, o.st, []⟩)
      (g₂ := fun o b => ⟨b.v, b.st, o.tr ++ b.tr⟩)
      (Pi := OutEq .T) (fun o => ext_doCall S c₁ t _ _ n _ o) (fun o => ext_doCall S c₂ t _ _ n _ o) ?_
      (fun _ _ _ _ _ _ _ _ hpi => hpi) hs₁ hs₂
    intro o₁ o₂ _ hp h₁ h₂
    exact doCall_ni hpure hp.1 (hp.2 hla) h₁ h₂
  | @mint Γ pc e τ ι β _ _ _ ih =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    simp only [eval] at hs₁ hs₂ ⊢
    rw [← hw.2.2] at hs₂ ⊢
    by_cases hg₁ : s₁.next ∈ s₁.used ∨ s₁.next ∈ s₁.keys
    · rw [if_pos hg₁] at hs₁; exact (hs₁ : False).elim
    by_cases hg₂ : s₁.next ∈ s₂.used ∨ s₁.next ∈ s₂.keys
    · rw [if_pos hg₂] at hs₂; exact (hs₂ : False).elim
    rw [if_neg hg₁] at hs₁ ⊢; rw [if_neg hg₂] at hs₂ ⊢
    -- a mesma chave fresca nas duas execuções: o contador é do host
    exact (ih (ρ₁ := .cap s₁.next :: ρ₁) (ρ₂ := .cap s₁.next :: ρ₂)
      (s₁ := { s₁ with next := s₁.next + 1 }) (s₂ := { s₂ with next := s₁.next + 1 })
      (.key hl) ⟨hw.1, hw.2.1, rfl⟩ (Settled.of_mintOut hs₁) (Settled.of_mintOut hs₂)).mintOut
  | @transfer Γ pc p _ hp =>
    intro ρ₁ ρ₂ s₁ s₂ hl hw hs₁ hs₂
    have hpe := hl.pend_eq hp
    simp only [eval] at hs₁ hs₂ ⊢
    rw [← hpe] at hs₂ ⊢
    cases hρ : ρ₁[p]? with
    | none => simp [hρ, Settled] at hs₁
    | some v =>
      cases v with
      | pend t' k => exact .inl ⟨_, _, rfl, rfl, rfl, hw, fun _ => rfl⟩
      | _ => simp [hρ, Settled] at hs₁

/-! ## Consequências -/

/-- **T3 com sagas, para execuções que terminam.** Duas execuções bem-sucedidas do mesmo
programa bem tipado, a partir de ambientes baixo-equivalentes e do mesmo mundo, produzem o
mesmo traço, o mesmo mundo final e o mesmo resultado `⊒ Endorsed`, ou divergem numa decisão
de endosso. Os traços incluem as falhas de pós-condição, as compensações e as sondas: duas
execuções só tomam caminhos diferentes numa saga depois de divergir num endosso. -/
theorem noninterference_saga_ok {c₁ c₂ : Cfg} {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty}
    {ι : Lab} {β : Budget} {ρ₁ ρ₂ : List Val} {s₁ s₂ : St} {o₁ o₂ : Out}
    (h : HasType S Γ pc e τ ι β) (hl : LowEq Γ ρ₁ ρ₂) (hw : SameW s₁ s₂)
    (h₁ : eval S c₁ e ρ₁ s₁ = .ok o₁) (h₂ : eval S c₂ e ρ₂ s₂ = .ok o₂) :
    (o₁.tr = o₂.tr ∧ SameW o₁.st o₂.st ∧ (low ι → o₁.v = o₂.v)) ∨ Diverge o₁.tr o₂.tr := by
  have := noninterference_saga (c₁ := c₁) (c₂ := c₂) h hl hw (by rw [h₁]; trivial) (by rw [h₂]; trivial)
  rw [h₁, h₂] at this
  rcases this with ⟨a₁, a₂, e₁, e₂, ht, hw', hv⟩ | ⟨_, _, _, _, e₁, _⟩ | hd
  · cases e₁; cases e₂; exact .inl ⟨ht, hw', hv⟩
  · cases e₁
  · exact .inr hd

/-- **T1 + T3 com sagas.** Para um programa bem tipado e linear, sem chamadas com chave, com
todo `await` dentro de uma saga e o orçamento dentro dos dois tetos, as duas execuções
terminam com valor, e a conclusão do T3 vale sem hipótese sobre o resultado. -/
theorem noninterference_saga_total {c₁ c₂ : Cfg} (hO₁ : c₁.oracle.Ok) (hO₂ : c₂.oracle.Ok)
    {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab} {β : Budget}
    {ρ₁ ρ₂ : List Val} {s₁ s₂ : St} (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (hg : Guarded e) (hk : keyless e = true) (h0₁ : Start c₁.grants Γ ρ₁ s₁)
    (h0₂ : Start c₂.grants Γ ρ₂ s₂)
    (hB₁ : s₁.spent + β ≤ c₁.limit) (hB₂ : s₂.spent + β ≤ c₂.limit) (hl : LowEq Γ ρ₁ ρ₂)
    (hw : SameW s₁ s₂) :
    ∃ o₁ o₂, eval S c₁ e ρ₁ s₁ = .ok o₁ ∧ eval S c₂ e ρ₂ s₂ = .ok o₂ ∧
      ((o₁.tr = o₂.tr ∧ SameW o₁.st o₂.st ∧ (low ι → o₁.v = o₂.v)) ∨ Diverge o₁.tr o₂.tr) := by
  obtain ⟨o₁, h₁, _⟩ := soundness_keyless hO₁ h hlin hg hk h0₁ hB₁
  obtain ⟨o₂, h₂, _⟩ := soundness_keyless hO₂ h hlin hg hk h0₂ hB₂
  exact ⟨o₁, o₂, h₁, h₂, noninterference_saga_ok h hl hw h₁ h₂⟩

/-- **T1 + T3 com sagas e chamadas com chave.** Para um programa bem tipado e linear, com todo
`await` dentro de uma saga e o orçamento dentro dos dois tetos, as duas execuções chegam a um
desfecho (valor, recusa do provedor ou encerramento pelo tratador de `TimeoutUnknown`), e os
desfechos não interferem: o mesmo traço, o mesmo mundo e o mesmo resultado `⊒ Endorsed`; ou a
mesma falha (a mesma causa, inclusive o valor do tratador, o mesmo traço, o mesmo mundo); ou
uma divergência numa decisão de endosso. Dados `Untrusted` não decidem se uma operação com
chave foi confirmada, recusada ou transferida. -/
theorem noninterference_saga_keys {c₁ c₂ : Cfg} (hO₁ : c₁.oracle.Ok) (hO₂ : c₂.oracle.Ok)
    {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab} {β : Budget}
    {ρ₁ ρ₂ : List Val} {s₁ s₂ : St} (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (hg : Guarded e) (h0₁ : Start c₁.grants Γ ρ₁ s₁) (h0₂ : Start c₂.grants Γ ρ₂ s₂)
    (hB₁ : s₁.spent + β ≤ c₁.limit) (hB₂ : s₂.spent + β ≤ c₂.limit) (hl : LowEq Γ ρ₁ ρ₂)
    (hw : SameW s₁ s₂) :
    NIR (OutEq ι) (eval S c₁ e ρ₁ s₁) (eval S c₂ e ρ₂ s₂) := by
  have hs : ∀ {c : Cfg}, c.oracle.Ok → ∀ {ρ : List Val} {s : St}, Start c.grants Γ ρ s →
      s.spent + β ≤ c.limit → Settled (eval S c e ρ s) := by
    intro c hO ρ s h0 hB
    rcases soundness hO h hlin hg h0 hB with ⟨o, he, _⟩ | ⟨_, _, _, he⟩ | ⟨_, _, _, he, _⟩ <;>
      rw [he] <;> trivial
  exact noninterference_saga h hl hw (hs hO₁ h0₁ hB₁) (hs hO₂ h0₂ hB₂)

end

end Aletheia
