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

/-! ## Ciclo 1: avaliação ordenada e autorização integral

`keeping campo by Q` em que `Q` recebe também a lista dos elementos que já aceitou neste plano
(`pruneOrd`), e `keeping all campo by Q` (`pruneAll`). O modelo é o do `prune` do `arx` e do
`_prune` do runtime Python: uma passada, na ordem proposta; aceitar acrescenta o elemento ao fim
da lista de aceitos; recusar não muda a lista e não interrompe a passada. -/

/-- A passada. `accR` são os aceitos até aqui, do mais recente para o mais antigo (a política
os vê na ordem de aceitação, `accR.reverse`). -/
def pruneOrdAux (Q : α → List α → Bool) : List α → List α → List α
  | accR, [] => accR.reverse
  | accR, x :: xs => if Q x accR.reverse then pruneOrdAux Q (x :: accR) xs else pruneOrdAux Q accR xs

/-- A poda ordenada: começa sem nenhum aceito. -/
def pruneOrd (Q : α → List α → Bool) (xs : List α) : List α := pruneOrdAux Q [] xs

/-- Sobre a lista invertida: cada elemento foi aceito por `Q` diante dos que vieram antes dele. -/
def JustifiedR (Q : α → List α → Bool) : List α → Prop
  | [] => True
  | x :: rest => Q x rest.reverse = true ∧ JustifiedR Q rest

/-- Cada elemento de `l` foi aceito por `Q` diante exatamente dos que o precedem em `l`. -/
def Justified (Q : α → List α → Bool) (l : List α) : Prop := JustifiedR Q l.reverse

theorem pruneOrdAux_spec (Q : α → List α → Bool) (xs accR : List α) (h : JustifiedR Q accR) :
    Justified Q (pruneOrdAux Q accR xs) ∧ ∃ r, pruneOrdAux Q accR xs = accR.reverse ++ r ∧ r.Sublist xs := by
  induction xs generalizing accR with
  | nil =>
    refine ⟨?_, [], ?_, List.Sublist.refl _⟩
    · simpa [Justified, pruneOrdAux] using h
    · simp [pruneOrdAux]
  | cons x xs ih =>
    unfold pruneOrdAux
    by_cases q : Q x accR.reverse = true
    · simp only [q, if_true]
      obtain ⟨j, r, e, s⟩ := ih (x :: accR) ⟨q, h⟩
      refine ⟨j, x :: r, ?_, s.cons₂ x⟩
      rw [e]
      simp [List.reverse_cons, List.append_assoc]
    · simp only [q]
      obtain ⟨j, r, e, s⟩ := ih accR h
      exact ⟨j, r, e, s.cons x⟩

/-- **A poda ordenada é sólida, sobre a lista que ela devolve.** A lista resultante é uma
subsequência da proposta (nada inventado, a ordem é a mesma), e cada elemento dela foi aceito
pela política diante exatamente dos elementos que o precedem **nessa lista**. O teorema não fala
da execução: que o handler execute essa lista, uma vez e inteira, não é provado aqui (o programa
pode percorrê-la de novo ou só em parte); o teto que a política impõe vale para o plano
autorizado, não para o que um handler escolha fazer com ele. -/
theorem pruneOrd_sound (Q : α → List α → Bool) (xs : List α) :
    (pruneOrd Q xs).Sublist xs ∧ Justified Q (pruneOrd Q xs) := by
  obtain ⟨j, r, e, s⟩ := pruneOrdAux_spec Q xs [] trivial
  refine ⟨?_, j⟩
  unfold pruneOrd
  rw [e]
  simpa using s

/-- **Sem estado, é a poda de antes.** Uma política que ignora os aceitos dá o filtro da RC7:
os programas anteriores não mudam de significado. -/
theorem pruneOrdAux_stateless (Q : α → Bool) (xs accR : List α) :
    pruneOrdAux (fun x _ => Q x) accR xs = accR.reverse ++ xs.filter Q := by
  induction xs generalizing accR with
  | nil => simp [pruneOrdAux]
  | cons x xs ih =>
    unfold pruneOrdAux
    by_cases q : Q x = true
    · simp [q, ih, List.filter_cons, List.reverse_cons, List.append_assoc]
    · simp [q, ih, List.filter_cons]

theorem pruneOrd_stateless (Q : α → Bool) (xs : List α) : pruneOrd (fun x _ => Q x) xs = xs.filter Q := by
  simp [pruneOrd, pruneOrdAux_stateless]

/-- `keeping all`: uma recusa basta para nada ficar. -/
def pruneAll (Q : α → List α → Bool) (xs : List α) : List α :=
  if (pruneOrd Q xs).length = xs.length then xs else []

/-- **A autorização integral é tudo ou nada.** O resultado é a proposta inteira ou a lista vazia;
quando é a proposta inteira, nenhum elemento foi recusado: ela é o que a poda ordenada manteria,
e portanto cada elemento foi aceito diante dos anteriores. -/
theorem pruneAll_sound (Q : α → List α → Bool) (xs : List α) :
    (pruneAll Q xs = [] ∨ pruneAll Q xs = xs) ∧ (pruneAll Q xs ≠ [] → Justified Q (pruneAll Q xs)) := by
  unfold pruneAll
  by_cases h : (pruneOrd Q xs).length = xs.length
  · rw [if_pos h]
    refine ⟨Or.inr rfl, fun _ => ?_⟩
    obtain ⟨s, j⟩ := pruneOrd_sound Q xs
    have e : pruneOrd Q xs = xs := s.eq_of_length h
    rw [e] at j
    exact j
  · rw [if_neg h]
    exact ⟨Or.inl rfl, fun c => absurd rfl c⟩

/-- O exemplo do teto corrido: cada número é aceito se a soma dos aceitos com ele não passa de 500. -/
def cap500 (x : Nat) (acc : List Nat) : Bool := acc.sum + x ≤ 500

/-- A ordem decide, a recusada não entra no total, e a seguinte ainda é julgada. -/
theorem ordered_cap_examples :
    pruneOrd cap500 [300, 301, 200] = [300, 200] ∧ pruneOrd cap500 [301, 300, 200] = [301]
      ∧ pruneOrd cap500 [600, 100] = [100] ∧ pruneAll cap500 [300, 200, 1] = []
      ∧ pruneAll cap500 [300, 200] = [300, 200] := by
  decide

end Aletheia.Prune
