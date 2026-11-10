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

O estado carrega as capacidades consumidas, o gasto acumulado e o número de chamadas ao
oráculo. A saída de cada expressão carrega o valor, o estado final e os eventos que **ela**
produziu. O Teorema T1 (`Soundness.lean`) prova que um programa bem tipado nunca chega a
nenhum desses erros.
-/

namespace Aletheia

open Budget

structure St where
  used  : List Nat
  spent : Budget
  calls : Nat

structure Out where
  v  : Val
  st : St
  tr : List Ev

inductive Err where
  | stuck | noCap | reuse | budget
  deriving DecidableEq, Repr

/-- O `bind` de `Except`, com equações de simplificação próprias. -/
def bnd {α β : Type} (r : Except Err α) (f : α → Except Err β) : Except Err β :=
  match r with
  | .ok a => f a
  | .error e => .error e

@[simp] theorem bnd_ok {α β : Type} (a : α) (f : α → Except Err β) : bnd (.ok a) f = f a := rfl
@[simp] theorem bnd_error {α β : Type} (e : Err) (f : α → Except Err β) :
    bnd (.error e) f = .error e := rfl

theorem bnd_eq_ok {α β : Type} {r : Except Err α} {f : α → Except Err β} {b : β} :
    bnd r f = .ok b ↔ ∃ a, r = .ok a ∧ f a = .ok b := by
  cases r <;> simp [bnd]

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
        .ok ⟨.unit, ⟨id :: o.st.used, o.st.spent + S.cost t, o.st.calls⟩, o.tr ++ [.act t id o.v]⟩
      else .error .budget
    else .error .noCap
  | _ => .error .noCap

/-- `drop k`: consome a capacidade sem efeito e sem evento. -/
def doDrop (cv : Option Val) (s : St) : Except Err Out :=
  match cv with
  | some (.cap id) =>
    if id ∈ s.used then .error .reuse
    else .ok ⟨.unit, ⟨id :: s.used, s.spent, s.calls⟩, []⟩
  | _ => .error .noCap

/-- O efeito de `infer m t`, depois de avaliado o prompt (saída `o`). -/
def doInfer (S : Sig) (c : Cfg) (m : Nat) (t : Ty) (o : Out) : Except Err Out :=
  if ble (o.st.spent + S.mcost m) c.limit then
    .ok ⟨c.oracle o.st.calls m t o.v, ⟨o.st.used, o.st.spent + S.mcost m, o.st.calls + 1⟩, o.tr⟩
  else .error .budget

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
    else bnd (eval S c d ρ o.st) fun o' => .ok ⟨o'.v, o'.st, o.tr ++ .endorse p none :: o'.tr⟩
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

theorem cond_ok {α : Type} {b : Bool} {x y : Except Err α} {r : α} (h : cond b x y = .ok r) :
    (b = true ∧ x = .ok r) ∨ (b = false ∧ y = .ok r) := by
  cases b
  · exact .inr ⟨rfl, h⟩
  · exact .inl ⟨rfl, h⟩

theorem doAct_ok {S : Sig} {c : Cfg} {t : Nat} {cv : Option Val} {o r : Out}
    (h : doAct S c t cv o = .ok r) :
    ∃ id, cv = some (.cap id) ∧ S.effect t ∈ c.grants id ∧ id ∉ o.st.used ∧
      ble (o.st.spent + S.cost t) c.limit = true ∧
      r = ⟨.unit, ⟨id :: o.st.used, o.st.spent + S.cost t, o.st.calls⟩, o.tr ++ [.act t id o.v]⟩ := by
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
    ∃ id, cv = some (.cap id) ∧ id ∉ s.used ∧ r = ⟨.unit, ⟨id :: s.used, s.spent, s.calls⟩, []⟩ := by
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
      r = ⟨c.oracle o.st.calls m t o.v, ⟨o.st.used, o.st.spent + S.mcost m, o.st.calls + 1⟩, o.tr⟩ := by
  unfold doInfer at h
  by_cases hb : ble (o.st.spent + S.mcost m) c.limit = true
  · simp [hb] at h; exact ⟨hb, h.symm⟩
  · simp [hb] at h

end Aletheia
