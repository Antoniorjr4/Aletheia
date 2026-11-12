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

/-- info: 'Aletheia.eval_fresh' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms eval_fresh

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

/-- info: 'Aletheia.effects_granted' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms effects_granted

/-- info: 'Aletheia.effects_lexical' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms effects_lexical

/-- info: 'Aletheia.caps_exactly_once' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms caps_exactly_once

/-- info: 'Aletheia.T2' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms T2

/-- info: 'Aletheia.Examples.remediate_runs' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms Examples.remediate_runs

/-- info: 'Aletheia.Examples.remediate_T2' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms Examples.remediate_T2

/-- info: 'Aletheia.Examples.injected_rejected' depends on axioms: [propext] -/
#guard_msgs in #print axioms Examples.injected_rejected

/-- info: 'Aletheia.Examples.steered_rejected' does not depend on any axioms -/
#guard_msgs in #print axioms Examples.steered_rejected

/-- info: 'Aletheia.Examples.deleting_rejected' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Examples.deleting_rejected

/-- info: 'Aletheia.Examples.twice_not_linear' does not depend on any axioms -/
#guard_msgs in #print axioms Examples.twice_not_linear

/-- info: 'Aletheia.Examples.looped_not_linear' does not depend on any axioms -/
#guard_msgs in #print axioms Examples.looped_not_linear

/-- info: 'Aletheia.Examples.oneBranch_not_linear' does not depend on any axioms -/
#guard_msgs in #print axioms Examples.oneBranch_not_linear

/-- info: 'Aletheia.Examples.discard_not_linear' does not depend on any axioms -/
#guard_msgs in #print axioms Examples.discard_not_linear

/-- info: 'Aletheia.Examples.bothBranches_linear' does not depend on any axioms -/
#guard_msgs in #print axioms Examples.bothBranches_linear

/-! ## Sagas (T6) e o invariante nos dois desfechos -/

/-- info: 'Aletheia.eval_preserves' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms eval_preserves

/-- info: 'Aletheia.T6_no_masking' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms T6_no_masking

/-- info: 'Aletheia.T6_recovered' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms T6_recovered

/-- info: 'Aletheia.T6' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms T6

/-- info: 'Aletheia.T6_saga' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms T6_saga

/-- info: 'Aletheia.SagaExample.saga_restores' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms SagaExample.saga_restores

/-! ## Sagas na tipagem: T1, T2 e T3 sobre `await` e `atomic` -/

/-- info: 'Aletheia.soundness_total' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms soundness_total

/-- info: 'Aletheia.guarded_no_timeout' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms guarded_no_timeout

/-- info: 'Aletheia.Partial.exit' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Partial.exit

/-- info: 'Aletheia.effects_lexical_total' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms effects_lexical_total

/-- info: 'Aletheia.confine_w' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms confine_w

/-- info: 'Aletheia.noninterference_saga' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms noninterference_saga

/-- info: 'Aletheia.noninterference_saga_ok' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms noninterference_saga_ok

/-- info: 'Aletheia.noninterference_saga_total' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms noninterference_saga_total

/-! ## Chaves e reconciliação (T9) -/

/-- info: 'Aletheia.keys_preserves' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms keys_preserves

/-- info: 'Aletheia.T9' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms T9

/-- info: 'Aletheia.T9_no_silent_discard' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms T9_no_silent_discard

/-- info: 'Aletheia.T9_done_observed' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms T9_done_observed

/-- info: 'Aletheia.T9_transferred_exhausted' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms T9_transferred_exhausted

/-! ## Rev11: o tratador de `TimeoutUnknown` como expressão do núcleo -/

/-- info: 'Aletheia.soundness_keyless' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms soundness_keyless

/-- info: 'Aletheia.pure_handler' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms pure_handler

/-- info: 'Aletheia.T2_keys' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms T2_keys

/-- info: 'Aletheia.caps_exactly_once_ok' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms caps_exactly_once_ok

/-- info: 'Aletheia.doCall_ni' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms doCall_ni

/-- info: 'Aletheia.noninterference_saga_keys' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms noninterference_saga_keys

/-- info: 'Aletheia.T9_outcome' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms T9_outcome

/-- info: 'Aletheia.T9_transfer_explicit' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms T9_transfer_explicit

/-- info: 'Aletheia.T9_honest' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms T9_honest

/-- info: 'Aletheia.KeysExample.transferred_but_applied' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms KeysExample.transferred_but_applied

/-! ## Rev12 -/

/-- info: 'Aletheia.next_preserves' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms next_preserves

/-- info: 'Aletheia.Lifecycle.pending_unique' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Lifecycle.pending_unique

/-- info: 'Aletheia.Arx.recA_eq' depends on axioms: [propext] -/
#guard_msgs in #print axioms Arx.recA_eq

/-- info: 'Aletheia.Arx.compA_sim' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Arx.compA_sim

/-- info: 'Aletheia.Arx.sim' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms Arx.sim

/-- info: 'Aletheia.Arx.arx_refines' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms Arx.arx_refines

/-- info: 'Aletheia.Arx.arx_sound' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms Arx.arx_sound

/-- info: 'Aletheia.Arx.Counter.comp_cost_breaks_budget' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Arx.Counter.comp_cost_breaks_budget

/-- info: 'Aletheia.Arx.Counter.impure_handler_reorders' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Arx.Counter.impure_handler_reorders

/-- info: 'Aletheia.Arx.Counter.top_level_await_differs' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Arx.Counter.top_level_await_differs

/-- info: 'Aletheia.T6_keys' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms T6_keys

/-- info: 'Aletheia.loggedK_preserves' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms loggedK_preserves

/-- info: 'Aletheia.KeyedSagaExample.refuted_not_compensated' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms KeyedSagaExample.refuted_not_compensated

/-- info: 'Aletheia.KeyedSagaExample.dishonest_refusal_strands' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms KeyedSagaExample.dishonest_refusal_strands

/-- info: 'Aletheia.Arx.arx_budget_closed' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms Arx.arx_budget_closed

/-- info: 'Aletheia.Arx.Counter.comp_cost_reserved' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Arx.Counter.comp_cost_reserved

/-- info: 'Aletheia.Arx.call_refuted' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Arx.call_refuted

/-- info: 'Aletheia.compensate_restoresK' depends on axioms: [propext] -/
#guard_msgs in #print axioms compensate_restoresK

/-- info: 'Aletheia.opsOf_keyless' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms opsOf_keyless

/-- info: 'Aletheia.KeyedSagaExample.blind_compensator_corrupts' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms KeyedSagaExample.blind_compensator_corrupts

/-- info: 'Aletheia.KeyedSagaExample.bound_compensator_restores' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms KeyedSagaExample.bound_compensator_restores

/-- info: 'Aletheia.ContractsExample.amplified_typed' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms ContractsExample.amplified_typed

/-- info: 'Aletheia.ContractsExample.policyOk_but_not_contract' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms ContractsExample.policyOk_but_not_contract

/-- info: 'Aletheia.Wal.no_silent_loss' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Wal.no_silent_loss

/-- info: 'Aletheia.Wal.evidence' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Wal.evidence

/-- info: 'Aletheia.Wal.at_most_once' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Wal.at_most_once

/-- info: 'Aletheia.Wal.opaque_escalates' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Wal.opaque_escalates

/-- info: 'Aletheia.Wal.registry_after_recovery' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Wal.registry_after_recovery

/-- info: 'Aletheia.Wal.opaque_not_exactly_once' does not depend on any axioms -/
#guard_msgs in #print axioms Wal.opaque_not_exactly_once

/-- info: 'Aletheia.Wal.idempotent_exactly_once' does not depend on any axioms -/
#guard_msgs in #print axioms Wal.idempotent_exactly_once

/-- info: 'Aletheia.Wal.resend_without_dedup_duplicates' does not depend on any axioms -/
#guard_msgs in #print axioms Wal.resend_without_dedup_duplicates

/-- info: 'Aletheia.Lifting.lifting_sound' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Lifting.lifting_sound

/-- info: 'Aletheia.Lifting.amplified_not_discharged' depends on axioms: [propext] -/
#guard_msgs in #print axioms Lifting.amplified_not_discharged

/-- info: 'Aletheia.Lifting.lifted_example' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Lifting.lifted_example

/-- info: 'Aletheia.Grant.exec_sound' depends on axioms: [propext] -/
#guard_msgs in #print axioms Grant.exec_sound

/-- info: 'Aletheia.Grant.exec_conserves' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms Grant.exec_conserves

/-- info: 'Aletheia.Grant.exec_linear' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms Grant.exec_linear

/-- info: 'Aletheia.Grant.other_run_refused' depends on axioms: [propext] -/
#guard_msgs in #print axioms Grant.other_run_refused

/-- info: 'Aletheia.Grant.refused_leaves_store' depends on axioms: [propext] -/
#guard_msgs in #print axioms Grant.refused_leaves_store

/-- info: 'Aletheia.Grant.cartesian_overgrants' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Grant.cartesian_overgrants

/-- info: 'Aletheia.Grant.two_payees_two_amounts' depends on axioms: [propext] -/
#guard_msgs in #print axioms Grant.two_payees_two_amounts

/-- info: 'Aletheia.Grant.relation_is_reusable' depends on axioms: [propext] -/
#guard_msgs in #print axioms Grant.relation_is_reusable

-- v1.0-RC2: concessão e intenção, uma transação (`Aletheia/Txn.lean`)

/-- info: 'Aletheia.Txn.spent_has_intent' depends on axioms: [propext] -/
#guard_msgs in #print axioms Txn.spent_has_intent

/-- info: 'Aletheia.Txn.at_most_one_sender' depends on axioms: [propext] -/
#guard_msgs in #print axioms Txn.at_most_one_sender

/-- info: 'Aletheia.Txn.unsent_never_left' depends on axioms: [propext] -/
#guard_msgs in #print axioms Txn.unsent_never_left

/-- info: 'Aletheia.Txn.nothing_never_left' depends on axioms: [propext] -/
#guard_msgs in #print axioms Txn.nothing_never_left

/-- info: 'Aletheia.Txn.consumed_is_recoverable' depends on axioms: [propext] -/
#guard_msgs in #print axioms Txn.consumed_is_recoverable

/-- info: 'Aletheia.Txn.unsent_leaves_grant' depends on axioms: [propext] -/
#guard_msgs in #print axioms Txn.unsent_leaves_grant

/-- info: 'Aletheia.Txn.crash_reachable' does not depend on any axioms -/
#guard_msgs in #print axioms Txn.crash_reachable

/-- info: 'Aletheia.Txn.expected_is_recover' depends on axioms: [propext] -/
#guard_msgs in #print axioms Txn.expected_is_recover

/-- info: 'Aletheia.Txn.rc1_window' depends on axioms: [propext] -/
#guard_msgs in #print axioms Txn.rc1_window

/-- info: 'Aletheia.Txn.rc1_race' depends on axioms: [propext] -/
#guard_msgs in #print axioms Txn.rc1_race

-- v1.0-RC3: autoridade durável (`Aletheia/Anchor.lean`)

/-- info: 'Aletheia.Anchor.anchor_monotone' depends on axioms: [propext] -/
#guard_msgs in #print axioms Anchor.anchor_monotone

/-- info: 'Aletheia.Anchor.sent_anchored' depends on axioms: [propext] -/
#guard_msgs in #print axioms Anchor.sent_anchored

/-- info: 'Aletheia.Anchor.accepted_keeps_spent' depends on axioms: [propext] -/
#guard_msgs in #print axioms Anchor.accepted_keeps_spent

/-- info: 'Aletheia.Anchor.rollback_rejected' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Anchor.rollback_rejected

/-- info: 'Aletheia.Anchor.fork_rejected' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms Anchor.fork_rejected

/-- info: 'Aletheia.Anchor.rc2_chain_accepts_rollback' depends on axioms: [propext] -/
#guard_msgs in #print axioms Anchor.rc2_chain_accepts_rollback

/-- info: 'Aletheia.Anchor.rc3_rejects_that_rollback' depends on axioms: [propext] -/
#guard_msgs in #print axioms Anchor.rc3_rejects_that_rollback

/-- info: 'Aletheia.Anchor.fresh_is_recent' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Anchor.fresh_is_recent

/-- info: 'Aletheia.Anchor.fresh_is_scoped' does not depend on any axioms -/
#guard_msgs in #print axioms Anchor.fresh_is_scoped

/-- info: 'Aletheia.Anchor.other_run_is_stale' does not depend on any axioms -/
#guard_msgs in #print axioms Anchor.other_run_is_stale

/-- info: 'Aletheia.Anchor.expired_stays_expired' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms Anchor.expired_stays_expired
