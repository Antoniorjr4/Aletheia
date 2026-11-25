/-!
# v1.0-RC1: autorização como capacidade linear (concessões)

A Rev13 autorizava uma operação quando a tupla (operação, destinatário, valor) era uma linha de
uma relação confiável **derivada do pedido**. O harness construía essa relação como o produto
cartesiano dos IBANs e dos valores do pedido, para as duas operações: ela continha tuplas que
ninguém pediu (`cartesian_overgrants`). E uma relação é reutilizável: a mesma linha autoriza
quantos efeitos se queira (`relation_is_reusable`).

Uma **concessão** é uma tupla inteira, ligada a uma execução, que o efeito consome. Este arquivo
modela o que `arx::grant` implementa — `find` (há uma concessão livre desta execução para a
tupla) e `consume` (ela deixa de estar livre) — e prova:

* `exec_sound`: todo efeito executado corresponde a uma concessão **desta execução** com a
  **mesma** operação e os **mesmos** argumentos. Não há combinação;
* `exec_linear`: o número de efeitos de uma tupla não passa do número de concessões dela;
* `other_run_refused`: uma concessão de outra execução não autoriza nada;
* `refused_leaves_store`: uma recusa não consome nada.

Fora do modelo (hipóteses do sistema, documentadas em `docs/RC1.md`): a origem da concessão é
uma confirmação do principal (o runtime recusa qualquer outra origem na carga, mas não pode
provar quem confirmou), e o registro de consumo em disco é íntegro.
-/

namespace Aletheia.Grant

/-- Uma concessão: a execução, a operação e os argumentos exatos. -/
structure G where
  run : Nat
  op : Nat
  args : List Nat
  deriving DecidableEq, Repr

/-- As concessões livres. O consumo remove **uma** ocorrência. -/
abbrev Store := List G

/-- Um efeito pedido pela execução `r`: há uma concessão livre para a tupla inteira? Se há, o
efeito a consome; se não, a recusa deixa as concessões como estavam. -/
def step (r : Nat) (s : Store) (op : Nat) (xs : List Nat) : Option Store :=
  if (⟨r, op, xs⟩ : G) ∈ s then some (s.erase ⟨r, op, xs⟩) else none

/-- A execução `r` pede os efeitos `qs`, em ordem; para no primeiro recusado. Devolve os
efeitos executados e as concessões que sobraram. -/
def exec (r : Nat) : Store → List (Nat × List Nat) → List (Nat × List Nat) × Store
  | s, [] => ([], s)
  | s, (op, xs) :: qs =>
    match step r s op xs with
    | some s' => let (done, s'') := exec r s' qs; ((op, xs) :: done, s'')
    | none => ([], s)

theorem step_some {r : Nat} {s s' : Store} {op : Nat} {xs : List Nat} (h : step r s op xs = some s') :
    (⟨r, op, xs⟩ : G) ∈ s ∧ s' = s.erase ⟨r, op, xs⟩ := by
  unfold step at h
  split at h
  · rename_i hm; exact ⟨hm, (Option.some.inj h).symm⟩
  · cases h

/-- **Sem combinação.** Todo efeito executado tem uma concessão desta execução com a mesma
operação e os mesmos argumentos, presente no início. -/
theorem exec_sound (r : Nat) : ∀ (qs : List (Nat × List Nat)) (s : Store) (op : Nat) (xs : List Nat),
    (op, xs) ∈ (exec r s qs).1 → (⟨r, op, xs⟩ : G) ∈ s := by
  intro qs
  induction qs with
  | nil => intro s op xs h; simp [exec] at h
  | cons q qs ih =>
    intro s op xs h
    obtain ⟨o, ys⟩ := q
    simp only [exec] at h
    cases hs : step r s o ys with
    | none => rw [hs] at h; simp at h
    | some s' =>
      rw [hs] at h
      obtain ⟨hm, rfl⟩ := step_some hs
      simp only [List.mem_cons, Prod.mk.injEq] at h
      rcases h with ⟨rfl, rfl⟩ | h
      · exact hm
      · exact List.mem_of_mem_erase (ih _ _ _ h)

/-- Quantas vezes a tupla aparece entre os efeitos executados. -/
def times (op : Nat) (xs : List Nat) (done : List (Nat × List Nat)) : Nat := done.count (op, xs)

/-- **Linearidade.** Os efeitos executados de uma tupla, mais as concessões dela que sobraram,
são exatamente as concessões que havia: uma concessão paga um efeito, e nenhuma some sem efeito. -/
theorem exec_conserves (r : Nat) : ∀ (qs : List (Nat × List Nat)) (s : Store) (op : Nat) (xs : List Nat),
    times op xs (exec r s qs).1 + (exec r s qs).2.count ⟨r, op, xs⟩ = s.count ⟨r, op, xs⟩ := by
  intro qs
  induction qs with
  | nil => intro s op xs; simp [exec, times]
  | cons q qs ih =>
    intro s op xs
    obtain ⟨o, ys⟩ := q
    simp only [exec]
    cases hs : step r s o ys with
    | none => simp [times]
    | some s' =>
      obtain ⟨hm, rfl⟩ := step_some hs
      have h := ih (s.erase ⟨r, o, ys⟩) op xs
      simp only [times] at h ⊢
      by_cases he : (o, ys) = (op, xs)
      · obtain ⟨rfl, rfl⟩ := Prod.mk.inj he
        have hpos : 0 < s.count (⟨r, o, ys⟩ : G) := List.count_pos_iff.mpr hm
        rw [List.count_erase_self] at h
        simp only [List.count_cons_self]
        omega
      · have hne : (⟨r, o, ys⟩ : G) ≠ ⟨r, op, xs⟩ := by
          intro hc; injection hc with _ h1 h2; exact he (by rw [h1, h2])
        rw [List.count_erase_of_ne (Ne.symm hne)] at h
        rw [List.count_cons_of_ne he]
        exact h

/-- O número de efeitos de uma tupla não passa do número de concessões dela. -/
theorem exec_linear (r : Nat) (qs : List (Nat × List Nat)) (s : Store) (op : Nat) (xs : List Nat) :
    times op xs (exec r s qs).1 ≤ s.count ⟨r, op, xs⟩ := by
  have := exec_conserves r qs s op xs
  omega

/-- Uma concessão de outra execução não autoriza nada. -/
theorem other_run_refused {r r' : Nat} (hr : r ≠ r') (s : Store) (hs : ∀ g ∈ s, g.run = r')
    (op : Nat) (xs : List Nat) : step r s op xs = none := by
  unfold step
  split
  · rename_i hm; exact absurd (hs _ hm) hr
  · rfl

/-- Uma recusa não consome nada: as concessões ficam como estavam. -/
theorem refused_leaves_store (r : Nat) (s : Store) (op : Nat) (xs : List Nat) (qs : List (Nat × List Nat))
    (h : step r s op xs = none) : exec r s ((op, xs) :: qs) = ([], s) := by
  simp [exec, h]

/-! ## Os contraexemplos: a relação da Rev13 -/

/-- A relação `intents` da Rev13: todo destinatário × todo valor, para as duas operações. -/
def cartesian (ibans amounts : List Nat) : List (Nat × List Nat) :=
  [0, 1].flatMap fun op => ibans.flatMap fun i => amounts.map fun a => (op, [i, a])

/-- O pedido "send 100 → 7 e schedule 250 → 9" (operação 0 = send, 1 = schedule). A relação
cartesiana contém `send 250 → 7`, que ninguém pediu; as duas concessões confirmadas recusam. -/
theorem cartesian_overgrants :
    (0, [7, 250]) ∈ cartesian [7, 9] [100, 250] ∧
    step 1 [⟨1, 0, [7, 100]⟩, ⟨1, 1, [9, 250]⟩] 0 [7, 250] = none := by
  decide

/-- As seis recombinações são recusadas e as duas confirmadas executam, uma vez cada. -/
theorem two_payees_two_amounts :
    let s : Store := [⟨1, 0, [7, 100]⟩, ⟨1, 1, [9, 250]⟩]
    (∀ q ∈ [(0, [7, 250]), (0, [9, 100]), (0, [9, 250]), (1, [7, 100]), (1, [7, 250]), (1, [9, 100])],
      step 1 s q.1 q.2 = none) ∧
    exec 1 s [(0, [7, 100]), (1, [9, 250])] = ([(0, [7, 100]), (1, [9, 250])], []) ∧
    (exec 1 s [(0, [7, 100]), (0, [7, 100])]).1 = [(0, [7, 100])] := by
  decide

/-- Uma relação (sem consumo) autoriza a mesma tupla quantas vezes se queira: é o que a
concessão linear impede (`exec_linear`). -/
theorem relation_is_reusable (rel : List (Nat × List Nat)) (q : Nat × List Nat) (h : q ∈ rel) (n : Nat) :
    ∀ x ∈ List.replicate n q, x ∈ rel := by
  intro x hx
  rw [(List.mem_replicate.mp hx).2]; exact h

end Aletheia.Grant
