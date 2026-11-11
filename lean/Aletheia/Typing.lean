import Aletheia.Syntax

/-!
# Julgamento de tipagem

`HasType S Γ pc e τ ι β` lê-se Γ; pc ⊢ e : τ ^ ι ▷ β. O valor de `e` tem tipo `τ` e
integridade `ι`; em contexto de controle de integridade `pc`, `e` gasta no máximo `β`.

Estes são os estratos do `athc` que o núcleo cobre:

* **E3 (integridade)**: o resultado de uma operação recebe o encontro dos rótulos. Ramificar
  ou iterar sobre um dado rebaixa `pc`. `act` e `endorse` exigem `pc ⊒ Endorsed`, e `act`
  exige também o argumento `⊒ Endorsed`. A saída de `infer` é sempre `Untrusted`.
* **E6 (orçamento)**: os custos somam em sequência e fazem junção nos ramos, e o laço
  multiplica: β(for) = β(xs) + n • β(corpo).
* **E2 (efeitos e capacidades)**: `act t k a` exige que o índice `k` seja uma capacidade
  `Cap[E]` e que o efeito da ferramenta `t` esteja em `E`.
* **E1 (linearidade, multiplicidade 1)**: `consumes e k` é o número de vezes que **todo**
  caminho de execução de `e` consome a capacidade do índice `k`, ou `none` se caminhos
  diferentes consomem quantidades diferentes. `Linear Γ e` exige exatamente um consumo de
  cada `Cap[E]` em todo caminho: nem duplicação, nem descarte implícito. Descartar uma
  capacidade exige `drop`.
-/

namespace Aletheia

open Lab Budget

/-! ## Multiplicidade exata -/

/-- Soma de contagens; `none` (caminhos discordantes) absorve. -/
def Count.add : Option Nat → Option Nat → Option Nat
  | some a, some b => some (a + b)
  | _, _ => none

/-- Dois ramos alternativos precisam consumir a mesma quantidade. -/
def Count.same : Option Nat → Option Nat → Option Nat
  | some a, some b => if a = b then some a else none
  | _, _ => none

/-- Uma parte que pode não executar (o `else` de um endosso, o corpo de um laço) não pode
consumir nada que venha de fora. -/
def Count.zero : Option Nat → Option Nat
  | some 0 => some 0
  | _ => none

/-- Quantas vezes todo caminho de execução de `e` consome a capacidade do índice `k`. -/
def consumes : Exp → Nat → Option Nat
  | .val _, _ => some 0
  | .var _, _ => some 0
  | .let_ e₁ e₂, k => Count.add (consumes e₁ k) (consumes e₂ (k + 1))
  | .ite c t f, k => Count.add (consumes c k) (Count.same (consumes t k) (consumes f k))
  | .prim _ a b, k => Count.add (consumes a k) (consumes b k)
  | .endorse _ e d, k => Count.add (consumes e k) (Count.zero (consumes d k))
  | .act _ j a, k => Count.add (consumes a k) (some (if j = k then 1 else 0))
  | .drop j, k => some (if j = k then 1 else 0)
  | .for_ _ xs body, k => Count.add (consumes xs k) (Count.zero (consumes body (k + 1)))
  | .infer _ _ a, k => consumes a k
  | .await _, _ => some 0
  -- a saga consome o que o corpo consome e o que os tratadores consomem (os dois iguais);
  -- o que um caminho não usar é revogado na saída (`Eval.lean`), então todo caminho
  -- consome exatamente essa contagem
  | .atomic body _ f g, k => Count.add (consumes body k) (Count.same (consumes f k) (consumes g k))
  -- o tratador roda num ambiente fechado (só a obrigação): não consome nada de fora
  | .call _ j a _ _, k => Count.add (consumes a k) (some (if j = k then 1 else 0))
  | .transfer j, k => some (if j = k then 1 else 0)


/-- O tratador de `TimeoutUnknown` no núcleo: computação pura sobre dados e `transfer`. Sem
efeito, sem modelo, sem saga: o que ele decide é com que valor encerrar o evento, e a
obrigação que ele recebe precisa sair por `transfer`. -/
def pureH : Exp → Bool
  | .val _ | .var _ | .transfer _ => true
  | .let_ a b | .prim _ a b => pureH a && pureH b
  | .ite x t f => pureH x && pureH t && pureH f
  | _ => false

inductive HasType (S : Sig) : List Bind → Lab → Exp → Ty → Lab → Budget → Prop where
  | val {Γ pc v τ} :
      HasTy v τ →
      HasType S Γ pc (.val v) τ .T 0
  | var {Γ pc k τ ι} :
      Γ[k]? = some (.data τ ι) →
      HasType S Γ pc (.var k) τ ι 0
  | let_ {Γ pc e₁ e₂ τ₁ τ₂ ι₁ ι₂ β₁ β₂} :
      HasType S Γ pc e₁ τ₁ ι₁ β₁ →
      HasType S (.data τ₁ ι₁ :: Γ) pc e₂ τ₂ ι₂ β₂ →
      HasType S Γ pc (.let_ e₁ e₂) τ₂ ι₂ (β₁ + β₂)
  | ite {Γ pc c t f τ ιc ιt ιf βc βt βf} :
      HasType S Γ pc c .bool ιc βc →
      HasType S Γ (pc ⊓ ιc) t τ ιt βt →
      HasType S Γ (pc ⊓ ιc) f τ ιf βf →
      HasType S Γ pc (.ite c t f) τ (ιc ⊓ (ιt ⊓ ιf)) (βc + (βt ⊔ βf))
  | prim {Γ pc op a b ιa ιb βa βb} :
      HasType S Γ pc a .int ιa βa →
      HasType S Γ pc b .int ιb βb →
      HasType S Γ pc (.prim op a b) op.ty (ιa ⊓ ιb) (βa + βb)
  | endorse {Γ pc p e d τ ιe ιd βe βd} :
      low pc →
      HasType S Γ pc e τ ιe βe →
      HasType S Γ pc d τ ιd βd →
      HasType S Γ pc (.endorse p e d) τ (.E ⊓ ιd) (βe + βd)
  | act {Γ pc t k a ιa βa E} :
      low pc →
      Γ[k]? = some (.cap E) →
      S.effect t ∈ E →
      HasType S Γ pc a (S.arg t) ιa βa →
      low ιa →
      HasType S Γ pc (.act t k a) .unit .T (βa + S.cost t)
  | drop {Γ pc k E} :
      Γ[k]? = some (.cap E) →
      HasType S Γ pc (.drop k) .unit .T 0
  | for_ {Γ pc n xs body τ τb ιx ιb βx βb} :
      HasType S Γ pc xs (.list τ n) ιx βx →
      HasType S (.data τ ιx :: Γ) (pc ⊓ ιx) body τb ιb βb →
      HasType S Γ pc (.for_ n xs body) .unit .T (βx + n • βb)
  | infer {Γ pc m t a τa ιa βa} :
      HasType S Γ pc a τa ιa βa →
      HasType S Γ pc (.infer m t a) t .U (βa + S.mcost m)
  /-- `await q` observa o mundo pela sonda confiável `q`. Como uma falha dispara
  compensações, que são efeitos, ela exige `pc ⊒ Endorsed`. -/
  | await {Γ pc q} :
      low pc →
      HasType S Γ pc (.await q) .unit .T 0
  /-- A saga: o corpo e os dois tratadores têm o mesmo tipo. A escolha do tratador depende
  só do mundo e das compensações (observações confiáveis do host). As compensações rodam
  como obrigação, sem orçamento: β = β(corpo) + (β(f) ⊔ β(g)). -/
  | atomic {Γ pc body r f g τ ιb ιf ιg βb βf βg} :
      low pc →
      HasType S Γ pc body τ ιb βb →
      HasType S Γ pc f τ ιf βf →
      HasType S Γ pc g τ ιg βg →
      HasType S Γ pc (.atomic body r f g) τ (ιb ⊓ (ιf ⊓ ιg)) (βb + (βf ⊔ βg))
  /-- A chamada com chave: as exigências de `act` (E2, E3) e um tratador puro, tipado no
  ambiente que só contém a obrigação, com o tipo de retorno do agente, e que consome a
  obrigação **exatamente uma vez em todo caminho** (E1, o `E1107` do `athc`). O orçamento da
  chamada é o da ação; a reconciliação é obrigação, fora do contrato Δ, como no ARX. -/
  | call {Γ pc t k a n e ιa βa E ιe βe} :
      low pc →
      Γ[k]? = some (.cap E) →
      S.effect t ∈ E →
      HasType S Γ pc a (S.arg t) ιa βa →
      low ιa →
      HasType S [.pend] .T e S.ret ιe βe →
      pureH e = true →
      consumes e 0 = some 1 →
      HasType S Γ pc (.call t k a n e) .unit .T (βa + S.cost t)
  /-- `transfer p` exige uma obrigação no índice `p` e, como toda decisão com efeito
observável, `pc ⊒ Endorsed`: dado `Untrusted` não decide entregar uma obrigação. -/
  | transfer {Γ pc p} :
      low pc →
      Γ[p]? = some .pend →
      HasType S Γ pc (.transfer p) .unit .T 0

/-- E1 com multiplicidade 1: cada `Cap[E]` de Γ é consumida exatamente uma vez em todo
caminho de execução. -/
def Linear (Γ : List Bind) (e : Exp) : Prop :=
  ∀ (k : Nat) (E : List EffectId), Γ[k]? = some (Bind.cap E) → consumes e k = some 1

theorem Count.add_eq {a b : Option Nat} {m : Nat} (h : Count.add a b = some m) :
    ∃ x y, a = some x ∧ b = some y ∧ m = x + y := by
  cases a <;> cases b <;> simp [Count.add] at h
  exact ⟨_, _, rfl, rfl, h.symm⟩

theorem Count.same_eq {a b : Option Nat} {m : Nat} (h : Count.same a b = some m) :
    a = some m ∧ b = some m := by
  cases a <;> cases b <;> simp [Count.same] at h
  obtain ⟨rfl, rfl⟩ := h
  exact ⟨rfl, rfl⟩

theorem Count.zero_eq {a : Option Nat} {m : Nat} (h : Count.zero a = some m) :
    a = some 0 ∧ m = 0 := by
  unfold Count.zero at h
  split at h
  · cases h; exact ⟨rfl, rfl⟩
  · cases h

/-- O ambiente realiza o contexto: dados bem tipados nas posições de dado e capacidades nas
posições de capacidade. -/
inductive EnvOk : List Bind → List Val → Prop where
  | nil : EnvOk [] []
  | data {Γ ρ v τ ι} : HasTy v τ → EnvOk Γ ρ → EnvOk (.data τ ι :: Γ) (v :: ρ)
  | cap {Γ ρ id E} : EnvOk Γ ρ → EnvOk (.cap E :: Γ) (.cap id :: ρ)
  | pend {Γ ρ t k} : EnvOk Γ ρ → EnvOk (.pend :: Γ) (.pend t k :: ρ)

theorem EnvOk.data_lookup {Γ : List Bind} {ρ : List Val} (h : EnvOk Γ ρ) :
    ∀ {k : Nat} {τ : Ty} {ι : Lab}, Γ[k]? = some (Bind.data τ ι) → ∃ v, ρ[k]? = some v ∧ HasTy v τ := by
  induction h with
  | nil => intro k τ ι hk; simp at hk
  | @data Γ ρ v τ' ι' hv _ ih =>
    intro k τ ι hk
    cases k with
    | zero => simp at hk; obtain ⟨rfl, rfl⟩ := hk; exact ⟨v, rfl, hv⟩
    | succ k => simpa using ih (by simpa using hk)
  | cap _ ih =>
    intro k τ ι hk
    cases k with
    | zero => simp at hk
    | succ k => simpa using ih (by simpa using hk)
  | pend _ ih =>
    intro k τ ι hk
    cases k with
    | zero => simp at hk
    | succ k => simpa using ih (by simpa using hk)

theorem EnvOk.cap_lookup {Γ : List Bind} {ρ : List Val} (h : EnvOk Γ ρ) :
    ∀ {k : Nat} {E : List EffectId}, Γ[k]? = some (Bind.cap E) → ∃ id, ρ[k]? = some (Val.cap id) := by
  induction h with
  | nil => intro k E hk; simp at hk
  | data _ _ ih =>
    intro k E hk
    cases k with
    | zero => simp at hk
    | succ k => simpa using ih (by simpa using hk)
  | @cap Γ ρ id _ _ ih =>
    intro k E hk
    cases k with
    | zero => exact ⟨id, rfl⟩
    | succ k => simpa using ih (by simpa using hk)
  | pend _ ih =>
    intro k E hk
    cases k with
    | zero => simp at hk
    | succ k => simpa using ih (by simpa using hk)

/-- Uma capacidade no ambiente está numa posição de capacidade de Γ. -/
theorem EnvOk.cap_pos {Γ : List Bind} {ρ : List Val} (h : EnvOk Γ ρ) :
    ∀ {k id : Nat}, ρ[k]? = some (Val.cap id) → ∃ E, Γ[k]? = some (Bind.cap E) := by
  induction h with
  | nil => intro k id hk; simp at hk
  | data hv _ ih =>
    intro k id hk
    cases k with
    | zero => simp at hk; exact absurd hk (hv.not_cap id)
    | succ k => simpa using ih (by simpa using hk)
  | @cap Γ ρ id' E _ ih =>
    intro k id hk
    cases k with
    | zero => exact ⟨E, rfl⟩
    | succ k => simpa using ih (by simpa using hk)
  | pend _ ih =>
    intro k id hk
    cases k with
    | zero => simp at hk
    | succ k => simpa using ih (by simpa using hk)

theorem EnvOk.length {Γ : List Bind} {ρ : List Val} (h : EnvOk Γ ρ) : ρ.length = Γ.length := by
  induction h <;> simp_all

/-- Uma posição `Pending` de Γ guarda uma obrigação. -/
theorem EnvOk.pend_lookup {Γ : List Bind} {ρ : List Val} (h : EnvOk Γ ρ) :
    ∀ {k : Nat}, Γ[k]? = some Bind.pend → ∃ t id, ρ[k]? = some (Val.pend t id) := by
  induction h with
  | nil => intro k hk; simp at hk
  | data _ _ ih =>
    intro k hk
    cases k with
    | zero => simp at hk
    | succ k => simpa using ih (by simpa using hk)
  | cap _ ih =>
    intro k hk
    cases k with
    | zero => simp at hk
    | succ k => simpa using ih (by simpa using hk)
  | @pend Γ ρ t id _ ih =>
    intro k hk
    cases k with
    | zero => exact ⟨t, id, rfl⟩
    | succ k => simpa using ih (by simpa using hk)

/-- As concessões do runtime cobrem o tipo estático: a capacidade guardada em cada posição
`Cap[E]` concede pelo menos os efeitos de `E`. -/
def Granted (G : Nat → List EffectId) (Γ : List Bind) (ρ : List Val) : Prop :=
  ∀ (k : Nat) (E : List EffectId) (id : Nat), Γ[k]? = some (Bind.cap E) →
    ρ[k]? = some (Val.cap id) → ∀ x ∈ E, x ∈ G id

theorem Granted.cons {G : Nat → List EffectId} {Γ : List Bind} {ρ : List Val} {τ : Ty}
    {ι : Lab} {v : Val} (h : Granted G Γ ρ) : Granted G (.data τ ι :: Γ) (v :: ρ) := by
  intro k E id hk hid
  cases k with
  | zero => simp at hk
  | succ k => exact h k E id (by simpa using hk) (by simpa using hid)

end Aletheia
