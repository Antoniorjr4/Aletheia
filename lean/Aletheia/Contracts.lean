import Aletheia.Examples
import Aletheia.Noninterference

/-!
# M4: por que o contrato fica na fronteira do efeito

A Rev11 deixou três contraexemplos: um valor inflado a um beneficiário legítimo (R1), um
endereço do atacante (R2) e uma política que aceita tudo e compila (RS1). Os três têm a mesma
raiz, que este arquivo demonstra: **o rótulo `Endorsed` é fechado sob computação confiável,
mas o predicado da política não é.** O T3 garante que só valores endossados (ou confiáveis)
decidem efeitos, e o `PolicyOk` garante que todo valor endossado satisfaz a política no
momento do `endorse`. Nada disso diz que o argumento do efeito satisfaz a política: o programa
pode transformar o valor endossado depois, com o rótulo preservado.

`endorsed_amplified` é um programa **bem tipado**, linear, que passa por T1 e T3, em que a
política aceita `n = 64` e o efeito recebe `64 + 64000`, fora da política. Nenhuma propriedade
de fluxo de informação pode recusá-lo; só uma condição avaliada sobre o argumento real, no
momento do efeito, pode. É o contrato `requires` (Rev12), implementado no ARX e no runtime
Python como monitor antes do host (`R0630`), com o mesmo journal nos dois
(`crates/athc/tests/differential.rs`, `contracts_monitor`) e avaliado em
`bench/adversarial/results/CONTRACTS.md`.
-/

namespace Aletheia.ContractsExample

open Aletheia Aletheia.Examples Lab Budget

/-- O agente da remediação com um passo a mais: o valor endossado é somado a uma constante
antes de virar argumento da ação. -/
def amplified : Exp :=
  .let_ (.infer 0 .int (.var 0))
    (.let_ (.endorse 0 (.var 0) (.val (.int 3)))
      (.act 0 3 (.prim .add (.var 0) (.val (.int 64000)))))

/-- **Bem tipado.** O argumento da ação tem rótulo `E ⊓ T = E`: endossado. -/
theorem amplified_typed :
    HasType S Γ .T amplified .unit .T ((0 + S.mcost 0) + ((0 + 0) + ((0 + 0) + S.cost 0))) :=
  .let_ (.infer (.var rfl))
    (.let_ (.endorse (by decide) (.var rfl) (.val .int))
      (.act (by decide) rfl (by decide) (.prim (.var rfl) (.val .int)) (by decide)))

theorem amplified_linear : Linear Γ amplified := by
  intro k E hk
  match k, hk with
  | 1, _ => decide

/-- **A política aceitou; o efeito a viola.** Com o modelo respondendo 64, a política
`SafeScale` (2 ≤ n ≤ 64) endossa 64, e o traço registra a ação com 64064, que a mesma
política recusaria. -/
theorem endorsed_amplified :
    (eval S (cfg 64) amplified ρ₀ s₀).map (·.tr) =
      .ok [.endorse 0 (some (.int 64)), .act 0 7 (.int 64064)] ∧
    S.pol 0 (.int 64) = true ∧ S.pol 0 (.int 64064) = false := ⟨rfl, rfl, rfl⟩

/-- O `PolicyOk` do traço vale (o endosso foi honesto), e mesmo assim o argumento do efeito
está fora da política: o T3 e o `PolicyOk` não implicam o contrato. -/
theorem policyOk_but_not_contract :
    (∀ tr, (eval S (cfg 64) amplified ρ₀ s₀).map (·.tr) = .ok tr → PolicyOk S tr) ∧
    ∃ tr id v, (eval S (cfg 64) amplified ρ₀ s₀).map (·.tr) = .ok tr ∧ Ev.act 0 id v ∈ tr ∧
      S.pol 0 v = false := by
  refine ⟨fun tr h => ?_, ⟨_, 7, .int 64064, endorsed_amplified.1, by simp, rfl⟩⟩
  rw [endorsed_amplified.1] at h
  cases h
  intro p x hx
  simp at hx
  obtain ⟨rfl, rfl⟩ := hx
  rfl

end Aletheia.ContractsExample
