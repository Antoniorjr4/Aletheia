/-!
# v1.0-RC5: a fronteira nativa

O modelo de `arx::bind` e do monitor de tipo de `Arx::monitor`.

| `arx` | aqui |
|---|---|
| o tipo declarado do resultado de uma ferramenta (`Int`, `Bool`, `Text[..n]`) | `Ty` |
| a resposta crua do provedor | `Raw` |
| `ath_grammar::check` sobre a resposta crua | `check` |
| o monitor: o valor entra no programa, ou a chamada falha com `R0640` | `monitor` |
| `secret("NOME")`: uma referência; o valor só existe no cabeçalho da requisição | `Atom.sec` |
| `BoundHost::scrub`: o segredo refletido vira `‹secret:NOME›` | `scrub` |

## Hipóteses

* **B1 simbólico**: o provedor pode **copiar** o segredo que recebeu para a resposta, quantas
  vezes quiser e onde quiser, mas não transformá-lo (um provedor que devolve o segredo
  codificado ou cortado não é detectado: nem aqui, nem no executável).
* **B2 canal**: o segredo chega ao provedor pelo cabeçalho; o canal (TLS, ou o próprio host)
  não é modelado.
* **B3 sem leitura**: a linguagem não tem expressão que leia um segredo (`secret(…)` só é
  sintaxe dentro de `bind`), então os argumentos que o programa envia não dependem dele. Isso
  vale por construção da gramática e não é um teorema daqui.

Não modelado: listas e tipos algébricos no resultado (o executável confere listas de
escalares; tipos algébricos não atravessam a fronteira de `bind`).
-/

namespace Aletheia.Boundary

/-- O tipo declarado do resultado. -/
inductive Ty where
  | int (lo hi : Int)
  | bool
  | text (max : Nat)

/-- O que o provedor devolve: qualquer coisa. -/
inductive Raw where
  | num (n : Int)
  | bool (b : Bool)
  | str (len : Nat) (id : Nat)
  | other (id : Nat)

/-- O valor habita o tipo. -/
inductive HasTy : Ty → Raw → Prop where
  | int {lo hi n : Int} : lo ≤ n → n ≤ hi → HasTy (.int lo hi) (.num n)
  | bool {b : Bool} : HasTy .bool (.bool b)
  | text {max len id : Nat} : len ≤ max → HasTy (.text max) (.str len id)

/-- A conferência executável. -/
def check : Ty → Raw → Bool
  | .int lo hi, .num n => decide (lo ≤ n) && decide (n ≤ hi)
  | .bool, .bool _ => true
  | .text max, .str len _ => decide (len ≤ max)
  | _, _ => false

theorem check_sound {τ : Ty} {v : Raw} (h : check τ v = true) : HasTy τ v := by
  cases τ <;> cases v <;> simp [check] at h
  · exact .int h.1 h.2
  · exact .bool
  · exact .text h

theorem check_complete {τ : Ty} {v : Raw} (h : HasTy τ v) : check τ v = true := by
  cases h <;> simp [check, *]

/-- O que a chamada devolve ao programa: um valor, ou a culpa do provedor (`R0640`). -/
inductive Outcome where
  | value (v : Raw)
  | blame

/-- O monitor da fronteira. -/
def monitor (τ : Ty) (v : Raw) : Outcome :=
  if check τ v then .value v else .blame

/-- **Solidez da fronteira.** Qualquer que seja o provedor (`f` é arbitrário: é o adversário),
o que entra no programa por uma ferramenta de tipo `τ` habita `τ`. -/
theorem boundary_sound (τ : Ty) (f : Nat → Raw) (req : Nat) (v : Raw)
    (h : monitor τ (f req) = .value v) : HasTy τ v := by
  unfold monitor at h
  by_cases c : check τ (f req) = true
  · simp only [c, if_true, Outcome.value.injEq] at h
    exact h ▸ check_sound c
  · simp [c] at h

/-- **Sem culpa indevida.** Um provedor que devolve um valor do tipo nunca é culpado, e o
valor passa sem alteração: o monitor é transparente para quem cumpre o contrato. -/
theorem boundary_transparent {τ : Ty} {v : Raw} (h : HasTy τ v) : monitor τ v = .value v := by
  simp [monitor, check_complete h]

/-- **A culpa é do provedor.** A chamada só falha no monitor se o valor está fora do tipo. -/
theorem blame_is_earned {τ : Ty} {v : Raw} (h : monitor τ v = .blame) : ¬ HasTy τ v := by
  intro t
  rw [boundary_transparent t] at h
  cases h

/-! ## Segredos por referência -/

/-- Um átomo de uma resposta: um dado público, ou o segredo de nome `name` (copiado). -/
inductive Atom where
  | pub (n : Nat)
  | sec (name : Nat)
  /-- a marca `‹secret:NOME›`: cita o nome, não o valor -/
  | mark (name : Nat)

/-- O ambiente do executor: o valor de cada segredo. -/
abbrev Env := Nat → Nat

/-- O valor concreto de um átomo, num ambiente. A marca e o dado público não dependem dele. -/
def inst (σ : Env) : Atom → Nat × Nat
  | .pub n => (0, n)
  | .sec name => (1, σ name)
  | .mark name => (2, name)

/-- A redação: toda cópia do segredo vira a marca com o nome. -/
def scrub : Atom → Atom
  | .sec name => .mark name
  | a => a

/-- O que o programa (e o journal, o WAL, a fita, o prompt) vê de uma resposta. -/
def observed (σ : Env) (resp : List Atom) : List (Nat × Nat) :=
  (resp.map scrub).map (inst σ)

theorem inst_scrub (σ₁ σ₂ : Env) (a : Atom) : inst σ₁ (scrub a) = inst σ₂ (scrub a) := by
  cases a <;> rfl

/-- **Não interferência do segredo.** Qualquer que seja o provedor (`p` devolve o que quiser,
inclusive cópias do segredo), o que o programa observa é o mesmo para quaisquer dois valores
do segredo: nada do que o programa vê, grava ou envia a um modelo depende do valor. -/
theorem secret_noninterference (σ₁ σ₂ : Env) (p : Nat → List Atom) (req : Nat) :
    observed σ₁ (p req) = observed σ₂ (p req) := by
  unfold observed
  induction p req with
  | nil => rfl
  | cons a t ih => simp only [List.map_cons, ih, inst_scrub σ₁ σ₂ a]

/-- **Sem a redação, o segredo vaza.** O contraexemplo que a redação fecha: um provedor que
copia o segredo para a resposta faz o que o programa vê depender do valor. -/
theorem without_scrub_it_leaks :
    ∃ (σ₁ σ₂ : Env) (resp : List Atom), resp.map (inst σ₁) ≠ resp.map (inst σ₂) :=
  ⟨fun _ => 0, fun _ => 1, [.sec 0], by simp [inst]⟩

end Aletheia.Boundary
