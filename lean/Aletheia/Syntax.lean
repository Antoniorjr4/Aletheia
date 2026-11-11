import Aletheia.Lattice
import Aletheia.Budget

/-!
# Sintaxe do núcleo λ_Ath

O subconjunto mecanizado corresponde ao que o `athc` verifica e o ARX executa no Marco Zero.
Ele tem dados de primeira ordem (inteiros, booleanos, unidade e listas com teto
`List[T, 0..n]`), capacidades lineares, o reticulado de integridade, orçamentos graduados,
inferência por um oráculo não confiável, endosso por política auditada e ações efetivas.
Variáveis usam índices de de Bruijn: o índice 0 é a ligação mais recente.

Uma capacidade não é um dado. Ela só existe no ambiente inicial (as capacidades do agente) e
tem o tipo `Cap[E]`, indexado pelo conjunto de efeitos `E` que autoriza. Não tem tipo de
valor: não pode ser copiada, guardada numa lista nem passada como argumento. Ela é linear
(multiplicidade 1): todo caminho de execução a consome exatamente uma vez, como a autoridade
de uma `act` ou por um `drop` explícito.
-/

namespace Aletheia

/-- Tipos de dados. `list t n` é `List[t, 0..n]`. -/
inductive Ty where
  | int
  | bool
  | unit
  | list (t : Ty) (n : Nat)
  deriving DecidableEq, Repr

/-- Valores de execução. `cap id` é a capacidade de identidade `id`. `pend t k` é a obrigação
linear `Pending[k]` da operação `t` de chave `k`: só existe no ambiente do tratador de
`TimeoutUnknown` (`call`), e só `transfer` a consome. -/
inductive Val where
  | int (n : Int)
  | bool (b : Bool)
  | unit
  | list (vs : List Val)
  | cap (id : Nat)
  | pend (t k : Nat)
  deriving Repr

/-- Tipagem de valores. Nenhum valor de dado é uma capacidade nem uma obrigação. -/
inductive HasTy : Val → Ty → Prop where
  | int  {n : Int} : HasTy (.int n) .int
  | bool {b : Bool} : HasTy (.bool b) .bool
  | unit : HasTy .unit .unit
  | list {vs : List Val} {t : Ty} {n : Nat} :
      (∀ v ∈ vs, HasTy v t) → vs.length ≤ n → HasTy (.list vs) (.list t n)

theorem HasTy.not_cap {v : Val} {t : Ty} (h : HasTy v t) (id : Nat) : v ≠ .cap id := by
  cases h <;> nofun

theorem HasTy.not_pend {v : Val} {t : Ty} (h : HasTy v t) (a k : Nat) : v ≠ .pend a k := by
  cases h <;> nofun

/-- Operações primitivas sobre inteiros. -/
inductive Op where
  | add | sub | le | eq
  deriving DecidableEq, Repr

def Op.ty : Op → Ty
  | .add | .sub => .int
  | .le | .eq => .bool

def Op.apply : Op → Int → Int → Val
  | .add, x, y => .int (x + y)
  | .sub, x, y => .int (x - y)
  | .le,  x, y => .bool (decide (x ≤ y))
  | .eq,  x, y => .bool (decide (x = y))

theorem Op.apply_ty (op : Op) (x y : Int) : HasTy (op.apply x y) op.ty := by
  cases op <;> constructor

/-- Expressões do núcleo. -/
inductive Exp where
  /-- literal escrito pelo autor do programa (Trusted) -/
  | val (v : Val)
  | var (k : Nat)
  /-- `let x = e₁ in e₂`; `e₂` vê `x` no índice 0 -/
  | let_ (e₁ e₂ : Exp)
  | ite (c t f : Exp)
  | prim (op : Op) (a b : Exp)
  /-- `refine e by p else d`: endosso de `e` pela política auditada `p` -/
  | endorse (p : Nat) (e d : Exp)
  /-- executa a ferramenta `t` com a autoridade da capacidade no índice `k`, consumindo-a -/
  | act (t : Nat) (k : Nat) (a : Exp)
  /-- descarta explicitamente a capacidade do índice `k`, sem efeito -/
  | drop (k : Nat)
  /-- `for x in xs { body }` sobre uma lista de no máximo `n` elementos -/
  | for_ (n : Nat) (xs body : Exp)
  /-- `infer m -> t { a }`: a saída do modelo é Untrusted -/
  | infer (m : Nat) (t : Ty) (a : Exp)
  /-- `await q`: a pós-condição observada pela sonda confiável `q`; se ela não confirma, a
  saga corrente falha (`TimeoutUnknown` do lado do mundo, `timeout` do lado do avaliador) -/
  | await (q : Nat)
  /-- `atomic recover r { body } on failure { f } on compensation_failed { g }`: a saga.
  Se o corpo falha, as compensações do que ele executou rodam em ordem reversa; `f` roda se
  todas deram certo e a sonda de recuperação `r` confirma, e `g` (`CompensationFailed`) em
  qualquer outro caso -/
  | atomic (body : Exp) (r : Nat) (f g : Exp)
  /-- `call t k a n e`: a ferramenta `t` com chave `own IdemKey`, com a autoridade da
  capacidade do índice `k`, que ela consome. A chave da operação é a identidade dessa
  capacidade. A chamada pode ser aplicada, recusada (`Failed`) ou ficar sem resposta
  (`TimeoutUnknown`). No último caso roda o tratador `reconcile p fuel n else e`: até `n`
  consultas pelo protocolo do provedor; se nenhuma for conclusiva, `e` roda num ambiente
  fechado que só contém a obrigação linear `Pending[k]` (índice 0), precisa transferi-la, e o
  seu valor encerra o evento (`Abort`). -/
  | call (t k : Nat) (a : Exp) (n : Nat) (e : Exp)
  /-- `transfer p`: entrega a obrigação do índice `p` ao reconciliador durável -/
  | transfer (p : Nat)
  deriving Repr

/-- Classe de efeito externo (identificador canônico atribuído pelo compilador, por exemplo
`Cluster.Write`). -/
abbrev EffectId := Nat

/-- Uma entrada do contexto de tipagem Γ: um dado com tipo e integridade, ou uma capacidade
linear `Cap[E]`. -/
inductive Bind where
  | data (t : Ty) (ι : Lab)
  | cap (E : List EffectId)
  /-- a obrigação linear `Pending[k]` no ambiente do tratador -/
  | pend
  deriving DecidableEq, Repr

/-- O que o provedor responde a uma chamada com chave (tentativa 0) ou a uma tentativa da
reconciliação (1, 2, …): aplicada, não aplicada, ou sem resposta. -/
inductive Reply where
  | done | failed | unknown
  deriving DecidableEq, Repr

/-- A classe do provedor de uma ferramenta com chave (spec 3.5): `idempotent` reenvia com a
mesma chave, `queryable` consulta o status pela chave, `opaque` não tem protocolo. -/
inductive Prov where
  | idempotent | queryable | opaque
  deriving DecidableEq, Repr

/-- Os estados terminais de uma chave `IdemKey` (spec 3.5): `Done` (a operação aconteceu),
`Failed` (não aconteceu) ou `Transferred` (a obrigação `Pending[k]` foi entregue ao
reconciliador durável). -/
inductive KEnd where
  | done | failed | transferred
  deriving DecidableEq, Repr

/-- A causa de uma falha que uma saga captura: a pós-condição `q` não observada (`R0910`),
a operação de chave `k` não aplicada (`R0600` na chamada, `R0611` na reconciliação), ou o
tratador de `TimeoutUnknown` que transferiu a obrigação e encerrou o evento com `v`. -/
inductive Cause where
  | probe (q : Nat)
  | refused (k : Nat)
  | abort (v : Val)
  deriving Repr

def Cause.isAbort : Cause → Bool
  | .abort _ => true
  | _ => false

/-- Eventos observáveis de uma execução. -/
inductive Ev where
  /-- a ferramenta `t` executou com a capacidade `id` e o argumento `v` -/
  | act (t : Nat) (id : Nat) (v : Val)
  /-- decisão de endosso pela política `p`: `some v` aceito, `none` recusado -/
  | endorse (p : Nat) (o : Option Val)
  /-- a saga corrente falhou pela causa `c` -/
  | fail (c : Cause)
  /-- a compensação de `t(v)` rodou; `ok = false` se o provedor a recusou -/
  | undo (t : Nat) (v : Val) (ok : Bool)
  /-- a sonda de recuperação `r` observou (`true`) ou não (`false`) o invariante -/
  | recover (r : Nat) (ok : Bool)
  /-- `TimeoutUnknown`: a chamada `t(v)` com a chave `k` não respondeu; nasce `Pending[k]` -/
  | pending (t : Nat) (k : Nat) (v : Val)
  /-- a `i`-ésima tentativa da reconciliação de `k` e a resposta; `none` para o provedor
  `opaque`, que não tem protocolo -/
  | query (k : Nat) (i : Nat) (r : Option Reply)
  /-- a chave `k` chegou ao estado terminal `e` -/
  | settle (k : Nat) (e : KEnd)
  /-- `transfer`: o tratador entregou a obrigação `k` ao reconciliador durável. O fechamento
  `Transferred` é emitido pela própria chamada, depois de conferir que o tratador entregou
  aquela obrigação exatamente uma vez -/
  | handoff (k : Nat)
  deriving Repr

/-- O estado abstrato do mundo: células indexadas (réplicas por deployment, saldos,
registros). As ferramentas, as compensações e as sondas são funções arbitrárias sobre ele,
declaradas na assinatura: os teoremas valem para qualquer uma delas. -/
abbrev World := Nat → Int

/-- A assinatura estática do programa: o contrato Δ das ferramentas e dos modelos, mais as
políticas auditadas. -/
structure Sig where
  /-- custo declarado de cada ferramenta -/
  cost  : Nat → Budget
  /-- custo máximo declarado de cada chamada de modelo -/
  mcost : Nat → Budget
  /-- tipo do argumento de cada ferramenta -/
  arg   : Nat → Ty
  /-- classe de efeito de cada ferramenta (a linha de efeitos da sua assinatura) -/
  effect : Nat → EffectId
  /-- políticas auditadas (decidíveis) -/
  pol   : Nat → Val → Bool
  /-- a transição que a ferramenta `t` com argumento `v` causa no mundo -/
  step  : Nat → Val → World → World := fun _ _ w => w
  /-- a ferramenta é `compensable` (entra no registro de compensação de uma saga). As
  reversíveis e as leituras não entram, como no ARX -/
  compensable : Nat → Bool := fun _ => true
  /-- a compensação de `t(v)` (`compensable by …`): o mundo depois dela, ou `none` se o
  provedor a recusou -/
  undo  : Nat → Val → World → Option World := fun _ _ w => some w
  /-- sondas confiáveis do mundo (`await`, `recover`) -/
  probe : Nat → World → Bool := fun _ _ => true
  /-- a classe do provedor de cada ferramenta com chave -/
  prov  : Nat → Prov := fun _ => .queryable
  /-- o provedor: `reply k i` é a resposta à tentativa `i` da operação de chave `k` (0 é a
  chamada; 1, 2, … o reenvio ou a consulta de status). Arbitrária: os teoremas valem para
  qualquer provedor, inclusive um que nunca responde -/
  reply : Nat → Nat → Reply := fun _ _ => .done
  /-- a verdade oculta de uma operação que ficou sem resposta: `applied k` diz se o efeito
  de `k` existe no mundo, saiba o agente ou não. É a razão de `Transferred` não significar
  "desfeito" -/
  applied : Nat → Bool := fun _ => false
  /-- o tipo do valor com que o tratador de `TimeoutUnknown` encerra o evento (o tipo de
  retorno do agente, como em `handle timeout_unknown(p) -> T`) -/
  ret   : Ty := .unit

/-- O oráculo de inferência: `O i m t v` é a resposta da `i`-ésima chamada ao modelo `m`,
pedida no tipo `t`, com o prompt `v`. Ele é arbitrário e, portanto, adversarial. -/
def Oracle := Nat → Nat → Ty → Val → Val

/-- O ARX valida cada resposta contra o esquema do tipo pedido (decodificação restrita e
análise). Do lado formal, isso é a hipótese de que o oráculo devolve valores bem tipados. -/
def Oracle.Ok (O : Oracle) : Prop := ∀ i m t v, HasTy (O i m t v) t

/-- Configuração de uma execução: o oráculo, o teto de orçamento do agente e a tabela de
concessões do runtime. `grants id` lista os efeitos que a capacidade de identidade `id`
realmente concede. O host instala a tabela; nenhuma resposta de modelo a escreve. -/
structure Cfg where
  oracle : Oracle
  limit  : Budget
  grants : Nat → List EffectId

end Aletheia
