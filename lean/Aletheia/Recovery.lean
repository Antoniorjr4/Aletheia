import Aletheia.Eval

/-!
# Rev6: primeiro nucleo formal de recuperacao de sagas (T6) e reconciliacao (T9)

A especificacao v0.2, seções 3.4-3.5, distingue:
- compensar de desfazer: a pos-condicao deve satisfazer um invariante R;
- timeout de falha: resultado remoto desconhecido requer reconciliacao ou transferencia;
- recuperacao observada de recuperacao apenas presumida.

Este modulo e um modelo abstrato inicial, NAO uma prova de refinamento do ARX.
Nao afirma a correção do provedor remoto nem a persistencia fisica do journal.
As hipoteses de contrato e de verificacao sao declaradas explicitamente.
-/

namespace Aletheia
namespace Recovery

universe u

/-- Prova composicional da obrigacao de compensacao da spec 3.4.
    O contrato deve cobrir tanto 'aconteceu' (Q) quanto 'nao aconteceu' (P). -/
theorem compensation_restores {W : Type u}
    (P Q R Inv : W → Prop) (compensate : W → W)
    (hcomp : ∀ w, P w ∨ Q w → R (compensate w))
    (hInv : ∀ w, R w → Inv w)
    (w : W) (hPossible : P w ∨ Q w) :
    Inv (compensate w) :=
  hInv _ (hcomp _ hPossible)

/-- Uma sonda da recuperacao pode confirmar, reprovar ou continuar inconclusiva. -/
inductive Probe where
  | done | failed | unknown
  deriving DecidableEq, Repr

/-- Nenhuma reconciliacao encerra como 'unknown': transfere/escalona o pendente. -/
inductive Resolution where
  | done | failed | escalated
  deriving DecidableEq, Repr

/-- Reconciliacao com combustivel finito (spec 3.5): se a ultima sonda
    ainda e inconclusiva, o token Pending deve ser entregue ao reconciliador
    duravel. A funcao da sonda representa observacoes confiaveis do provedor. -/
def resolve : Nat → (Nat → Probe) → Resolution
  | 0, observe =>
    match observe 0 with
    | .done => .done
    | .failed => .failed
    | .unknown => .escalated
  | Nat.succ fuel, observe =>
    match observe (Nat.succ fuel) with
    | .done => .done
    | .failed => .failed
    | .unknown => resolve fuel observe

/-- Um provedor perpetuamente inconclusivo nunca vira sucesso por timeout. -/
theorem unknown_escalates (fuel : Nat) :
    resolve fuel (fun _ => Probe.unknown) = .escalated := by
  induction fuel with
  | zero => rfl
  | succ n ih => simpa [resolve] using ih

/-- Sucesso declarado exige evidência de pelo menos uma sonda 'done'. -/
theorem done_requires_evidence (fuel : Nat) (observe : Nat → Probe) :
    resolve fuel observe = .done →
      ∃ n, n ≤ fuel ∧ observe n = .done := by
  induction fuel with
  | zero =>
    intro h
    cases hp : observe 0 with
    | done => exact ⟨0, Nat.le_refl _, hp⟩
    | failed => simp [resolve, hp] at h
    | unknown => simp [resolve, hp] at h
  | succ fuel ih =>
    intro h
    cases hp : observe (Nat.succ fuel) with
    | done => exact ⟨Nat.succ fuel, Nat.le_refl _, hp⟩
    | failed => simp [resolve, hp] at h
    | unknown =>
      have h' : resolve fuel observe = .done := by
        simpa [resolve, hp] using h
      obtain ⟨n, hn, he⟩ := ih h'
      exact ⟨n, Nat.le_trans hn (Nat.le_succ fuel), he⟩

/-- Estado terminal da compensacao. Uma sonda reprovada nao pode ser
    mascarada como recuperacao bem-sucedida. -/
inductive CompensationResult where
  | recovered | compensationFailed
  deriving DecidableEq, Repr

/-- Execucao abstrata de C com sonda decisivel e confiavel do invariante. -/
def compensateOrEscalate {W : Type u}
    (compensate : W → Option W) (verify : W → Bool) (w : W) : CompensationResult :=
  match compensate w with
  | none => .compensationFailed
  | some w' =>
    if verify w' then .recovered else .compensationFailed

/-- Se o runtime reportou recuperacao, houve resultado de C e sonda positiva. -/
theorem recovered_is_verified {W : Type u}
    (compensate : W → Option W) (verify : W → Bool) (w : W)
    (h : compensateOrEscalate compensate verify w = .recovered) :
    ∃ w', compensate w = some w' ∧ verify w' = true := by
  cases hc : compensate w with
  | none => simp [compensateOrEscalate, hc] at h
  | some w' =>
    cases hv : verify w' with
    | false => simp [compensateOrEscalate, hc, hv] at h
    | true => exact ⟨w', hc, hv⟩

/-- T6, nucleo de seguranca: recuperacao notificada implica Inv no mundo
    compensado, DESDE QUE a sonda positiva seja correta. -/
theorem recovered_restores {W : Type u}
    (Inv : W → Prop) (compensate : W → Option W)
    (verify : W → Bool)
    (hVerify : ∀ w, verify w = true → Inv w)
    (w : W)
    (h : compensateOrEscalate compensate verify w = .recovered) :
    ∃ w', compensate w = some w' ∧ Inv w' := by
  obtain ⟨w', hc, hv⟩ := recovered_is_verified compensate verify w h
  exact ⟨w', hc, hVerify w' hv⟩

end Recovery
end Aletheia
