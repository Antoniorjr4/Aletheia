/-!
# v1.0-RC3: autoridade durável — o registro de consumo, a âncora e o prazo

No RC2 o registro de consumo era confiado: uma cadeia de hashes só diz que o que está ali é
coerente, e um **prefixo** de uma cadeia íntegra é uma cadeia íntegra. Quem escreve no disco do
executor (ou restaura uma cópia antiga) apagava consumos e as concessões voltavam a valer
(`rc2_chain_accepts_rollback`).

O RC3 acrescenta uma **âncora**: um estado fora do disco do executor que só avança, um consumo
de cada vez. O protocolo (`arx::grant::{begin, commit, seal}`, `arx::anchor`):

1. `spend`  com o registro em dia com a âncora, grava o consumo no registro;
2. `fix`    (`seal` no ARX) avança a âncora para a marca do registro (o registro é a âncora mais um consumo);
3. `send`   só envia o efeito de um consumo **ancorado**.

O adversário (`tamper`) troca o registro por qualquer outro: apaga o fim, apaga tudo, reescreve.
Não toca na âncora. A conferência (`accepts`) é a de `arx::grant::settle`: o registro é a âncora,
ou a âncora mais um consumo (a queda entre `spend` e `fix`).

Provado sobre todo estado alcançável, **com o adversário agindo quando quiser**:

* `anchor_monotone`: a âncora só cresce;
* `sent_anchored`: todo efeito enviado tem o consumo ancorado;
* `accepted_keeps_spent`: **um registro aceito contém o consumo de todo efeito enviado**.
  Nenhum registro que passe pela conferência reintegra uma concessão cujo efeito saiu;
* `rollback_rejected`: um registro mais curto que a âncora nunca é aceito;
* `fork_rejected`: um registro do tamanho da âncora e diferente dela nunca é aceito.

O modelo identifica o registro pelo seu conteúdo: é a hipótese de que o hash da cadeia não
colide (a âncora guarda o tamanho e o hash da cabeça, não a lista). O que o modelo **não**
promete: o adversário pode acrescentar um consumo forjado e vê-lo ancorado (a âncora adianta um
passo). Isso queima uma concessão; nunca devolve uma.

A segunda parte é o prazo: `fresh_is_recent` diz que o que a conferência de validade aceita foi
emitido há no máximo `max_age` segundos, e `fresh_is_scoped` que foi emitido para este programa
e esta execução. A autenticidade da assinatura, a monotonicidade real da âncora e o relógio são
hipóteses do sistema (seção de limitações de `docs/RC3.md`).
-/

namespace Aletheia.Anchor

/-- O registro de consumo no disco, a âncora e os consumos cujo efeito já foi enviado. -/
structure Sys where
  ledger : List Nat
  anchor : List Nat
  sent : List Nat

def init : Sys := ⟨[], [], []⟩

inductive Step : Sys → Sys → Prop
  | spend (s : Sys) (x : Nat) : s.ledger = s.anchor → Step s { s with ledger := s.ledger ++ [x] }
  | fix (s : Sys) (x : Nat) : s.ledger = s.anchor ++ [x] → Step s { s with anchor := s.ledger }
  | send (s : Sys) (x : Nat) : x ∈ s.anchor → Step s { s with sent := x :: s.sent }
  /-- O adversário: o registro passa a ser qualquer coisa. -/
  | tamper (s : Sys) (l : List Nat) : Step s { s with ledger := l }

inductive Reach : Sys → Prop
  | init : Reach init
  | step {s t : Sys} : Reach s → Step s t → Reach t

/-- A conferência de `settle`: em dia, ou um consumo à frente. -/
def accepts (ledger anchor : List Nat) : Prop :=
  ledger = anchor ∨ ∃ x, ledger = anchor ++ [x]

/-- A âncora só cresce. -/
theorem anchor_monotone {s t : Sys} (h : Step s t) : ∃ k, t.anchor = s.anchor ++ k := by
  cases h with
  | spend x _ => exact ⟨[], by simp⟩
  | fix x hl => exact ⟨[x], by simp [hl]⟩
  | send x _ => exact ⟨[], by simp⟩
  | tamper l => exact ⟨[], by simp⟩

/-- Todo efeito enviado tem o consumo ancorado. -/
theorem sent_anchored {s : Sys} (h : Reach s) : ∀ x ∈ s.sent, x ∈ s.anchor := by
  induction h with
  | init => intro x hx; simp [init] at hx
  | step _ st ih =>
    cases st with
    | spend y _ => exact ih
    | tamper l => exact ih
    | fix y hl =>
      intro x hx
      have := ih x hx
      simp [hl, this]
    | send y hy =>
      intro x hx
      simp at hx
      rcases hx with hx | hx
      · subst hx; exact hy
      · exact ih x hx

/-- **Um registro aceito contém o consumo de todo efeito enviado.** Não importa o que o
adversário tenha feito com o disco: se a conferência passa, nenhuma concessão cujo efeito saiu
está livre de novo. -/
theorem accepted_keeps_spent {s : Sys} (h : Reach s) (ha : accepts s.ledger s.anchor) :
    ∀ x ∈ s.sent, x ∈ s.ledger := by
  intro x hx
  have hanch := sent_anchored h x hx
  rcases ha with ha | ⟨y, ha⟩
  · rw [ha]; exact hanch
  · rw [ha]; exact List.mem_append_left _ hanch

/-- O retrocesso: um registro mais curto que a âncora nunca é aceito (`L0002`). -/
theorem rollback_rejected {ledger anchor : List Nat} (h : ledger.length < anchor.length) :
    ¬ accepts ledger anchor := by
  intro ha
  rcases ha with ha | ⟨y, ha⟩
  · rw [ha] at h; exact Nat.lt_irrefl _ h
  · rw [ha] at h; simp at h; omega

/-- A bifurcação: do tamanho da âncora e diferente dela, nunca é aceito (`L0003`). -/
theorem fork_rejected {ledger anchor : List Nat} (hl : ledger.length = anchor.length)
    (hne : ledger ≠ anchor) : ¬ accepts ledger anchor := by
  intro ha
  rcases ha with ha | ⟨y, ha⟩
  · exact hne ha
  · rw [ha] at hl; simp at hl

/-- **O contraexemplo do RC2.** A conferência do RC2 era a cadeia: qualquer prefixo do registro
verdadeiro passa. Há um estado alcançável em que o efeito do consumo `7` saiu, o registro em
disco é um prefixo (vazio) do verdadeiro, e `7` não está nele: a concessão voltou. -/
theorem rc2_chain_accepts_rollback :
    ∃ s, Reach s ∧ 7 ∈ s.sent ∧ (∃ k, s.anchor = s.ledger ++ k) ∧ 7 ∉ s.ledger := by
  let a : Sys := { init with ledger := init.ledger ++ [7] }
  let b : Sys := { a with anchor := a.ledger }
  let c : Sys := { b with sent := 7 :: b.sent }
  let d : Sys := { c with ledger := [] }
  have r : Reach d := by
    refine .step (.step (.step (.step .init (.spend init 7 rfl)) (.fix a 7 ?_)) (.send b 7 ?_)) (.tamper c [])
    · simp [a, init]
    · simp [b, a, init]
  refine ⟨d, r, ?_, ⟨[7], ?_⟩, ?_⟩
  · simp [d, c]
  · simp [d, c, b, a, init]
  · simp [d]

/-- E o RC3 recusa exatamente esse estado. -/
theorem rc3_rejects_that_rollback {s : Sys} (h : Reach s) (hx : 7 ∈ s.sent) (hl : s.ledger = []) :
    ¬ accepts s.ledger s.anchor := by
  intro ha
  have := accepted_keeps_spent h ha 7 hx
  rw [hl] at this
  simp at this

/-! ## Prazo e escopo do que a autoridade assina -/

/-- O carimbo de uma assinatura: programa, execução, emissão e expiração. -/
structure Stamp where
  program : Nat
  run : Nat
  issued : Nat
  expires : Nat

/-- A conferência de `Arx::attest` (sem a parte criptográfica). -/
def fresh (program run now maxAge : Nat) (st : Stamp) : Prop :=
  st.program = program ∧ st.run = run ∧ st.issued ≤ now ∧ now ≤ st.expires ∧
    st.expires - st.issued ≤ maxAge

/-- O que é aceito foi emitido há no máximo `max_age`. -/
theorem fresh_is_recent {program run now maxAge : Nat} {st : Stamp}
    (h : fresh program run now maxAge st) : now - st.issued ≤ maxAge := by
  obtain ⟨_, _, _, h2, h3⟩ := h
  omega

/-- O que é aceito foi emitido para este programa e esta execução. -/
theorem fresh_is_scoped {program run now maxAge : Nat} {st : Stamp}
    (h : fresh program run now maxAge st) : st.program = program ∧ st.run = run :=
  ⟨h.1, h.2.1⟩

/-- Uma fotografia de outra execução nunca é aceita, por mais recente que seja. -/
theorem other_run_is_stale {program run now maxAge : Nat} {st : Stamp} (h : st.run ≠ run) :
    ¬ fresh program run now maxAge st :=
  fun hf => h hf.2.1

/-- Depois de expirar, nunca mais é aceita: o tempo não a revalida. -/
theorem expired_stays_expired {program run now later maxAge : Nat} {st : Stamp}
    (h : st.expires < now) (hl : now ≤ later) : ¬ fresh program run later maxAge st := by
  intro hf
  obtain ⟨_, _, _, h2, _⟩ := hf
  omega

end Aletheia.Anchor
