import Aletheia.Prune
import Aletheia.Lattice

/-!
# v1.0-RC8: o relatório é do que foi executado

Na RC7 o título e o resumo da issue de resumo eram parte do plano que o modelo propôs: o
"resto" `β` do plano, que `keeping` não toca e que a política do plano (que lê as notas) não
julga. Com um modelo real, o resumo citava a nota que `keeping` deixou de fora.

Na RC8 o programa compõe o relatório depois de executar, a partir das notas que restaram, e
o que saiu fica ligado a um nome (`keeping … as fora`) com rótulo `Untrusted`.

| `arx` | aqui |
|---|---|
| `refine … keeping campo by Q` | `Prune.keeping` |
| as notas enviadas, em ordem (`for n in notes { commit … }`) | `out.1` |
| `join(notes.issue, ", #")` | `compose f out`, com `f` a projeção |
| `keeping … as fora` | `dropped Q pl`, da amostra aceita |
| o resumo do modelo (RC7) | `out.2`, o resto do plano |
| `join`, `++` com refinamento de comprimento | `joinL`, `List.append` sobre `List Char` |

`report_is_exact` e `report_cites_only_what_ran` são a garantia; `rejected_content_is_invisible`
diz que o conteúdo do que foi recusado não alcança nada do que é executado ou relatado;
`model_summary_can_misreport` é o defeito da RC7; `dropped_exact` diz que a contagem relatada é
exata; `joinL_length_le` e `append_length_le` são os comprimentos que o verificador afirma; e
`composed_with_dropped_is_not_low` diz por que nada composto com `fora` chega a uma ação.
-/

namespace Aletheia.Report

open Aletheia.Prune

variable {α β γ : Type}

/-- O que `keeping … as fora` liga: o que `Q` recusou, na ordem da proposta. -/
def dropped (Q : α → Bool) (pl : Plan α β) : List α := pl.1.filter (fun x => !Q x)

/-- O relatório composto pelo programa: a projeção `f` de cada nota executada. -/
def compose (f : α → γ) (out : Plan α β) : List γ := out.1.map f

/-- **O relatório cita exatamente o que foi executado.** Se `keeping` devolve o plano `out`
(que é o que o laço executa, nota a nota), `y` está no relatório se e só se `y` é a projeção de
uma nota executada. -/
theorem report_is_exact {P : Plan α β → Bool} {Q : α → Bool} {min : Nat} {pl out : Plan α β}
    {f : α → γ} (_h : keeping P Q min pl = some out) (y : γ) :
    y ∈ compose f out ↔ ∃ x ∈ out.1, f x = y := by
  unfold compose
  exact List.mem_map

/-- **E só do que as políticas aceitaram, na proposta.** Cada item do relatório é a projeção de
uma nota que foi proposta, que a política do elemento aceitou e que foi executada; o relatório
é a projeção do filtro da proposta. -/
theorem report_cites_only_what_ran {P : Plan α β → Bool} {Q : α → Bool} {min : Nat} {pl out : Plan α β}
    {f : α → γ} (h : keeping P Q min pl = some out) :
    compose f out = (pl.1.filter Q).map f ∧
      ∀ y ∈ compose f out, ∃ x ∈ out.1, f x = y ∧ Q x = true ∧ x ∈ pl.1 := by
  obtain ⟨e, _, _⟩ := keeping_eq h
  subst e
  refine ⟨rfl, ?_⟩
  intro y hy
  obtain ⟨x, hx, rfl⟩ := List.mem_map.1 hy
  have m := List.mem_filter.1 hx
  exact ⟨x, hx, rfl, m.2, m.1⟩

/-- **`keeping` só vê o que aceita.** Duas propostas com o mesmo resto e as mesmas notas
aceitas, na mesma ordem, dão o mesmo resultado: o mesmo plano executado, o mesmo relatório. -/
theorem keeping_sees_only_the_accepted {P : Plan α β → Bool} {Q : α → Bool} {min : Nat}
    {pl pl' : Plan α β} (h₁ : pl.1.filter Q = pl'.1.filter Q) (h₂ : pl.2 = pl'.2) :
    keeping P Q min pl = keeping P Q min pl' := by
  unfold keeping prune
  rw [h₁, h₂]

/-- **O conteúdo do que foi recusado não alcança nada.** Inserir, em qualquer posição, uma nota
que a política do elemento recusa não muda o que `keeping` devolve: nem o que é executado, nem o
que o programa relata a partir disso. O texto de uma nota injetada não tem por onde influir. -/
theorem rejected_content_is_invisible {P : Plan α β → Bool} {Q : α → Bool} {min : Nat}
    {l₁ l₂ : List α} {r : β} {x : α} (hx : Q x = false) :
    keeping P Q min (l₁ ++ x :: l₂, r) = keeping P Q min (l₁ ++ l₂, r) := by
  refine keeping_sees_only_the_accepted (pl := (l₁ ++ x :: l₂, r)) (pl' := (l₁ ++ l₂, r)) ?_ rfl
  simp [List.filter_append, List.filter_cons, hx]

theorem filter_split (Q : α → Bool) : ∀ l : List α,
    (l.filter Q).length + (l.filter (fun x => !Q x)).length = l.length
  | [] => rfl
  | x :: l => by
    have ih := filter_split Q l
    cases h : Q x <;> simp [List.filter_cons, h] <;> omega

/-- **A contagem relatada é exata.** O que foi executado mais o que `fora` liga é a proposta
inteira; e tudo em `fora` foi proposto e recusado pela política do elemento. -/
theorem dropped_exact (Q : α → Bool) (pl : Plan α β) :
    (prune Q pl).1.length + (dropped Q pl).length = pl.1.length ∧
      ∀ x ∈ dropped Q pl, Q x = false ∧ x ∈ pl.1 := by
  refine ⟨filter_split Q pl.1, ?_⟩
  intro x hx
  have m := List.mem_filter.1 hx
  refine ⟨?_, m.1⟩
  cases q : Q x
  · rfl
  · simp [q] at m

/-! ## O defeito da RC7 -/

/-- **O resumo do modelo pode citar o que não foi feito.** Com o resumo como parte do plano
proposto (o resto `β`, aqui a lista das issues que ele cita), a política do plano da triagem
aceita, `keeping` deixa de fora a nota para a issue 3, e o resumo endossado ainda cita a 3; o
relatório composto não a cita. -/
theorem model_summary_can_misreport :
    ∃ pl out : Plan Nat (List Nat),
      keeping (fun p => safePlan (p.1, ())) inCadastro 0 pl = some out ∧ 3 ∈ out.2 ∧ 3 ∉ compose id out :=
  ⟨([1, 3, 2], [1, 2, 3]), ([1, 2], [1, 2, 3]), by decide, by decide, by decide⟩

/-! ## Os comprimentos que o verificador afirma

O texto é modelado como `List Char` (ou qualquer lista): `++` e `join` têm o comprimento que o
verificador põe no refinamento do resultado. -/

/-- `join(xs, sep)`. -/
def joinL (sep : List α) : List (List α) → List α
  | [] => []
  | [x] => x
  | x :: y :: rest => x ++ sep ++ joinL sep (y :: rest)

/-- `Text[..m] ++ Text[..n] : Text[..m+n]`. -/
theorem append_length_le {a b : List α} {m n : Nat} (ha : a.length ≤ m) (hb : b.length ≤ n) :
    (a ++ b).length ≤ m + n := by
  simp only [List.length_append]
  omega

theorem joinL_length_succ (sep : List α) (a : Nat) : ∀ (xs : List (List α)) (k : Nat),
    (∀ x ∈ xs, x.length ≤ a) → xs.length ≤ k + 1 → (joinL sep xs).length ≤ (k + 1) * a + k * sep.length
  | [], _, _, _ => by simp [joinL]
  | [x], k, h, _ => by
    have hx := h x (by simp)
    simp only [joinL, Nat.succ_mul]
    omega
  | x :: y :: rest, k, h, hl => by
    have hx := h x (by simp)
    obtain ⟨j, rfl⟩ : ∃ j, k = j + 1 := ⟨k - 1, by simp at hl; omega⟩
    have ih := joinL_length_succ sep a (y :: rest) j (fun z hz => h z (List.mem_cons_of_mem _ hz))
      (by simp at hl ⊢; omega)
    simp only [joinL, List.length_append, Nat.succ_mul, Nat.add_mul] at ih ⊢
    omega

/-- **`join(xs, sep)` com `xs : List[Text[..a], 0..hi]` cabe em `Text[..hi·a + (hi-1)·|sep|]`**,
o refinamento que o verificador dá ao resultado. -/
theorem joinL_length_le (sep : List α) {a hi : Nat} {xs : List (List α)}
    (h : ∀ x ∈ xs, x.length ≤ a) (hl : xs.length ≤ hi) :
    (joinL sep xs).length ≤ hi * a + (hi - 1) * sep.length := by
  cases hi with
  | zero =>
    have : xs = [] := List.length_eq_zero_iff.1 (Nat.le_zero.1 hl)
    subst this
    simp [joinL]
  | succ k =>
    have := joinL_length_succ sep a xs k h hl
    simpa using this

/-! ## Por que nada composto com `fora` chega a uma ação -/

/-- **`fora` é `Untrusted`, e o encontro com `Untrusted` é `Untrusted`.** Um texto composto com
qualquer parte de `fora` (`++`, `join`, `text(len(fora))`) tem rótulo `ℓ ⊓ U = U`, que não é
`low`: não passa a um parâmetro `Endorsed`, nem como condição de um `commit` (o `pc`). -/
theorem composed_with_dropped_is_not_low (ℓ : Lab) : ¬ Lab.low (ℓ ⊓ Lab.U) ∧ ¬ Lab.low (Lab.U ⊓ ℓ) := by
  cases ℓ <;> decide

end Aletheia.Report
