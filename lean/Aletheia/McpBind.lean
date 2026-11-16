import Aletheia.Lattice

/-!
# v1.0-RC9: a ligação a um servidor MCP

O modelo da decisão que o ARX toma ao ligar uma ferramenta do programa a uma ferramenta de um
servidor MCP (`arx::bind::conform`, `arx::bind::call_mcp`), e das três regras que a tornam
segura diante de um servidor que pode mentir:

1. **o que o servidor diz de si só restringe**: as anotações (`readOnlyHint`, `destructiveHint`)
   e o rótulo (`_meta.ifc`) podem recusar uma ligação ou um valor, nunca dar confiança;
2. **a conferência vem antes de qualquer efeito**: uma ligação recusada não executa nada;
3. **a resposta do servidor nunca prova ausência**: numa operação com chave, nenhuma resposta
   da chamada (erro, `isError`, `input_required`, silêncio, queda) faz a linguagem concluir "não
   aplicada"; só a consulta por marcador decide.

| `arx` | aqui |
|---|---|
| a declaração da ferramenta (`reversible` ou não; o rótulo do resultado) | `Decl` |
| o que o servidor anuncia (esquema compatível? anotações; descrição) | `Advert` |
| `conform` | `admit` |
| `call_mcp` sobre a resposta | `classify` |
| o rótulo `_meta.ifc` contra o rótulo declarado | `enter` |
-/

namespace Aletheia.McpBind

open Aletheia

/-- O que o programa declara da ferramenta. -/
structure Decl where
  /-- uma leitura (`reversible`), ou a consulta de `.find` -/
  reads : Bool
  /-- o rótulo do resultado -/
  lab : Lab

/-- Uma anotação: ausente, ou o valor que o servidor afirma. -/
abbrev Hint := Option Bool

/-- O que o servidor anuncia da ferramenta. -/
structure Advert where
  /-- os esquemas conferem com a declaração (argumentos, obrigatórios, `enum`, caminho de saída) -/
  schemaOk : Bool
  readOnly : Hint
  destructive : Hint
  idempotent : Hint
  /-- a descrição (e as `instructions` do servidor): texto qualquer -/
  description : String

/-- O servidor **admite** que a ferramenta muda o mundo: só uma afirmação explícita conta. -/
def admitsDanger (a : Advert) : Bool := a.readOnly == some false || a.destructive == some true

/-- `conform`: a ligação é aceita se os esquemas conferem e o servidor não admite que muda o
mundo uma ferramenta que o programa usa como leitura. -/
def admit (d : Decl) (a : Advert) : Bool := a.schemaOk && !(d.reads && admitsDanger a)

/-- **A descrição não decide nada.** -/
theorem description_is_inert (d : Decl) (a : Advert) (s : String) :
    admit d { a with description := s } = admit d a := rfl

/-- **Uma promessa de segurança não muda nada.** Trocar o que o servidor promete (leitura,
idempotência, nada destrutivo) pela ausência de anotação dá a mesma decisão. -/
theorem promises_are_inert (d : Decl) (a : Advert) :
    admit d { a with readOnly := (if a.readOnly = some true then none else a.readOnly),
                     destructive := (if a.destructive = some false then none else a.destructive),
                     idempotent := none } = admit d a := by
  unfold admit admitsDanger
  cases h1 : a.readOnly with
  | none => cases h2 : a.destructive with
    | none => simp [h1, h2]
    | some b => cases b <;> simp [h1, h2]
  | some r => cases r <;> cases h2 : a.destructive with
    | none => simp [h1, h2]
    | some b => cases b <;> simp [h1, h2]

/-- A ordem de "admite mais perigo": `h ⊑ h'` se `h'` é `h` ou uma afirmação de perigo. -/
def moreDanger (a a' : Advert) : Prop :=
  a'.schemaOk = a.schemaOk ∧ (admitsDanger a = true → admitsDanger a' = true)

/-- **As anotações só restringem.** Se o servidor passa a admitir mais perigo, o que era
recusado continua recusado: nenhuma anotação transforma uma recusa em aceitação. -/
theorem hints_only_restrict {d : Decl} {a a' : Advert} (h : moreDanger a a') :
    admit d a' = true → admit d a = true := by
  obtain ⟨hs, hd⟩ := h
  unfold admit
  intro h'
  simp only [Bool.and_eq_true, Bool.not_eq_true'] at h' ⊢
  refine ⟨hs ▸ h'.1, ?_⟩
  cases hr : d.reads <;> cases ha : admitsDanger a <;> simp_all

/-- **Uma ferramenta que muda o mundo é aceita como o programa a declara.** Para uma operação
que o programa já trata como irrevogável (não `reads`), as anotações não importam: a classe do
efeito é a declarada. -/
theorem writes_ignore_hints (d : Decl) (a : Advert) (h : d.reads = false) :
    admit d a = a.schemaOk := by
  simp [admit, h]

/-- Um efeito, no modelo: só interessa se aconteceu. -/
abbrev Effect := Nat

/-- A execução: com a ligação aceita, os efeitos do plano; recusada, nada. -/
def run (ds : List (Decl × Advert)) (plan : List Effect) : List Effect :=
  if ds.all (fun p => admit p.1 p.2) then plan else []

/-- **A conferência vem antes de qualquer efeito.** Se uma ferramenta ligada é recusada, a
execução não tem efeito algum. -/
theorem refused_means_nothing {ds : List (Decl × Advert)} {p : Decl × Advert} (hm : p ∈ ds)
    (hr : admit p.1 p.2 = false) (plan : List Effect) : run ds plan = [] := by
  unfold run
  have : ds.all (fun p => admit p.1 p.2) = false := by
    rw [Bool.eq_false_iff]
    intro h
    have := List.all_eq_true.1 h p hm
    simp_all
  simp [this]

/-! ## O rótulo do servidor -/

/-- `_meta.ifc`: o valor entra com o rótulo **declarado**, ou não entra. Não entra quando o
servidor diz não confiável e o programa declara confiável (`R0649`). -/
def enter (declared : Lab) (server : Option Lab) : Option Lab :=
  if server = some Lab.U ∧ Lab.low declared then none else some declared

/-- **A confiança do servidor nunca eleva.** Com qualquer rótulo do servidor, o valor que entra
tem o rótulo declarado. -/
theorem server_label_never_raises (d : Lab) (s : Option Lab) (l : Lab) (h : enter d s = some l) :
    l = d := by
  unfold enter at h
  split at h <;> simp_all

/-- **Declarado não confiável, o rótulo do servidor não importa.** -/
theorem untrusted_declared_always_enters (s : Option Lab) : enter Lab.U s = some Lab.U := by
  simp [enter, Lab.not_low_U]

/-- **Só a contradição recusa.** -/
theorem refused_only_on_contradiction (d : Lab) (s : Option Lab) (h : enter d s = none) :
    s = some Lab.U ∧ Lab.low d := by
  unfold enter at h
  split at h <;> simp_all

/-! ## A resposta do servidor nunca prova ausência -/

/-- O que o servidor responde a um `tools/call`. -/
inductive Reply where
  | ok
  | isError
  | inputRequired
  | rpcError
  | violation
  | timeout
  | died
  /-- o pedido não chegou a ser escrito -/
  | notSent
  deriving DecidableEq, Repr

/-- O que a linguagem vê. -/
inductive Outcome where
  | value
  | failed
  | unknown
  | unreachable
  deriving DecidableEq, Repr

/-- `call_mcp`: numa operação com chave, toda resposta que não é um valor é desconhecida; numa
leitura, uma falha. O que não saiu é `unreachable`. -/
def classify (keyed : Bool) : Reply → Outcome
  | .ok => .value
  | .notSent => .unreachable
  | .timeout | .died => .unknown
  | _ => if keyed then .unknown else .failed

/-- **Nenhuma resposta da chamada conclui "não aplicada" numa operação com chave.** Nem
`isError` (o servidor do GitHub cria a issue e responde erro se os rótulos falham), nem
`input_required`, nem um erro do protocolo: o efeito pode ter acontecido, e quem decide é a
consulta por marcador. `failed` (o que a linguagem trata como "o provedor recusou, o efeito não
aconteceu") nunca é a classificação de uma operação com chave. -/
theorem server_reply_never_proves_absence (r : Reply) : classify true r ≠ .failed := by
  cases r <;> decide

/-- E `unreachable` só quando o pedido não saiu. -/
theorem unreachable_only_if_not_sent (k : Bool) (r : Reply) (h : classify k r = .unreachable) :
    r = .notSent := by
  cases r <;> cases k <;> simp_all [classify]

end Aletheia.McpBind
