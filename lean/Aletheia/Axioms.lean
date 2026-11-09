import Aletheia

/-!
# Auditoria de axiomas

Cada teorema principal tem o conjunto de axiomas fixado aqui. Se uma prova passar a depender
de `sorryAx` (um `sorry`) ou de qualquer axioma novo, a mensagem muda e o build falha. Os
três axiomas que aparecem são os padrões do Lean, usados por `simp`, `omega` e `by_cases`:
`propext`, `Quot.sound` e `Classical.choice`.
-/

open Aletheia

/-- info: 'Aletheia.soundness' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms soundness

/-- info: 'Aletheia.progress' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms progress

/-- info: 'Aletheia.preservation' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms preservation

/-- info: 'Aletheia.eval_nodup' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms eval_nodup

/-- info: 'Aletheia.confine' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms confine

/-- info: 'Aletheia.noninterference' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms noninterference

/-- info: 'Aletheia.noninterference_total' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms noninterference_total

/-- info: 'Aletheia.robust_endorsement' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms robust_endorsement

/-- info: 'Aletheia.actions_agree_until_endorsement' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms actions_agree_until_endorsement

/-- info: 'Aletheia.endorsed_sound' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms endorsed_sound

/-- info: 'Aletheia.Examples.remediate_runs' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms Examples.remediate_runs

/-- info: 'Aletheia.Examples.injected_rejected' depends on axioms: [propext] -/
#guard_msgs in #print axioms Examples.injected_rejected

/-- info: 'Aletheia.Examples.steered_rejected' does not depend on any axioms -/
#guard_msgs in #print axioms Examples.steered_rejected

/-- info: 'Aletheia.Examples.twice_not_linear' does not depend on any axioms -/
#guard_msgs in #print axioms Examples.twice_not_linear

/-- info: 'Aletheia.Examples.looped_not_linear' does not depend on any axioms -/
#guard_msgs in #print axioms Examples.looped_not_linear
