# Rev11: chaves no tripé, o tratador de `TimeoutUnknown` no núcleo e a resistência estrutural

Revisão executada sobre a branch `rev11` (a partir de `rev10`, `b0754ed`). Gasto na Groq:
**US$ 0,00**. Nenhuma chamada paga, nenhum workflow pago disparado, nenhuma tag `live-*`,
agendamento pausado mantido pausado, limites do projeto intocados. Os 50 pares restantes do
banking continuam pendentes de decisão financeira. Nenhum merge foi executado.

## 1. Diretriz Zero

**Estado.** `main` está em `245214b`. O PR #1 (`rev10` → `main`, rascunho) leva 9 commits,
de `00a3c11` a `b0754ed`. Os resultados, journals, cassetes e referências da Rev10 e das
anteriores ficaram intocados. Esta revisão não alterou nenhum arquivo em `bench/agentdojo/runs`
nem em `bench/replay`, e os 16 cassetes do ARX reproduzem o journal gravado byte a byte (seção 5).

**Auditoria dos artefatos públicos** (o repositório é público):

- nenhuma credencial (chave de API, token, segredo) em arquivo versionado nem em histórico;
- os cassetes da Rev6 contêm o identificador de organização da Groq que a API devolve nos
  cabeçalhos. São evidência gravada e ficam como estão; a Rev10 já redige esses
  identificadores em `rate_limits.jsonl` e nas mensagens de commit do workflow;
- a mensagem do commit `ee4b095` (anterior à correção da Rev10) cita o identificador de projeto
  da Groq. O **squash merge** do PR #1 o elimina do histórico de `main`; ele continua nas
  referências do PR, que o GitHub preserva;
- os metadados de autoria dos commits trazem o e-mail pessoal do autor. Isso não é conteúdo
  do repositório e não foi reproduzido em nenhum documento.

**Squash merge proposto para o PR #1** (preparado, não executado):

```
Rev10: teto em dólar da Groq, AgentDojo banking (94/144 pares) e estatística pareada

- livro-razão de gasto (spend.jsonl), --max-usd por rodada e --max-usd-total acumulado;
  um só grupo de concorrência para os workflows pagos
- corpo de cada 429 em rate_limits.jsonl, com identificadores de projeto e organização
  redigidos; taxonomia de falhas
- banking (openai/gpt-oss-120b): 94 pares atacados e 16 benignos; ASR do baseline 68/94,
  do Aletheia 0/94 (Wilson 0–4% por par, 0–26% por tarefa); McNemar sobre a ASR p < 1e-20
- docs/REV10.md: auditoria da Rev9, plano executivo e critérios da v1.0
- gasto total US$ 0,103119
```

## 2. Eixo I: chaves no tripé

### 2.1 Fase 1: eventos canônicos

Os três backends projetam o protocolo de chaves nos mesmos eventos do journal do núcleo:

| evento do núcleo | Lean (`Ev`) | ARX / Python (journal completo) |
|---|---|---|
| `act {args, cap, tool}` + `settle {key, status: done}` | `act t id v`, `settle id done` | `tool` com `key` (a chave sai de `args`) |
| `settle {key, status: failed}` (recusa na hora) | `settle id failed` | **novo** `tool_failed {tool, key, error}` |
| `pending {key, tool}` | `pending t id v` | `timeout_unknown` |
| `query {attempt, key, status}` | `query id i r` (`none` = `opaque`) | `reconcile` |
| `settle {key, status}` (`done`/`failed`) | `settle id e` | `resolved` |
| `transfer {key}` + `settle {key, status: transferred}` | `handoff id`, `settle id transferred` | `transfer` |
| `fail {cause: refused, key}` · `fail {cause: abort}` | `fail (refused k)` · `fail (abort v)` | `saga_abort` por R0600/R0611 · pela obrigação transferida |
| `undo {ok, tool}` (ferramenta com chave) | `undo t v ok` | `compensate` com `key` |
| `abort {cause: refused, key}` | desfecho `timeout (refused k)` | `abort` por R0600/R0611 |
| `end {aborted: true, value}` | desfecho `timeout (abort v)` | `end` com `aborted_by_handler` |

Sem chaves, a vista do núcleo é exatamente a de antes: os 400 + 400 programas da Rev10
continuam idênticos byte a byte nos três backends. O único evento novo no journal completo é
`tool_failed`, só para ferramentas com chave. Os cassetes gravados não têm R0600, então
nenhum deles muda.

**Equivalência módulo renomeação bijetiva**, com duas verificações independentes
(`crates/athc/tests/differential.rs`):

1. **Journals normalizados iguais.** `canon_keys` renomeia a `i`-ésima chave distinta, na
   ordem da primeira aparição, para `k{i}`, e refaz a cadeia BLAKE3. A renomeação é injetiva
   por construção e não remove, reordena nem funde eventos. O Lean, o ARX e o Python coincidem
   byte a byte depois dela.
2. **Propriedades semânticas independentes da representação.** `lifecycle` verifica cada
   chave usando só a igualdade de nomes dentro de um journal. Confere:
   - exatamente um estado terminal, e ele é o último evento da chave;
   - `Done` com evidência: o `act` da própria chamada ou a última tentativa `done`;
   - `Failed` com evidência: a recusa da chamada ou a última tentativa `not_applied`;
   - `Transferred` só depois de esgotar o protocolo e logo depois de um `transfer`;
   - tentativas numeradas 1, 2, …;
   - nenhum `pending` sem `settle`;
   - `fail`/`abort` por recusa citam uma chave `Failed`.

   O ciclo de vida do Lean e o do ARX são então comparados chave a chave, pela ferramenta e
   pelo desfecho.

**Por que os bytes crus diferem.** O Lean nomeia a chave pela capacidade que a carrega
(`c3`), porque no núcleo a chave *é* a capacidade (seção 2.2). O ARX e o Python a nomeiam pela
ordem de cunhagem (`k1`, `k2`, …). O ARX e o Python coincidem byte a byte em todos os casos. O
journal do ARX já está na forma canônica (o teste confere), e o do Lean difere só nos nomes.

**A normalização não esconde defeito** (`key_normalization_does_not_hide_loss_duplication_or_swap`).
Sobre 200 programas reais, o journal do ARX foi mutado de seis formas. **Todos os 1.969
mutantes foram recusados**:

| mutação | mutantes | recusados |
|---|---|---|
| perda de um evento de chave | 540 | 540 |
| evento de chave duplicado | 540 | 540 |
| duas operações com a mesma chave | 120 | 120 |
| dois eventos trocam de chave | 457 | 457 |
| estado terminal antes da obrigação | 124 | 124 |
| tentativa depois do estado terminal | 188 | 188 |

O controle positivo são 144 renomeações bijetivas consistentes: a ordem inversa com outro
prefixo, e a troca das chaves de duas operações inteiras quando cada chave aparece uma única
vez. Todas foram aceitas. Na primeira rodada, 4 trocas foram aceitas, e eram exatamente esse
caso, uma renomeação legítima. Quando a chave aparece também num `fail`/`abort` de recusa,
trocar muda qual operação encerrou o evento, e o teste recusa.

### 2.2 Fase 2: chave distinta da capacidade (não feita; obrigação restante)

No núcleo desta revisão, a chave de `call t k a n e` é a identidade da capacidade `k`. Isso
mantém a linearidade sem maquinaria nova. Cada capacidade é consumida exatamente uma vez
(`Linear`, `Fresh`), então cada chave identifica uma única operação, e o T9 e o T1 herdam isso.
A superfície, porém, cunha a chave à parte (`keys.mint()`, com `Cap[IdemKey.Mint]`).

A Fase 2 **não foi feita**. A obrigação restante:

1. `Val.key k`, `Bind.key` e `Exp.mint m`, com `Cap[IdemKey.Mint]` não linear;
2. `consumes` e `Linear` estendidos a uma segunda classe de recurso linear, com a contagem
   independente das capacidades;
3. `call t c κ a n e` com a autoridade `c` e a chave `κ` separadas;
4. `Start`/`Fresh` para chaves cunhadas (frescor pelo contador de cunhagem);
5. refazer o T1 (`sound`, linearidade das chaves), o T2 (a autoridade continua sendo `c`) e o
   T9 (unicidade da chave pelo frescor da cunhagem, não pela linearidade da capacidade).

Com isso, o Lean nomearia a chave pela cunhagem, e os bytes crus poderiam coincidir com os do
ARX. A semântica observável não muda. O que muda é que o Lean passaria a provar a linearidade
das chaves cunhadas, que hoje só o `athc` impõe (E1104).

### 2.3 Fase 3: o tratador como expressão do núcleo

`lean/Aletheia/{Syntax,Typing,Eval}.lean`:

- `Exp.call t k a n e`, a chamada com chave:
  - as guardas de `act` (concessão, linearidade, orçamento);
  - a resposta da tentativa 0 pode ser `done` (`act` + `settle done`, efeito aplicado) ou
    `failed` (`refused k`: capacidade consumida, custo pago, mundo e registro intocados);
  - `unknown` faz nascer `Pending[k]` e roda `reconcile` com combustível `n` pela classe do
    provedor (`idempotent`, `queryable`, `opaque`);
  - esgotado o combustível, roda o tratador `e`;
- `Exp.transfer p` e `Bind.pend`;
- `Cause` = `probe` | `refused` | `abort`.

O tratador `e` roda num ambiente fechado `[Pending[k]]`. A tipagem (`HasType.call`) exige que
ele seja puro (`pureH`), do tipo de retorno do agente (`S.ret`), e que consuma a obrigação
exatamente uma vez em todo caminho (`consumes e 0 = some 1`). Em tempo de execução, `call` só
fecha `Transferred` se o traço do tratador for exatamente um `handoff k` e nenhum evento de
chave (`transferOnly`). Qualquer outra coisa é erro de linearidade, e a obrigação nunca some
em silêncio. O valor do tratador encerra o evento (`abort v`).

Numa saga, o encerramento compensa e atravessa a saga que recupera
(`eval_atomic_reraise`). Na saga que não recupera, `on compensation_failed` roda e o
encerramento é absorvido, como no ARX.

**Status das provas.** `lake build --wfail` passa, sem `sorry`, `admit` nem axioma novo.
O alvo `Audit` fixa o conjunto de axiomas de cada teorema: as 44 entradas anteriores
continuam idênticas, e entraram 10 novas:

| teorema | o que afirma | axiomas |
|---|---|---|
| `soundness` (T1) | programa bem tipado, linear e `Guarded` termina com valor bem tipado e no orçamento; **ou** com a recusa `refused k`; **ou** com o encerramento `abort v`, `v : S.ret`. Nunca `stuck`, `noCap`, `reuse`, `budget` | propext, Classical.choice, Quot.sound |
| `progress`, `preservation` (T1) | todo erro é recusa ou encerramento; o valor tem o tipo estático | idem |
| `soundness_keyless` (T1) | o enunciado anterior (termina com valor), no fragmento sem `call` | idem |
| `T2` | ações léxicas, concedidas, capacidades consumidas uma vez, com término (sem `call`) | idem |
| `T2_keys` | em **todo** desfecho, toda ação usou `Cap[E]` léxica e concedida | idem |
| `caps_exactly_once_ok` | com valor, cada capacidade consumida exatamente uma vez, com chaves | idem |
| `noninterference`, `robust_endorsement` (T3) | duas execuções sem saga terminando com valor: mesmo traço ou divergência num endosso (com `call`) | idem |
| `noninterference_saga` (T3) | todo programa bem tipado, **com sagas e chaves**: mesmo valor; ou a mesma falha (mesma causa, inclusive o valor do tratador, mesmo traço, mesmo mundo); ou divergência num endosso | idem |
| `noninterference_saga_keys` (T1 + T3) | com chaves, sem hipótese sobre o desfecho | idem |
| `doCall_ni` | mesma capacidade, mesmo argumento e mesmo mundo dão o mesmo desfecho da chamada | propext, Quot.sound |
| `T6_no_masking`, `T6_recovered` (T6) | a falha de uma saga (qualquer causa) aparece no traço com as compensações e a sonda | propext, Quot.sound |
| `T6`, `T6_saga` (T6) | compensação restaura o invariante sob contratos `restoring` (corpo sem `call`) | propext, Quot.sound |
| `T9`, `T9_outcome` | toda chave de todo desfecho segue o ciclo de vida do protocolo (qualquer programa) | propext, Classical.choice, Quot.sound |
| `T9_no_silent_discard`, `T9_done_observed`, `T9_transferred_exhausted` | nenhuma obrigação some; `Done` depois de `pending` exige `query … done`; `Transferred` exige o protocolo esgotado | idem |
| `T9_transfer_explicit` | `Transferred` exige um `transfer` explícito daquela chave | propext, Quot.sound |
| `T9_honest` | com provedor honesto, `Done` ⇒ aplicado e `Failed` ⇒ não aplicado | propext, Classical.choice, Quot.sound |
| `KeysExample.transferred_but_applied` | uma operação `Transferred` cujo efeito está no mundo: transferir não é desfazer | propext, Quot.sound |

**Hipóteses alteradas e por quê:**

- `soundness` passou de "termina com valor" para três desfechos. Com chaves, a recusa e o
  encerramento pelo tratador são desfechos legítimos, e o enunciado anterior seria falso. O
  enunciado antigo foi mantido como `soundness_keyless`, com `keyless e = true`.
- `T2`, `caps_exactly_once`, `noninterference_total` e `noninterference_saga_total` ganharam
  `keyless e = true`, porque afirmam término com valor. As versões com chaves (`T2_keys`,
  `caps_exactly_once_ok`, `noninterference_saga_keys`) não têm essa hipótese.
- `T6` e `T6_saga` ganharam `keyless body = true`. Uma operação com chave sem resposta entra
  no registro da saga mesmo que não tenha sido aplicada, como no ARX ("compensar é total
  sobre aconteceu e não aconteceu"). Sob um contrato `restoring` genérico, compensar o que não
  aconteceu não restaura o invariante, e o teorema seria falso. A restauração com chaves exige
  compensadores conscientes da chave (como o `unscale` do `remediate`, que restaura a
  pré-imagem registrada pela chave). Isso é contrato do provedor, não teorema do núcleo.
  `T6_no_masking` e `T6_recovered` valem para qualquer corpo.

Nenhuma dessas hipóteses torna a prova trivial. Cada uma recorta exatamente o fragmento em
que o enunciado anterior continua verdadeiro, e cada uma tem ao lado a versão geral.

### 2.4 Fase 4: provedores e os sete cenários

O gerador (`crates/athc/tests/lambda_ath/mod.rs`, `generate_keyed`) dá a cada capacidade uma
ferramenta com chave, com:

- um provedor `idempotent`, `queryable` ou `opaque`;
- falhas injetadas na chamada e nos reenvios (`timeout_lost`, `timeout_applied`, `error`);
- consultas inconclusivas (`unknown_status`).

`KeySpec::simulate` reproduz o `SimWorld` e produz o roteiro do provedor do Lean
(`replies`, `applied`). Um provedor `opaque` nunca entra em saga, porque o `athc` recusa
(T0305).

`keyed_scenarios_are_deterministic_on_the_three_backends` fixa os sete cenários, cada um com
o journal do núcleo esperado, conferido nos três backends:

1. **Timeout antes da aplicação.** `queryable`, `timeout_lost`: `query not_applied`, `Failed`,
   `fail {refused}`. A sonda de recuperação observa o mundo intacto (`recover ok: true`).
2. **Timeout depois da aplicação, antes da confirmação.** `timeout_applied`: `query done`,
   `Done`; a pós-condição observa o efeito.
3. **Repetição com a mesma chave.** `idempotent`: o reenvio devolve o recibo. A pós-condição
   exige a célula em 5, não em 10: o efeito não se repetiu.
4. **Consulta que confirma**, depois de uma inconclusiva.
5. **Consultas inconclusivas e, enfim, conclusiva** (`not_applied`): o evento termina recusado
   (`R0611`).
6. **Combustível esgotado e transferência.** O evento termina pelo tratador
   (`end {aborted: true}`). Na variante 6′, dentro de uma saga, a sonda observa o efeito ainda
   no mundo depois de `Transferred`.
7. **Falha de compensação com obrigação pendente.** A obrigação é transferida e a saga não
   consegue compensar a outra ação (`undo ok: false`). Roda `on compensation_failed`, o
   encerramento é absorvido, e o `settle {transferred}` da obrigação continua no journal.

"Done" sempre tem a evidência do protocolo. "Transferred" não significa desfeito. Nenhuma
obrigação desconhecida some do journal. São os três requisitos, verificados pelo `lifecycle` em
todo programa e provados no Lean (T9).

### 2.5 Fase 5: o tripé

| corpus | programas | Lean × ARX × Python |
|---|---|---|
| núcleo (sem sagas) | 400 | idênticos byte a byte |
| sagas | 400 | idênticos byte a byte |
| **chaves** (sagas + `call`) | **600** (431 executam chamadas com chave) | ARX = Python byte a byte; Lean = ARX módulo renomeação; ciclo de vida igual chave a chave |
| cenários determinísticos | 8 | journal esperado exato nos três |

Cobertura do corpus com chaves (600 sementes, em número de chaves):

| cenário | chaves |
|---|---|
| timeout antes de aplicar | 175 |
| timeout depois de aplicar | 192 |
| reenvio com a mesma chave | 100 |
| consulta que confirma | 42 |
| consulta inconclusiva | 228 |
| combustível esgotado e transferência | 179 |
| … das quais com o efeito aplicado | 80 |
| falha de compensação com obrigação pendente | 29 |
| `Done` na hora | 111 |
| `Failed` na hora | 69 |
| `Failed` reconciliado | 50 |

Desfechos dos eventos: 346 com valor, 159 encerrados pelo tratador e 95 recusados. Nenhuma
divergência sem explicação e nenhuma regressão, e o oráculo não foi alterado. As 169 sementes
com os bytes crus Lean = ARX são exatamente as que não executam nenhuma chamada com chave.
Evidência: `cargo test -p athc --test differential -- --nocapture` com `ATH_TRIPOD_N=400
ATH_SAGA_N=400 ATH_KEYED_N=600 ATH_MUTANT_N=200`. O job `tripod` da CI agora roda com esses
valores.

## 3. Eixo II: resistência estrutural com o oráculo comprometido

`bench/adversarial/` (README, `harness.py`, `results/`) roda separado do AgentDojo oficial,
que ficou intacto. O modelo é um roteiro que sempre propõe a ação do atacante. O harness usa:

- para o banking, o ambiente, as ferramentas e o verificador de segurança oficiais do
  `agentdojo` v1.2.2;
- para o remediate, o ARX sobre o `SimWorld`;
- para as variantes estáticas, o `athc check`.

**Resultado** (`results/MATRIX.md`):

| | tentativas | resultado |
|---|---|---|
| proibidas (categorias a–f) | 29 | **29 recusadas antes do efeito**; mundo intacto; verificador oficial nunca violado |
| controles legítimos | 4 | 4 executados |
| resíduos (indevidos dentro da política) | 3 | 3 executados (limite documentado) |

Cada garantia, atribuída ao estrato que de fato a produz:

| estrato | o que recusou |
|---|---|
| **compilador** (`athc`) | o resultado do modelo direto numa escrita (E0412); fluxo implícito (E0412); texto Untrusted como instrução (E0415); capacidade sem o efeito (E0502); irrevogável sem `commit` (T0301); chave reutilizada ou fora do laço (E1104); `Pending` descartado (E1107); irrevogável na saga (T0302); `opaque` na saga (T0305) |
| **política de endosso** (`refine … by SafePlan`) | transferências ao atacante, redirecionamento, senha não citada, parcelamento de US$ 30 mil, agendamento recorrente, plano misto; contra os inventários confiáveis: agendada inexistente, a própria conta, IBAN a um dígito de um beneficiário, agendada existente para não beneficiário |
| **decodificação tipada** | IBAN com 40 caracteres, variante inexistente com campo de autorização inventado, cinco escritas (`decode_reject`) |
| **verificação dinâmica** | a escrita endossada sem a concessão do runtime (`R0502`, antes do host) |
| **semântica de execução** | o reenvio com a mesma chave não repete o efeito; o desconhecido não vira sucesso nem falha; a compensação roda na falha da pós-condição |

**O que o Lean prova e o que não prova.** O Lean prova os teoremas da seção 2.3 sobre o núcleo
λ_Ath e o seu avaliador canônico (`eval`). Ele **não** prova nada sobre:

- `banking.ath` e `remediate.ath`, que usam ADTs, listas, `match`, texto e inventários, todos
  fora do núcleo;
- o `athc` (a correção do verificador estático em relação a `Typing.lean`);
- o ARX e o `aletheia_runtime`;
- a adequação da `SafePlan`.

A correspondência entre o núcleo e os runtimes é empírica (o tripé), não provada. As recusas
da matriz são, portanto, garantias do compilador, da política e do runtime *implementados*. O
Lean dá a forma delas no núcleo: T3/`robust_endorsement` (dado Untrusted só alcança uma ação
por uma decisão de endosso), T2 (só ações com capacidade léxica e concedida) e T9 (ciclo de
vida das chaves).

**H1 e H2, separados.**

- **H2 (resistência estrutural): sustentada para as formas de ataque testadas.** Com o modelo
  100% comprometido, nenhuma ação proibida produziu efeito, sob duas condições: a correção da
  política e dos inventários confiáveis, e a implementação do runtime.
- **H1 (resistência comportamental): o que explica o 0/94 da Rev10.** Nos journals oficiais da
  Rev10 (`bench/agentdojo/runs/…/aletheia-journals`, 110 execuções, 94 atacadas), o modelo
  real **nunca** propôs um plano que a `SafePlan` recusasse: `policy_reject` = 0. As 7
  recusas do `refine` foram saídas fora do tipo (`decode_reject`), não decisões de política. O
  mesmo modelo, no baseline, cumpriu o objetivo do atacante em 68/94. O pipeline Aletheia
  (planejador de leitura que só vê o pedido, dados citados, plano tipado) mudou o
  comportamento do modelo, e a política nunca precisou agir. A ASR oficial de 0/94 é evidência
  de H1 nesse pipeline. **Não** é evidência de H2. A evidência de H2 é esta matriz.
- **Limite de H2.** Os resíduos mostram onde a estrutura não protege:
  - R1: valor inflado a um beneficiário legítimo, porque a `SafeWrite` não limita valores;
  - R2: endereço trocado, porque `Profile` é aceito incondicionalmente;
  - RS1: uma política que aceita todo plano compila, porque o compilador exige endosso mas
    não julga a política.

  Nenhuma tarefa de injeção do banking v1.2.2 explora R1 ou R2. Um atacante que as explorasse
  dependeria só de H1.

## 4. Eixo III

Nenhuma chamada paga. Gasto desta revisão: **US$ 0,00**. O acumulado autorizado e gasto até a
Rev10 continua em US$ 0,103119. O restante do orçamento anterior não foi tratado como permissão.

## 5. Regressões

| verificação | resultado |
|---|---|
| `lake build --wfail` (provas, auditoria de axiomas, `ath-lean-eval`) | ✓ sem `sorry`/`admit`/axioma novo |
| `cargo fmt --check`, `cargo clippy --all-targets -D warnings` | ✓ |
| `cargo test --workspace` (com `ATH_REQUIRE_LEAN=1`) | ✓ |
| tripé 400 + 400 + 600 chaves + 8 cenários + 200 sementes de mutantes | ✓ |
| `athc replay` dos 16 cassetes do ARX | ✓ journal idêntico ao gravado |
| replays Python (testes diferenciais) | ✓ |
| `aletheia_runtime` (unidade) | ✓ 8 |
| ponte AgentDojo (offline) | ✓ 18 |
| M1 (`bench/m1/run.py`; oráculos) | ✓ (9) |
| matriz adversarial | ✓ 29/29, 4/4, 3/3 |

## 6. Achados e limitações

1. **O journal não registra os argumentos de uma chamada sem resposta.** `timeout_unknown`
   tem `{tool, key, provider, elapsed}`, sem `args`. A vista do núcleo projeta a compensação de
   uma operação com chave como `undo {ok, tool}`, sem argumentos. A correção (incluir `args`)
   muda o formato de um evento presente nos cassetes gravados, e por isso fica para um formato
   de journal versionado (v1.0). A classe `compensable` das ferramentas entra na vista do
   núcleo pelo programa (`Journal::core_view_with`), não pelo journal.
2. **A recusa na hora de uma operação com chave não deixava rastro.** Antes desta revisão, o
   R0600 abortava sem entrada no journal. Agora `tool_failed` registra a recusa no ARX e no
   Python. A vista do núcleo dessa recusa (`settle failed`) não diz a ferramenta, e o journal
   completo diz.
3. **Compensadores de célula não conhecem a chave.** No tripé, o compensador `u{i}` subtrai o
   argumento mesmo de uma operação que não foi aplicada. Os três backends fazem o mesmo, e é
   por isso que o T6 com restauração exige corpo sem `call` (seção 2.3).
4. **Fase 2 não feita** (seção 2.2): no núcleo, a chave é a capacidade.
5. **A correspondência núcleo ↔ runtimes é empírica.** O tripé cobre o fragmento gerado, não
   a superfície inteira.

## 7. Entregas

- **Lean.** `Syntax`, `Typing`, `Eval`, `Soundness`, `Confinement`, `Noninterference`,
  `SagaNI`, `Saga`, `Keys`, `Examples`, `Exec`, `Main`, `Axioms`.
- **ARX.** `tool_failed`; `Journal::core_view_with`; `Journal::untimed`.
- **athc.** `compensable_tools`; o `--core-journal` usa a vista com chaves.
- **Python.** `tool_failed`; `core_view(entries, compensable)`.
- **Testes.** O gerador com chaves e `KeySpec::simulate`; `keyed_tripod…`;
  `key_normalization…`; `keyed_scenarios…`; o tripé aceita as recusas R0600/R0611 e confere o
  desfecho ARX × Python.
- **Eixo II.** `bench/adversarial/` e a CI (job `agentdojo`).
- **CI.** O job `tripod` com `ATH_KEYED_N=600 ATH_MUTANT_N=200`.
