# Rev10 · auditoria da Rev9 e plano executivo

Base: Marco Zero v0.9, commit `245214b` (10/11/2026). Trabalho desta revisão na branch `rev10`.
Toda afirmação abaixo vem com a evidência que a sustenta. As medições feitas nesta revisão
estão separadas das herdadas de execuções anteriores.

## 1. Reconhecimento

| item | estado verificado em 10/11/2026, 22h UTC |
| --- | --- |
| commit de referência | `main` = `245214b`, igual ao da v0.9 |
| workflows | `ci.yml` (push na `main` e PR) e quatro que gastam na Groq: `agentdojo.yml`, `live.yml`, `m1-multisample.yml` e `replay.yml`. Nenhum tem `schedule:`; todos rodam por `workflow_dispatch`, e `live.yml` também por tag `live-*` |
| execuções em andamento ou na fila | nenhuma antes desta revisão |
| agendamento do 2º lote | **não estava no repositório.** Era uma tarefa agendada da conta, criada pela sessão anterior: o workflow completo, na `main`, às 08:45 UTC de 11/11, com `max_minutes=320`, todos os pares, 120B e sem teto em dólar. **Foi pausada com a autorização do usuário**, sem ser apagada |
| limites da chave `GROQ_API_KEY` | a resposta de `limits.py` no run [42915660955](https://github.com/Antoniorjr4/Aletheia/actions/runs/42915660955) traz `x-ratelimit-limit-tokens: 8000` (por minuto) e `x-ratelimit-limit-requests: 1000` (por dia), que são os limites do plano gratuito. **O plano pago não se reflete nesta chave.** Falta conferir no console da Groq a organização e o plano dela |
| preço de lista (GroqCloud, out./2026) | `gpt-oss-120b`: US$ 0,15 / 0,60 por milhão de tokens (entrada / saída); `gpt-oss-20b`: US$ 0,075 / 0,30 |

## 2. Evidências

### 2.1 Reproduzidas nesta revisão (máquina limpa, commit `245214b`)

| verificação | resultado |
| --- | --- |
| `cargo build --release` | ok |
| `cargo test` (inclui diferenciais e os replays de `bench/replay/`) | **99 testes, 0 falhas** (o README ainda diz 98) |
| `lake build --wfail` (Lean 4.20.0) | ok: 30 alvos, auditoria de axiomas (`Aletheia.Axioms`) e o binário `ath-lean-eval` |
| tripé com Lean obrigatório (`ATH_REQUIRE_LEAN=1`), corpus padrão (160 + 200) | ok |
| tripé com o corpus da CI (`ATH_TRIPOD_N=400 ATH_SAGA_N=400`) | ok: 400 programas sem sagas e 400 com sagas, os três journals do núcleo idênticos |
| `replay_of_real_groq_traffic_reproduces_the_recorded_journal` | ok: os 22 cassetes, ARX e Python |
| runtime Python (`runtimes/python/tests`) | 8 testes ok |
| oráculos da M1 (`bench/m1/tests`) | 9 testes ok |
| ponte AgentDojo offline (`agentdojo==0.1.35`) | 12 testes ok em `245214b`; 15 com os testes do teto em dólar (§4) |

Nota de reprodução: o `elan` baixa o Lean de `release.lean-lang.org`. Num ambiente que não
alcança esse host, o mesmo binário oficial vem das releases do GitHub
(`leanprover/lean4/releases/download/v4.20.0/lean-4.20.0-linux.tar.zst`), e a construção é
idêntica, porque o projeto não depende do Mathlib.

### 2.2 Herdadas (não reexecutadas nesta revisão)

- As rodadas da M1 contra a Groq (Rev6 e Rev7, 20B e 120B). Os oráculos as recalculam offline
  (§2.1), mas as chamadas ao modelo não foram refeitas.
- O primeiro lote do AgentDojo (run 42781873807, plano gratuito).
- O histórico de execuções da CI até `245214b`.

## 3. Achados da auditoria

| # | achado | severidade | estado |
| --- | --- | --- | --- |
| G1 | tarefa agendada fora do repositório disparando uma bateria paga sem teto em dólar nem autorização | crítica (financeira e de protocolo) | **pausada**; o workflow ganhou teto em dólar (§4) |
| G2 | a chave em `GROQ_API_KEY` opera com os limites do plano gratuito | alta (operacional): com 8K tokens/min e o teto diário, a `workspace` é inviável | aberto: conferir organização e plano no console da Groq |
| G3 | `live.yml`, `m1-multisample.yml` e `replay.yml` não têm teto em dólar, só em tempo e repetições | média | parcial: os quatro workflows pagos agora são serializados (`concurrency: groq-paid`); o teto em dólar deles fica para quando forem usados de novo |
| G4 | o README diz `cargo test # 98 testes`; são 99 | baixa | corrigir na documentação da v1.0 |
| G5 | o IC de Wilson do AgentDojo trata os pares como independentes, mas 9 injeções compartilham cada tarefa de usuário | média (estatística) | plano em §6.2 |
| G6 | o `elan` depende de um host fora do GitHub | baixa (reprodutibilidade) | documentado em §2.1 |

## 4. O que esta revisão já implementou (offline, verificado)

**Teto em dólar do AgentDojo** (`bench/agentdojo/run.py`, regras 7 a 9 da Rev10):

- Livro-razão `spend.jsonl`: uma linha por chamada ao modelo, com tokens, origem da contagem
  (`server` ou `estimate`) e custo. Ele sobrevive a uma rodada interrompida e é a base do teto
  acumulado.
- `--max-usd` (por rodada) e `--max-usd-total` (acumulado no logdir). Antes de cada
  requisição, a rodada recusa se o teto já foi alcançado, então o excesso máximo é o de uma
  requisição. Antes de cada tarefa, ela reserva 1,5 × o pior custo por execução já medido.
- Custo = tokens medidos pelo servidor × preço de lista. Descontos de cache não entram; o valor
  faturado é o do painel da Groq. Respostas sem `usage` entram como estimativa, separadas do
  medido.
- Uma tarefa interrompida pelo teto deixa um log parcial **sem veredito**, como a da cota no
  lote 1: a matriz não a conta e a retomada a refaz. Há teste para isso.
- Os quatro workflows pagos ficam num só grupo de concorrência.

O protocolo experimental não mudou. O `banking.ath` emitido é idêntico byte a byte ao do lote 1
(Codex `#8d1f2c3ede29`), e ataque, pacote (`agentdojo==0.1.35`, suíte v1.2.2), verificadores e
baseline são os mesmos. As mudanças do runtime Python entre o lote 1 e `245214b` (`core_view` e
células do `SimWorld`, do tripé com sagas) não estão no caminho da ponte.

## 5. Lacunas da Rev9 (Eixo III): classificação

| lacuna | severidade | bloqueia a v1.0? | caminho |
| --- | --- | --- | --- |
| **A.** correspondência λ_Ath ↔ ARX ↔ Python | alta para a alegação | **não**, se a v1.0 disser exatamente o que existe; **sim**, se anunciar a linguagem implementada como verificada | §6.3 |
| **B.** recuperação após queda | alta (operacional) | não para o núcleo; sim para qualquer alegação de execução durável | §6.4 |
| **C.** contratos `restoring` no compilador | média-alta | não, desde que o T6 seja apresentado como teorema do cálculo sob hipótese de contrato, e não como checagem do `athc` | §6.5 |
| **D.** atualização versionada do Δ | média | não | §6.6 |
| **E.** alcance da superfície | alta para a comunicação | **sim**: a documentação da v1.0 precisa da tabela garantia → teorema / teste / nada | §7 |

**Escopo exato da correspondência demonstrada hoje (A).** Para todo programa do fragmento
gerado F (núcleo linear, com e sem sagas, sem chaves e sem `drop` na superfície), o journal do
núcleo do ARX, o do Python e o do `ath-lean-eval` (o `eval` dos teoremas) são idênticos byte a
byte nos corpora de 400 + 400 sementes. Isso é **teste**, não prova: vale para os programas
executados e não diz nada fora de F.

## 6. Plano executivo

### 6.1 Matriz

| eixo · item | implementado | falta | depende de | risco formal | conclusão quando | pago? | custo estimado |
| --- | --- | --- | --- | --- | --- | --- | --- |
| I.1a vocabulário de chaves no journal do núcleo | `Ev.pending/query/settle` no Lean | eventos no `ath-lean-eval` (`Exec.lean`) e nas duas `core_view` | — | baixo | os três backends emitem `pending`, `query` e `settle` no mesmo formato | não | US$ 0 |
| I.1b cunhagem de chaves separada da capacidade | chave = identidade da capacidade | `mint` no núcleo, frescor das chaves | I.1a | médio: novo valor linear no T1 | `eval_fresh` estendido às chaves, sem `sorry` | não | US$ 0 |
| I.1c `handle timeout_unknown` como expressão do núcleo | o protocolo está dentro de `act` | `Pending` como vínculo linear; `reconcile … fuel … else` e `transfer` como expressões; guarda dinâmica na saída do tratador | I.1b | **alto**: T1, T6 e T9 precisam ser refeitos | T1, T2, T3, T6 e T9 sem `sorry`, com o mesmo conjunto de axiomas | não | US$ 0 |
| I.1d classes de provedor | `reply` arbitrário | `idempotent`, `queryable` e `opaque` na `Sig`; teorema de no máximo uma aplicação sob provedor honesto | I.1c | médio | teorema com hipóteses explícitas | não | US$ 0 |
| I.2 tripé com chaves | — | gerador com ferramentas com chave e roteiros de falha cobrindo os 7 cenários da Rev10; relatório de cobertura | I.1a–d | baixo | 400 programas com chaves, zero divergências não explicadas, cobertura por cenário documentada | não | US$ 0 |
| II.1 retomada controlada | **feito**: agendamento pausado, teto em dólar, congelamento verificado | — | — | — | — | — | — |
| II.2 Banking: pares restantes | lote 2a feito (§8) | os pares restantes, em lotes retomáveis | G2 define o ritmo | — | 144/144 pares nos dois pipelines, ou cobertura parcial declarada | sim | ≈ US$ 0,15 esperado e ≈ US$ 0,35 no pior caso observado, a preço de lista; US$ 0 faturado se a chave for de fato gratuita |
| II.3 estatística | Wilson por pipeline | bootstrap por agrupamento de tarefa; McNemar exato para a utilidade pareada; registro das dependências | II.2 | — | `RESULTS.md` com os 7 indicadores da Rev10 por pipeline | não | US$ 0 |
| II.4 suíte `workspace` | — | mapeamento offline: 24 ferramentas, inventários, política e agente `workspace.ath` | II.2 | — | ponte testada offline como a `banking` | não para o mapeamento; sim para a execução | 560 pares (1.200 execuções). Estimativa grosseira: US$ 2–5 com o 120B. **Exige autorização própria e um plano com limites de pago** |
| III.B reconciliador durável | o journal detecta chaves abertas (`verify-journal`) | `athc resume`, serviço durável e T9 sobre journals concatenados | I.1c | médio | journal interrompido em qualquer prefixo e retomado: toda chave termina | não | US$ 0 |
| III.C contratos `restoring` | T6 no cálculo sob hipótese de contrato | sintaxe de contrato com footprint; E5 com SMT | — | médio | `athc check` descarrega ou recusa a obrigação | não | US$ 0 |
| III.D Δ versionado | `contract-report` sugere a recalibração | `--propose`: nova versão do bloco `model` como PR, nunca editando a `main` | — | baixo | um PR gerado pela CI, com diff do contrato e hash novo | não (usa journals gravados) | US$ 0 |
| IV auditoria e release | §2 e §3 | tabela de alcance, critérios de aceitação, instruções de reprodução testadas | todos | — | §7 | não | US$ 0 |

**Equivalência declarada para o tripé com chaves (I.2).** O ARX numera as chaves pelo próprio
contador (`mint` → `k0`, `k1`, …) e registra `timeout_unknown`, uma linha `reconcile` por
tentativa, `reconcile_exhausted`, `transfer` e `resolved`. No núcleo, a chave é a identidade
da capacidade. A igualdade byte a byte será, portanto, **a menos de uma renomeação bijetora das
chaves**, canônica pela ordem da primeira aparição, aplicada na projeção do núcleo dos três
backends. Eventos cobertos: `act`, `pending`, `query` (com índice da tentativa e resposta) e
`settle`. Fora da equivalência: latências, o recuo de 2 s entre consultas e os textos de
diagnóstico.

Ordem de implementação: I.1a → I.1b → I.1c → I.1d → I.2. É o caminho de maior impacto estrutural
que pode ser verificado offline. III.B usa o tratador de I.1c; III.C e III.D são independentes.

### 6.2 Protocolo estatístico do AgentDojo

- **Unidade:** o par (tarefa de usuário, tarefa de injeção), uma execução por par e pipeline,
  temperatura 0. A Groq não garante determinismo, então a execução é uma amostra, não um valor
  fixo.
- **Dependência:** cada tarefa de usuário aparece em 9 pares, e cada injeção em 16. Erros do
  modelo se correlacionam dentro do grupo, e o Wilson ingênuo subestima a incerteza. O
  `RESULTS.md` passa a trazer, além do Wilson, um IC por bootstrap de agrupamento (reamostrando
  tarefas de usuário) e a taxa por tarefa de usuário.
- **Comparação pareada:** McNemar exato para a utilidade, sobre os mesmos pares nos dois
  pipelines.
- **Imutabilidade:** qualquer mudança no agente, no ataque, no baseline ou nos verificadores
  abre um experimento novo, com outro logdir. Melhorias identificadas no lote 1 (agenda no
  aluguel, `amount` como novo total) ficam para uma versão nova do agente.

### 6.3 A: correspondência formal

1. A v1.0 declara a correspondência como em §5: verificada por construção e por testes
   diferenciais sobre F, sem prova geral de refinamento.
2. **Validação de tradução por execução:** em vez de provar o ARX inteiro, cada execução de
   programa do núcleo pode ser conferida contra o `eval` provado (o `ath-lean-eval` como
   verificador do journal do núcleo). É uma garantia por execução, defensável e já quase
   implementada.
3. **Refinamento:** um modelo do interpretador do ARX restrito a F, em Lean, com uma simulação
   contra o `eval`. Trabalho grande, e fica para depois da v1.0.

### 6.4 B: recuperação após queda

- Journal antes do efeito: `pending` gravado e sincronizado antes da chamada.
- `athc resume <journal>`: reconstrói as `Pending[k]` sem `settle`, reconcilia pela chave e
  acrescenta os eventos ao mesmo journal encadeado.
- Obrigações `Transferred` saem para um reconciliador durável, um serviço idempotente pela
  chave e com o journal próprio.
- Formal: o T9 vale para qualquer prefixo de traço seguido da recuperação.

### 6.5 C: contratos `restoring`

Um contrato declarativo, como `restoring unscale on {replicas(d)}`, sobre um estado abstrato
com footprint explícito. O E5 descarrega a restauração por SMT, em aritmética linear sobre as
células. A cobertura do footprint obriga as pós-condições a ler só células do footprint.

### 6.6 D: Δ versionado

`athc contract-report --propose` gera o próximo bloco `model`, com versão nova e o diff. A CI
abre um PR e nunca altera a `main` sozinha. Cada journal já carrega o hash do contrato, então um
experimento antigo continua apontando para o contrato com que rodou.

## 7. Critérios de aceitação propostos para a v1.0

1. `cargo fmt`, `clippy -D warnings`, `cargo test`, `lake build --wfail` e a auditoria de
   axiomas sem mudança no conjunto (`propext`, `Quot.sound`, `Classical.choice`), com zero
   `sorry`.
2. O tripé com chaves (I.2) e os corpora de 400 + 400, sem divergência não explicada.
3. Todos os cassetes reproduzidos nos dois backends.
4. Banking com 144/144 pares nos dois pipelines, ou cobertura parcial declarada, e os
   indicadores de §6.2.
5. Uma tabela de alcance em que cada garantia anunciada aponta para um teorema, um teste ou
   "não garantido".
6. Decisão explícita, registrada, sobre cada lacuna de §5 que ficar aberta.
7. Instruções de reprodução executadas a partir de um checkout limpo.

## 8. Gasto da Rev10

Teto autorizado: **US$ 1,00** de consumo novo. O livro-razão é
`bench/agentdojo/runs/banking-openai-gpt-oss-120b/spend.jsonl`.

| lote | run | o que rodou | estimado | medido (tokens do servidor × preço de lista) |
| --- | --- | --- | --- | --- |
| 2a · conectividade | [42915660955](https://github.com/Antoniorjr4/Aletheia/actions/runs/42915660955) | sondas de limite e modelo; `user_task_2 × injection_task_1` no Aletheia | US$ 0,0013 (máx. 0,003) | US$ 0,000401, mais as sondas (< US$ 0,0001, fora do livro-razão) |
