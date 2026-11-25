import Aletheia.Lattice

/-!
# v1.0-RC10: confidencialidade e a não-interferência do fluxo tóxico

O reticulado de confidencialidade `Public ⊑ Internal ⊑ Secret` (`ath_syntax::ast::Conf`), dual
da integridade: a integridade diz de onde o dado veio (se pode escolher ação); a
confidencialidade diz para onde pode ir (se pode sair por um canal). Aqui, a junção `⊔`
(supremo) propaga pela derivação, e um canal que aceita `c` recusa tudo acima de `c`.

O teorema central, `no_toxic_flow`, é a garantia contra o "lethal trifecta": o valor que chega a
um sink público é função apenas das entradas públicas do programa — o conteúdo secreto não o
altera. É a não-interferência que o controle arquitetural exige (e que o `athc` faz por tipo:
`E0440`).

| `ath-check` | aqui |
|---|---|
| `Conf` (`conf`) | `Conf` |
| junção por derivação (`join_conf_from`) | `join` / `expr.conf` |
| `pc` de confidencialidade | `Pc` no avaliador abstrato |
| o sink recusa `at.conf ⊔ pc > allow` (`E0440`) | `accepts` |
-/

namespace Aletheia

inductive Conf where
  | Public
  | Internal
  | Secret
  deriving DecidableEq, Repr

namespace Conf

def rank : Conf → Nat
  | Public => 0
  | Internal => 1
  | Secret => 2

/-- Ordem do reticulado: `a ⊑ b` (a é no máximo tão secreto quanto b). -/
def le (a b : Conf) : Prop := a.rank ≤ b.rank
instance : LE Conf := ⟨le⟩
instance (a b : Conf) : Decidable (a ≤ b) := inferInstanceAs (Decidable (a.rank ≤ b.rank))

/-- Junção (supremo): o dado derivado é tão secreto quanto o mais secreto dos operandos. -/
def join : Conf → Conf → Conf
  | Secret, _ => Secret
  | _, Secret => Secret
  | Internal, _ => Internal
  | _, Internal => Internal
  | Public, Public => Public

infixl:65 " ⊔ " => join

theorem le_refl (a : Conf) : a ≤ a := Nat.le_refl _
theorem le_trans {a b c : Conf} (h₁ : a ≤ b) (h₂ : b ≤ c) : a ≤ c := Nat.le_trans h₁ h₂

theorem le_join_left (a b : Conf) : a ≤ a ⊔ b := by cases a <;> cases b <;> decide
theorem le_join_right (a b : Conf) : b ≤ a ⊔ b := by cases a <;> cases b <;> decide

/-- `⊔` é o supremo: a menor cota superior. -/
theorem join_le {a b c : Conf} (h₁ : a ≤ c) (h₂ : b ≤ c) : a ⊔ b ≤ c := by
  revert h₁ h₂; cases a <;> cases b <;> cases c <;> decide

/-- Um canal que aceita `allow` recebe `c` se e só se `c ⊑ allow`. -/
abbrev accepts (allow c : Conf) : Prop := c ≤ allow

/-- **Público é o fundo.** Dado público entra em qualquer canal. -/
theorem public_flows_anywhere (allow : Conf) : accepts allow Public := by
  cases allow <;> decide

/-- **Secreto só entra onde secreto é aceito.** Um canal público (`allow = Public`) recusa
`Secret` e `Internal`. -/
theorem public_channel_refuses_non_public {c : Conf} (h : accepts Public c) : c = Public := by
  cases c
  · rfl
  · exact absurd h (by decide)
  · exact absurd h (by decide)

end Conf

/-!
## Um avaliador abstrato e a não-interferência

Uma expressão sobre duas entradas rotuladas (`lo : Public`, `hi : Secret`): constantes, as
entradas, e a derivação binária (que junta os rótulos — modela `++`, `join`, construtor, e o
par tipo/rótulo de qualquer operação pura). O rótulo de uma expressão é `label`; o seu valor,
`eval`. `Pc` é a confidencialidade do contexto de controle.
-/

namespace Conf

inductive Expr where
  | lo                        -- uma entrada pública
  | hi                        -- uma entrada secreta
  | const (v : Nat)
  | derive (a b : Expr)       -- qualquer derivação binária pura
  deriving Repr

namespace Expr

/-- O rótulo estático, como o `ath-check` o calcula (junção na derivação). -/
def label : Expr → Conf
  | lo => Public
  | hi => Secret
  | const _ => Public
  | derive a b => a.label ⊔ b.label

/-- O valor, sob entradas concretas. -/
def eval (vlo vhi : Nat) : Expr → Nat
  | lo => vlo
  | hi => vhi
  | const v => v
  | derive a b => eval vlo vhi a + eval vlo vhi b

/-- **O rótulo limita a dependência.** Se o rótulo de `e` é `⊑ allow` e o canal `allow` não
aceita o secreto (`¬ accepts allow Secret`), então `e` não depende da entrada secreta: variá-la
não muda o valor. -/
theorem label_bounds_dependence (e : Expr) (vlo vhi vhi' : Nat)
    (hlab : ¬ Secret ≤ e.label) : e.eval vlo vhi = e.eval vlo vhi' := by
  induction e with
  | lo => rfl
  | hi => exact absurd (le_refl Secret) hlab
  | const v => rfl
  | derive a b iha ihb =>
      have ha : ¬ Secret ≤ a.label := fun h => hlab (le_trans h (le_join_left a.label b.label))
      have hb : ¬ Secret ≤ b.label := fun h => hlab (le_trans h (le_join_right a.label b.label))
      simp only [eval, iha ha, ihb hb]

end Expr

/-!
## O sink público, com `pc`

Um efeito num sink que aceita `allow` ocorre sob um contexto `pc`. O `athc` exige
`e.label ⊔ pc ≤ allow` (o argumento e o ramo que decidiu a chamada). `no_toxic_flow` conclui a
não-interferência: o que o sink recebe, e se o efeito ocorre, não dependem da entrada secreta.
-/

/-- A verificação do `athc` no sink (`E0440` é a sua negação). -/
abbrev sink_ok (allow pc : Conf) (e : Expr) : Prop := (e.label ⊔ pc) ≤ allow

/-- **Não-interferência do fluxo tóxico.** Se uma chamada a um sink público passa na verificação
(`sink_ok Public pc e`), então o argumento que o sink recebe não depende do segredo (o valor é o
mesmo variando a entrada secreta) e o contexto de controle é público (o efeito não é decidido por
dado secreto). Nenhum dado secreto alcança o canal público — qualquer que seja o segredo. -/
theorem no_toxic_flow (pc : Conf) (e : Expr) (vlo vhi vhi' : Nat)
    (h : sink_ok Public pc e) :
    e.eval vlo vhi = e.eval vlo vhi' ∧ pc = Public := by
  have hlab_le : e.label ≤ Public := le_trans (le_join_left e.label pc) h
  have hpc_le : pc ≤ Public := le_trans (le_join_right e.label pc) h
  refine ⟨?_, Conf.public_channel_refuses_non_public hpc_le⟩
  have : ¬ Secret ≤ e.label := by
    intro hs
    have : Secret ≤ Public := le_trans hs hlab_le
    exact absurd this (by decide)
  exact Expr.label_bounds_dependence e vlo vhi vhi' this

/-- **E a recíproca que torna o controle necessário.** Se o sink público recebe algo que depende
do segredo, a verificação falha: há um `e` (a própria entrada secreta) cujo valor varia e cujo
`sink_ok Public Public` é falso. É o programa que o `athc` recusa com `E0440`. -/
theorem a_real_leak_is_rejected :
    (∃ vhi vhi', Expr.hi.eval 0 vhi ≠ Expr.hi.eval 0 vhi') ∧ ¬ sink_ok Public Public Expr.hi :=
  ⟨⟨0, 1, by decide⟩, by decide⟩

end Conf
end Aletheia
