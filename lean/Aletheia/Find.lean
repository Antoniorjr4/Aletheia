/-!
# v1.0-RC6: a consulta por chave numa API que não conhece chaves

O modelo de `BoundHost::status_with` (`.key`, `.find`, `.settle`). O provedor cria um item a
cada requisição, sem deduplicar; cada item carrega o marcador da chave que o criou, e só aparece
na listagem algum tempo depois de criado.

| `arx::bind` | aqui |
|---|---|
| o item criado, com o marcador `<!-- aletheia:execução:kN -->` | `Item`, `key` |
| a listagem que `.find` consulta, no instante `t` | `listed w t` |
| `Link: rel="next"` (há mais páginas) | `more` |
| a consulta, a espera de `.settle` segundos e a nova consulta | `decide` |

## Hipóteses

* **G1 listagem**: todo item aparece na listagem em até `S` segundos depois de criado (`S` é o
  `.settle` declarado). É uma hipótese sobre o provedor; o programa a declara.
* **G2 marcador**: um item com o marcador da chave `k` foi criado pela operação de chave `k`, e
  o marcador não é removido (o marcador identifica, não autentica).
* **G3 nada em voo**: quando a consulta começa, nenhuma requisição da operação interrompida
  ainda vai chegar ao provedor (o executor que a enviou morreu, e a rede não a entrega depois).

Os três contraexemplos no fim mostram que nenhuma das hipóteses de tempo é dispensável: sem
espera, com atraso maior que `S`, ou com uma requisição ainda em voo, a consulta conclui "não
aplicada" sobre uma operação que existe, e a retomada duplica.
-/

namespace Aletheia.Find

/-- Um item no provedor: a chave do marcador, quando foi criado e quando passa a ser listado. -/
structure Item where
  key : Nat
  created : Nat
  visible : Nat

/-- O provedor: tudo o que foi (ou será) criado. -/
abbrev World := List Item

/-- A listagem no instante `t`: só o que já é visível. -/
def listed (w : World) (t : Nat) : List Item := w.filter (fun i => decide (i.visible ≤ t))

/-- A consulta acha a chave `k` no instante `t`. -/
def found (w : World) (t k : Nat) : Bool := (listed w t).any (fun i => i.key == k)

inductive Verdict where
  | done
  | notApplied
  | unknown
  deriving DecidableEq

/-- A consulta por chave: acha → `done`; não acha e há mais páginas → `unknown`; não acha →
espera `S` e consulta de novo; só então `notApplied`. -/
def verdict (w : World) (t S : Nat) (more : Bool) (k : Nat) : Verdict :=
  if found w t k then .done
  else if more then .unknown
  else if found w (t + S) k then .done
  else .notApplied

/-- G1: todo item é listado em até `S` depois de criado. -/
def Settles (w : World) (S : Nat) : Prop := ∀ i ∈ w, i.visible ≤ i.created + S

/-- G3: tudo o que tem a chave `k` foi criado até `t` (nada da operação ainda está em voo). -/
def Landed (w : World) (t k : Nat) : Prop := ∀ i ∈ w, i.key = k → i.created ≤ t

theorem found_iff (w : World) (t k : Nat) :
    found w t k = true ↔ ∃ i ∈ w, i.visible ≤ t ∧ i.key = k := by
  unfold found listed
  rw [List.any_eq_true]
  constructor
  · rintro ⟨i, hi, hk⟩
    rw [List.mem_filter] at hi
    exact ⟨i, hi.1, of_decide_eq_true hi.2, by simpa using hk⟩
  · rintro ⟨i, hi, hv, hk⟩
    exact ⟨i, List.mem_filter.2 ⟨hi, decide_eq_true hv⟩, by simp [hk]⟩

/-- **`done` tem evidência.** Se a consulta responde `done`, existe no provedor um item com o
marcador da chave (sob G2, é a operação). -/
theorem done_has_evidence {w : World} {t S k : Nat} {more : Bool}
    (h : verdict w t S more k = .done) : ∃ i ∈ w, i.key = k := by
  unfold verdict at h
  by_cases a : found w t k = true
  · obtain ⟨i, hi, _, hk⟩ := (found_iff w t k).1 a; exact ⟨i, hi, hk⟩
  · simp only [a, Bool.false_eq_true, if_false] at h
    by_cases m : more = true
    · simp [m] at h
    · simp only [m, Bool.false_eq_true, if_false] at h
      by_cases b : found w (t + S) k = true
      · obtain ⟨i, hi, _, hk⟩ := (found_iff w (t + S) k).1 b; exact ⟨i, hi, hk⟩
      · simp [b] at h

/-- **Ausência depois da espera é prova de ausência.** Sob G1 e G3, se a consulta responde
"não aplicada", não existe no provedor item algum com a chave: a operação não aconteceu. -/
theorem not_applied_is_sound {w : World} {t S k : Nat} {more : Bool}
    (g1 : Settles w S) (g3 : Landed w t k) (h : verdict w t S more k = .notApplied) :
    ∀ i ∈ w, i.key ≠ k := by
  intro i hi hk
  have hv : i.visible ≤ t + S := by
    have a := g1 i hi
    have b := g3 i hi hk
    omega
  have f : found w (t + S) k = true := (found_iff w (t + S) k).2 ⟨i, hi, hv, hk⟩
  unfold verdict at h
  by_cases a : found w t k = true
  · simp [a] at h
  · simp only [a, Bool.false_eq_true, if_false] at h
    by_cases m : more = true
    · simp [m] at h
    · simp [m, f] at h

/-- **Uma página incompleta nunca conclui "não aplicada".** -/
theorem more_pages_never_not_applied (w : World) (t S k : Nat) :
    verdict w t S true k ≠ .notApplied := by
  unfold verdict
  by_cases a : found w t k = true <;> simp [a]

/-- **A retomada não duplica.** Se a consulta responde "não aplicada" e a operação é refeita
com uma chave nova `k'`, o provedor fica com exatamente um item da operação (das chaves `k` e
`k'`), qualquer que seja o instante em que o novo item é criado e listado. -/
theorem resend_creates_exactly_one {w : World} {t S k k' : Nat} {more : Bool}
    (g1 : Settles w S) (g3 : Landed w t k) (h : verdict w t S more k = .notApplied)
    (fresh : ∀ i ∈ w, i.key ≠ k') (c v : Nat) :
    ((⟨k', c, v⟩ :: w).filter (fun i => decide (i.key = k ∨ i.key = k'))).length = 1 := by
  have none : w.filter (fun i => decide (i.key = k ∨ i.key = k')) = [] := by
    apply List.filter_eq_nil_iff.2
    intro i hi
    have a := not_applied_is_sound g1 g3 h i hi
    have b := fresh i hi
    simp [a, b]
  rw [List.filter_cons, none]
  simp

/-! ## As hipóteses são necessárias -/

/-- **Sem espera, duplica.** O item existe e vai aparecer dentro do prazo (G1 e G3 valem), mas
uma consulta sem `.settle` não o vê e conclui "não aplicada". Foi o que a listagem de issues
do GitHub mostrou na primeira execução. -/
theorem without_settle_it_duplicates :
    ∃ (w : World) (t k S : Nat), Settles w S ∧ Landed w t k ∧
      verdict w t 0 false k = .notApplied ∧ ∃ i ∈ w, i.key = k :=
  ⟨[⟨7, 0, 2⟩], 0, 7, 2, by simp [Settles], by simp [Landed], by decide, ⟨⟨7, 0, 2⟩, by simp, rfl⟩⟩

/-- **Se a listagem atrasa mais que `.settle`, duplica.** G1 é uma hipótese de verdade: com um
atraso maior que o declarado, a consulta com espera também erra. -/
theorem lag_beyond_settle_duplicates :
    ∃ (w : World) (t k S : Nat), ¬ Settles w S ∧ Landed w t k ∧
      verdict w t S false k = .notApplied ∧ ∃ i ∈ w, i.key = k :=
  ⟨[⟨7, 0, 5⟩], 0, 7, 2, by simp [Settles], by simp [Landed], by decide, ⟨⟨7, 0, 5⟩, by simp, rfl⟩⟩

/-- **Uma requisição ainda em voo duplica.** G3 também: se a requisição da operação
interrompida chega ao provedor depois da consulta, a consulta não tinha como vê-la. -/
theorem in_flight_request_duplicates :
    ∃ (w : World) (t k S : Nat), Settles w S ∧ ¬ Landed w t k ∧
      verdict w t S false k = .notApplied ∧ ∃ i ∈ w, i.key = k :=
  ⟨[⟨7, 9, 9⟩], 0, 7, 2, by simp [Settles], by simp [Landed], by decide, ⟨⟨7, 9, 9⟩, by simp, rfl⟩⟩

end Aletheia.Find
