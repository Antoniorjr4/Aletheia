# Aletheia em Lean 4: o núcleo λ_Ath mecanizado

Esta pasta formaliza o núcleo λ_Ath, isto é, o subconjunto que o `athc` verifica e o ARX
executa, e demonstra três teoremas sobre **um único** julgamento de tipagem e **um único**
avaliador:

- **T1**: preservação e progresso, com orçamento graduado e consumo exato de capacidades.
- **T2**: efeitos e capacidades lineares `Cap[E]`. Toda ação usa uma capacidade léxica cujo
  `E` contém o efeito e foi concedida pelo runtime, e cada capacidade é consumida exatamente
  uma vez.
- **T3**: não-interferência com endosso robusto.

Não há `sorry`. Cada teorema principal tem o conjunto de axiomas fixado em
[`Aletheia/Axioms.lean`](Aletheia/Axioms.lean), que só aceita os três axiomas padrão do Lean
(`propext`, `Quot.sound`, `Classical.choice`). O CI roda `lake build --wfail`, que reprova
qualquer aviso, inclusive `declaration uses 'sorry'`.

```bash
cd lean
lake build --wfail          # toolchain fixado em lean-toolchain (Lean 4.20.0)
```

## O que está modelado

| arquivo | conteúdo | no Marco Zero |
|---|---|---|
| `Lattice.lean` | `Untrusted ⊑ Endorsed ⊑ Trusted`, encontro `⊓`, `low ℓ` (ℓ ⊒ Endorsed) | `ath_syntax::ast::Label`, E3 |
| `Budget.lean` | β sobre seis dimensões (tok, usd, wall, energy, bytes, risk), `+`, `⊔`, `n • β` | E6, contrato Δ |
| `Syntax.lean` | tipos `Int`/`Bool`/`Unit`/`List[T, 0..n]`, valores, `Cap[E]`, eventos, assinatura Σ (custos, tipos de argumento, efeito de cada ferramenta, políticas), oráculo, tabela de concessões do runtime | AST, Codex, linhas de efeito |
| `Typing.lean` | Γ; pc ⊢ e : τ ^ ι ▷ β, contagem exata `consumes`, `Linear` (multiplicidade 1), `Granted` | E1, E2, E3, E5 (políticas), E6 |
| `Eval.lean` | avaliador total de passo grande com quatro erros verificados em tempo de execução | ARX |
| `Soundness.lean` | T1 e o consumo exato | |
| `Noninterference.lean` | T3 | |
| `Confinement.lean` | T2, sobre o mesmo `eval` e o mesmo `HasType` | E2, E1 |
| `Recovery.lean` | primeira modelagem abstrata da recuperação de sagas | T6 / T9 parcial |
| `Examples.lean` | o padrão de remediação: tipado, executado, com as variantes perigosas recusadas | `examples/remediate.ath` |

As expressões são estas, com índices de de Bruijn:

```
e ::= v | x | let x = e in e | if e then e else e | e ⊕ e
    | refine e by p else e        -- endosso pela política auditada p
    | act t k e                   -- ferramenta t com a capacidade Cap[E] no índice k
    | drop k                      -- descarte explícito da capacidade no índice k
    | for (n) x in e { e }        -- laço sobre List[T, 0..n]
    | infer m -> T { e }          -- saída do modelo: sempre Untrusted
```

**Capacidades `Cap[E]`.** Só existem no ambiente inicial: são as capacidades do agente.
Cada uma é indexada pelo conjunto de efeitos `E` que autoriza. Não têm tipo de valor, então
não podem ser copiadas por `let`, guardadas numa lista nem passadas como argumento. São
**lineares, com multiplicidade 1**: `consumes e k` conta quantas vezes **todo** caminho de
execução consome a capacidade `k`. As regras são estas:

- os dois ramos de um `if` precisam consumir o mesmo;
- o `else` de um endosso e o corpo de um laço não consomem nada de fora, porque podem não
  executar ou executar um número variável de vezes;
- o descarte só existe como `drop`, que é consumo explícito.

`Linear Γ e` exige `consumes e k = 1` para cada `Cap[E]` de Γ.

**Duas autoridades por ação.** `act t k a` só tipa se `Γ[k] = Cap[E]` e `effect t ∈ E`.
Em tempo de execução, o avaliador só emite o evento se a tabela de concessões
`c.grants id`, que o host instala e nenhuma resposta de modelo escreve, contiver o efeito.

**O avaliador não confia na tipagem.** Ele verifica tudo o que o ARX verifica e falha com um
erro explícito:

- `stuck`: operação sobre um valor de forma errada;
- `noCap`: efeito sem capacidade, ou sem concessão do runtime para o efeito da ferramenta;
- `reuse`: capacidade linear já consumida, por `act` ou por `drop`;
- `budget`: o efeito estouraria o teto. A verificação vem antes do efeito, como a reserva
  do ARX.

**O oráculo** `O : ℕ → ℕ → Ty → Val → Val` é arbitrário, logo adversarial. A única hipótese
sobre ele é que devolve valores do tipo pedido, que é o que a validação de esquema do ARX
garante (decodificação restrita e análise; recusa → `refine`).

## T1: preservação e progresso

```lean
theorem soundness (hO : c.oracle.Ok) (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    ∃ o, eval S c e ρ s = .ok o ∧ HasTy o.v τ ∧ Within s.spent o.st.spent β ∧
      ∃ new, o.st.used = new ++ s.used ∧ ∀ id, id ∈ new ↔ ∃ k, ρ[k]? = some (.cap id)

theorem progress     … : ∀ err, eval S c e ρ s ≠ .error err
theorem preservation … (he : eval S c e ρ s = .ok o) :
    HasTy o.v τ ∧ Within s.spent o.st.spent β ∧ o.st.spent ≤ c.limit
theorem eval_fresh   : eval S c e ρ s = .ok o → Fresh s.used o.st.used
```

`Start G Γ ρ s` diz quatro coisas:
- o ambiente realiza Γ;
- as capacidades têm identidades distintas;
- nenhuma delas foi consumida;
- a tabela de concessões cobre cada tipo `Cap[E]`.

`Within s s' β` diz que o gasto real fica dentro de β **em cada dimensão**; o laço gasta no
máximo `n • β(corpo)`, como na regra E6.

O conjunto `new` que a execução acrescenta é **exatamente** o conjunto das capacidades do
agente. `eval_fresh` diz que, por construção, esse acréscimo não repete identidade.

## T2: efeitos e capacidades lineares `Cap[E]`

```lean
-- qualquer programa: toda ação do traço foi concedida pelo runtime
theorem effects_granted (c : Cfg) (e : Exp) : eval S c e ρ s = .ok o → Granted' S c.grants o.tr

-- programa bem tipado: toda ação usou uma Cap[E] léxica cujo E contém o efeito
theorem effects_lexical (c : Cfg) (h : HasType S Γ pc e τ ι β) :
    eval S c e ρ s = .ok o → Lexical S Γ ρ o.tr

-- multiplicidade 1: cada capacidade do agente é consumida exatamente uma vez
theorem caps_exactly_once … : ∃ o new, eval S c e ρ s = .ok o ∧ o.st.used = new ++ s.used ∧
    new.Nodup ∧ ∀ id, id ∈ new ↔ ∃ k, ρ[k]? = some (.cap id)

theorem T2 (hO : c.oracle.Ok) (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    ∃ o new, eval S c e ρ s = .ok o ∧ Lexical S Γ ρ o.tr ∧ Granted' S c.grants o.tr ∧
      o.st.used = new ++ s.used ∧ new.Nodup ∧ ∀ id, id ∈ new ↔ ∃ k, ρ[k]? = some (.cap id)
```

O `T2` junta as quatro garantias e acrescenta a que dá sentido à checagem estática. Para um
programa bem tipado e linear, com concessões que cobrem os tipos, a guarda dinâmica **nunca
recusa**: a verificação estática basta. Uma capacidade nunca é duplicada (`new.Nodup`), e
nenhuma é descartada sem consumo explícito, porque todas as identidades do ambiente estão em
`new`.

## T3: não-interferência com endosso robusto

```lean
theorem noninterference (h : HasType S Γ pc e τ ι β) (hl : LowEq Γ ρ₁ ρ₂)
    (h₁ : eval S c₁ e ρ₁ s₁ = .ok o₁) (h₂ : eval S c₂ e ρ₂ s₂ = .ok o₂) :
    (o₁.tr = o₂.tr ∧ (low ι → o₁.v = o₂.v)) ∨ Diverge o₁.tr o₂.tr
```

`LowEq Γ ρ₁ ρ₂` deixa as duas execuções diferirem arbitrariamente em toda entrada
`Untrusted`. Os dois oráculos, os dois tetos e as duas tabelas de concessões (`c₁`, `c₂`)
também são arbitrários e independentes. `Diverge` diz que os traços coincidem até uma
**decisão de endosso** e divergem nela; não há terceira possibilidade. Consequências:

```lean
-- mesmas decisões de endosso ⇒ mesmas ações, mesmas capacidades, mesmos argumentos
theorem robust_endorsement … (hend : endorsements o₁.tr = endorsements o₂.tr) :
    o₁.tr = o₂.tr ∧ actions o₁.tr = actions o₂.tr ∧ (low ι → o₁.v = o₂.v)

-- todo valor aceito por `refine … by p` satisfaz p, em qualquer execução
theorem endorsed_sound : eval S c e ρ s = .ok o → PolicyOk S o.tr

-- sob pc Untrusted não há ação nem endosso
theorem confine (h : HasType S Γ pc e τ ι β) : ¬ low pc → eval S c e ρ s = .ok o → o.tr = []

-- com o T1, as duas execuções sempre terminam: o T3 vale sem hipótese sobre o resultado
theorem noninterference_total … : ∃ o₁ o₂, eval … = .ok o₁ ∧ eval … = .ok o₂ ∧ NI ι o₁ o₂
```

A robustez está na regra de tipagem do endosso: `refine` exige `pc ⊒ Endorsed`. A própria
decisão de endossar, portanto, não está sob controle de dado não confiável. Juntando tudo:
**entradas `Untrusted` não desviam o fluxo de execução para ações fora de políticas
comprovadas**. O único canal da entrada não confiável até uma ação é um endosso, e todo
endosso aceito satisfaz a sua política auditada.

## Exemplos verificados (`Examples.lean`)

- **A remediação** (`remediate_typed`, `remediate_linear`, `remediate_runs`,
  `remediate_T2`): o caminho é `infer → refine by SafeScale else 3 → scale` com
  `Cap[Cluster.Write]`. O programa é bem tipado, linear e cabe no orçamento. Para
  **qualquer** resposta do modelo, o T1 garante a execução, e o T2 garante que a ação usou a
  capacidade certa, concedida, exatamente uma vez.
- **Variantes que não tipam:**
  - `injected_rejected`: usar a resposta do modelo direto como argumento da ação;
  - `steered_rejected`: escolher a ação por um teste sobre a resposta do modelo;
  - `deleting_rejected`: uma ferramenta `Cluster.Delete` com `Cap[Cluster.Write]`.
- **O E1 com multiplicidade 1:**
  - recusa o consumo duplo (`twice_not_linear`);
  - recusa o consumo dentro de laço (`looped_not_linear`);
  - recusa o consumo num ramo só (`oneBranch_not_linear`);
  - recusa o descarte implícito (`discard_not_linear`);
  - aceita o mesmo programa de um ramo só com `drop` no outro (`bothBranches_linear`).
- **Concessão revogada:** o avaliador recusa a ação com `noCap`, por `rfl`.
- **A injeção que a Groq recebeu em `bench/replay/`,** em duas execuções concretas por `rfl`:
  - com o modelo respondendo 0, o traço é `[endorse none, act scale 3]`;
  - com o modelo respondendo 5, o traço é `[endorse (some 5), act scale 5]`.

  Os dois divergem exatamente na decisão de endosso.

## Fora deste núcleo, por ora

A prova é do cálculo, não do código Rust ou Python. A correspondência com o ARX vem da
construção e dos testes diferenciais, não de uma prova de refinamento. Por construção, o
ARX tem os mesmos erros, a reserva antes do efeito e a mesma contagem do E1 e do E6.

Uma diferença deliberada: o E1 do `athc` aplica multiplicidade 1 às obrigações lineares
(`IdemKey`, `Pending`), enquanto as capacidades de ferramenta do ARX (`Cap[Cluster.Write]`)
são reutilizáveis. No núcleo, toda capacidade é linear. Aproximar as duas disciplinas é parte
da prova de refinamento.

Ainda não estão mecanizados:

- registros, variantes e `match`;
- `Text` e refinamentos `{x:σ | φ}` além das políticas decidíveis;
- emissão de capacidades (`keys.mint()`) e atenuação;
- `retry`, sagas, compensação e reconciliação (T6, T9): `Recovery.lean` é só um modelo
  abstrato, sem ligação com o avaliador;
- graus de certeza `σ ~ p`;
- localizações ℓ;
- os teoremas T4 e T5.
