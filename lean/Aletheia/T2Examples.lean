import Aletheia.Confinement
import Aletheia.Examples

/-!
# Rev6 / T2: exemplos executaveis da guarda Cap[E]

Testes positivos e negativos sob a semantica total.
A autorizacao e associada a identidade concreta da capacidade, nao
a um texto fornecido pelo modelo.
-/

namespace Aletheia.T2Examples

def granted : Authority where
  effectOf := fun tool => tool
  grants := fun id => if id == 7 then [0] else []

def revoked : Authority where
  effectOf := fun tool => tool
  grants := fun _ => []

def scope : List ScopedBind := [.data, .cap [0]]
def noScope : List ScopedBind := [.data, .cap []]

/-- O programa original faz uma unica acao do tipo autorizado. -/
example : scopeCheck granted scope Examples.remediate = true := by decide

/-- Nenhuma ferramenta passa pelo checker com Cap[] no lugar de Cap[0]. -/
example : scopeCheck granted noScope Examples.remediate = false := by decide

/-- A mesma remediacao do T1/T3, mas com autoridade dinamica conferida. -/
example :
    (runScoped Examples.S ⟨Examples.canned 5, Examples.limit⟩ granted
      Examples.remediate scope Examples.ρ₀ Examples.s₀).map (·.tr) =
      .ok [.endorse 0 (some (.int 5)), .act 0 7 (.int 5)] := by rfl

/-- Falha no checker impede qualquer execucao. -/
example :
    runScoped Examples.S ⟨Examples.canned 5, Examples.limit⟩ granted
      Examples.remediate noScope Examples.ρ₀ Examples.s₀ = .error .noCap := by rfl

/-- A tabela de capacidades do runtime prevalece sobre Cap[E] estatico. -/
example :
    runScoped Examples.S ⟨Examples.canned 5, Examples.limit⟩ revoked
      Examples.remediate scope Examples.ρ₀ Examples.s₀ = .error .noCap := by rfl

end Aletheia.T2Examples
