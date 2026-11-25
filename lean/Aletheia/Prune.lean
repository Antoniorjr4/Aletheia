/-!
# v1.0-RC7: execução parcial sob política (`keeping`)

O modelo da decisão de `refine d by P keeping campo by Q`. Um plano é a lista do campo e o
resto; `Q` decide sobre cada elemento, `P` sobre o plano; `min` é o mínimo da lista no tipo.

| `arx` | aqui |
|---|---|
| `refine d by P` (até a RC6) | `strict` |
| `refine d by P keeping campo by Q` | `keeping`: deixa de fora o que `Q` recusa, e só então `P` decide |
| `prune_underflow` | a lista ficaria abaixo de `min` |

As políticas são funções puras do valor (e da fotografia confiável, que é a mesma durante a
execução): aqui, funções quaisquer.

`keeping_sound` e `keeping_keeps_every_accepted` são a garantia; `strict_denies` é o defeito
que a RC6 mostrou com um modelo real; `keeping_extends_strict` diz que nada que a RC6 aceitava
deixa de ser aceito; `checking_before_pruning_is_unsound` diz por que `P` decide **depois** da
poda, e não antes.
-/

namespace Aletheia.Prune

variable {α β : Type}

/-- Um plano: a lista do campo de `keeping` e o resto. -/
abbrev Plan (α β : Type) := List α × β

/-- O `refine` sem `keeping`: o plano inteiro, ou nada. -/
def strict (P : Plan α β → Bool) (pl : Plan α β) : Option (Plan α β) :=
  if P pl then some pl else none

/-- A poda: o que `Q` aceita, na ordem; o resto do plano não muda. -/
def prune (Q : α → Bool) (pl : Plan α β) : Plan α β := (pl.1.filter Q, pl.2)

/-- O `refine` com `keeping`. -/
def keeping (P : Plan α β → Bool) (Q : α → Bool) (min : Nat) (pl : Plan α β) : Option (Plan α β) :=
  let pl' := prune Q pl
  if pl'.1.length < min then none else if P pl' then some pl' else none

theorem keeping_eq {P : Plan α β → Bool} {Q : α → Bool} {min : Nat} {pl out : Plan α β}
    (h : keeping P Q min pl = some out) :
    out = prune Q pl ∧ min ≤ (prune Q pl).1.length ∧ P (prune Q pl) = true := by
  unfold keeping at h
  by_cases a : (prune Q pl).1.length < min
  · simp [a] at h
  · simp only [a, if_false] at h
    by_cases b : P (prune Q pl) = true
    · simp only [b, if_true, Option.some.injEq] at h
      exact ⟨h.symm, Nat.le_of_not_lt a, b⟩
    · simp [b] at h

/-- **O que é executado satisfaz tudo.** Se `keeping` devolve um plano: a política do plano o
aceita, cada elemento da lista é aceito pela política do elemento, a lista é uma subsequência
da proposta (nada é inventado, a ordem é a mesma), o resto do plano é o proposto, e o mínimo
do tipo é respeitado. -/
theorem keeping_sound {P : Plan α β → Bool} {Q : α → Bool} {min : Nat} {pl out : Plan α β}
    (h : keeping P Q min pl = some out) :
    P out = true ∧ (∀ x ∈ out.1, Q x = true) ∧ out.1.Sublist pl.1 ∧ out.2 = pl.2 ∧ min ≤ out.1.length := by
  obtain ⟨e, m, p⟩ := keeping_eq h
  subst e
  refine ⟨p, ?_, List.filter_sublist, rfl, m⟩
  intro x hx
  exact (List.mem_filter.1 hx).2

/-- **Nada seguro é perdido.** Se `keeping` devolve um plano, todo elemento proposto que a
política do elemento aceita está nele (com a mesma multiplicidade: é o filtro), e o que saiu
foi recusado por ela. -/
theorem keeping_keeps_every_accepted {P : Plan α β → Bool} {Q : α → Bool} {min : Nat} {pl out : Plan α β}
    (h : keeping P Q min pl = some out) :
    out.1 = pl.1.filter Q ∧ (∀ x ∈ pl.1, x ∉ out.1 → Q x = false) := by
  obtain ⟨e, _, _⟩ := keeping_eq h
  subst e
  refine ⟨rfl, ?_⟩
  intro x hx hn
  cases q : Q x
  · rfl
  · exact absurd (List.mem_filter.2 ⟨hx, q⟩) hn

/-- **Nada que a RC6 aceitava deixa de ser aceito.** Se o plano inteiro passa na política do
plano e todo elemento passa na do elemento (como numa política de plano que exige `all n in
notes => SafeNote(n)`), `keeping` devolve o mesmo plano. -/
theorem keeping_extends_strict {P : Plan α β → Bool} {Q : α → Bool} {min : Nat} {pl : Plan α β}
    (hq : ∀ x ∈ pl.1, Q x = true) (hm : min ≤ pl.1.length) (hs : strict P pl = some pl) :
    keeping P Q min pl = some pl := by
  have f : pl.1.filter Q = pl.1 := List.filter_eq_self.2 hq
  have pp : prune Q pl = pl := by simp [prune, f]
  have hp : P pl = true := by
    unfold strict at hs
    by_cases c : P pl = true
    · exact c
    · simp [c] at hs
  unfold keeping
  rw [pp]
  simp [Nat.not_lt.2 hm, hp]

/-- **A poda fecha a negação.** Se o que resta depois de deixar de fora o que `Q` recusa é um
plano que `P` aceita, `keeping` o devolve, qualquer que seja o resto da proposta. -/
theorem keeping_live {P : Plan α β → Bool} {Q : α → Bool} {min : Nat} {pl : Plan α β}
    (hm : min ≤ (prune Q pl).1.length) (hp : P (prune Q pl) = true) :
    keeping P Q min pl = some (prune Q pl) := by
  unfold keeping
  simp [Nat.not_lt.2 hm, hp]

/-! ## Os contraexemplos -/

/-- A política do elemento dos exemplos: só as issues 1 e 2 estão no cadastro. -/
def inCadastro (x : Nat) : Bool := x == 1 || x == 2

/-- A política do plano da triagem: há o que fazer, e toda nota é do cadastro. -/
def safePlan (pl : Plan Nat Unit) : Bool := pl.1.length ≥ 1 && pl.1.all inCadastro

/-- **O defeito da RC6.** Um elemento fora do cadastro faz o `refine` estrito recusar o plano
inteiro, embora o resto seja um plano que a mesma política aceita. -/
theorem strict_denies :
    strict safePlan ([1, 3, 2], ()) = none ∧ keeping safePlan inCadastro 0 ([1, 3, 2], ()) = some ([1, 2], ()) := by
  decide

/-- **Por que a política do plano decide depois da poda.** Decidir antes e podar depois
executaria um plano que a política do plano recusa: aqui ela exige as duas notas (uma por issue)
e a poda deixa uma. -/
def keepingBefore (P : Plan α β → Bool) (Q : α → Bool) (pl : Plan α β) : Option (Plan α β) :=
  if P pl then some (prune Q pl) else none

theorem checking_before_pruning_is_unsound :
    ∃ (P : Plan Nat Unit → Bool) (Q : Nat → Bool) (pl out : Plan Nat Unit),
      keepingBefore P Q pl = some out ∧ P out = false ∧ keeping P Q 0 pl = none :=
  ⟨fun pl => pl.1.length == 2, inCadastro, ([1, 3], ()), ([1], ()), by decide, by decide, by decide⟩

end Aletheia.Prune
