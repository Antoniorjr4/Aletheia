# Rev13 — Efeitos com prova e fechamento operacional das garantias

Branch `rev13`, a partir de `rev12` (c424c65). Gasto Groq: **US$ 0,00**. Nenhuma chamada paga, nenhum
workflow pago, nenhum agendamento reativado, nenhum merge. O `banking.ath` oficial, os resultados e
journals do AgentDojo, a matriz adversarial (`MATRIX.md`) e a matriz dos contratos da Rev12
(`CONTRACTS.md`) continuam como estavam.

## Reproduzir

```
cd lean && lake build --wfail                                  # provas + 88 guardas de axiomas + ath-lean-eval
cargo test --workspace                                         # inclui os diferenciais abaixo
ATH_REQUIRE_LEAN=1 ATH_TRIPOD_N=400 ATH_KEYED_N=600 ATH_SAGA_N=600 ATH_PAID_N=400 \
  cargo test -p athc --test differential                       # tamanhos usados neste relatório
python3 bench/adversarial/harness.py --intents --athc target/release/athc     # M-I + M-II
target/release/athc obligations bench/agentdojo/banking_intents.ath          # o destino de cada contrato
```

## 1. Propriedades demonstradas (Lean 4, sem `sorry`, sem axioma novo)

Os axiomas de cada teorema estão fixados em `Axioms.lean` (88 guardas; só `propext`,
`Classical.choice`, `Quot.sound`).

**M-IV — orçamento fechado sob compensação.**
- `Sig.ccost` é o pior custo declarado do compensador; `Sig.acost t = cost t + ccost t` (se `t` é
  `compensable`) é o custo que o tipo gradua e que o λ_Ath debita na chamada.
- O ARX tem uma reserva de compensação (`resv`, a soma dos compensadores dos quadros abertos). O
  medidor confere `gasto + reserva + need ≤ limite`, onde `need` inclui o compensador só dentro de
  saga e `need ≤ acost`. A reserva é liberada no commit, no abort e no `R0611`.
- `Arx.arx_budget_closed`: para **todo** programa, bem tipado ou não, `gasto + reserva ≤ limite`
  é invariante. Vale atravessando sagas que falham e liquidam compensadores, e a pilha de quadros
  volta à forma inicial. A hipótese é `cc ≤ ccost`: cada compensador custa no máximo o declarado.
- `sim`, `arx_refines` e `arx_sound` agora valem com compensadores pagos, sob a mesma hipótese. A
  relação de simulação passou a ser `gasto do ARX + reserva ≤ gasto contabilizado`.
- `Counter.comp_cost_reserved` fecha o contraexemplo da Rev12:
  - sob limite 1, o λ_Ath recusa antes do efeito e o ARX não gasta nada;
  - sob limite 2, a saga compensa e o ARX termina em 2 = o limite.
- `comp_cost_breaks_budget` continua verdadeiro e agora mostra que a hipótese é necessária: um
  compensador que não declara o custo rompe o limite.

**M-III, registro — a operação refutada não é estornada.**
- `recLog`/`recSt` (`Eval.lean`) tiram do registro a operação que a reconciliação confirmou não
  aplicada.
- `T6_keys` (agora em `SagaKeys.lean`): as compensações são exatamente as operações emitidas **menos
  as refutadas**. A hipótese nova é `Refutes`: uma resposta `Failed` vem de operação não aplicada.
  O registro carrega o `KeysOk` do T9, porque a refutação vem depois da emissão e chaves de trechos
  consecutivos precisam ser disjuntas.
- `refuted_not_compensated`: o compensador cego da Rev12, que levava o mundo a −5, agora não roda,
  e o mundo termina em 0.
- `dishonest_refusal_strands`: sem `Refutes`, o efeito fica sem estorno (o mundo termina em 5). A
  hipótese é necessária.
- `Arx.call_refuted`: o modelo do ARX tira a operação do quadro e libera a reserva.

**M-III, WAL — queda em qualquer ponto** (`Wal.lean`). O protocolo de uma operação com chave tem
cinco pontos: persistir, enviar, aplicação remota, confirmação, encerramento. Para todo ponto de
queda, toda rede e toda classe de provedor:
- `no_silent_loss`: exatamente um desfecho (`close` ou `escalate`) se a intenção foi persistida, e
  nenhum se não foi.
- `evidence`: `close done` implica efeito aplicado e `close failed` implica efeito ausente.
- `at_most_once`: o efeito nunca é aplicado duas vezes.
- `opaque_escalates`: uma operação `opaque` em aberto é escalada, nunca fechada.
- `registry_after_recovery`: o que a recuperação tira do registro de compensação não tem efeito.

Sobre "exatamente uma vez":
- `opaque_not_exactly_once`: há dois mundos, com e sem o efeito, que produzem o mesmo log. Para
  `opaque` não existe "exatamente uma vez", e a Aletheia não o alega.
- `idempotent_exactly_once`: com deduplicação pela chave e a rede de volta, existe.
- `resend_without_dedup_duplicates`: sem deduplicação, o reenvio dobraria o efeito. Por isso só o
  `idempotent` recebe reenvio.

**M-I — a obrigação levantada vale no efeito** (`Lifting.lean`).
- `lifting_sound`: no fragmento do λ_Ath (valores, `let`, aritmética, `if`, inferência, endosso,
  ação), se o argumento da ação é exatamente um valor endossado por uma política que carrega o
  contrato, todo efeito satisfaz o contrato, para qualquer resposta do modelo.
- `amplified_not_discharged`: a análise recusa o contraexemplo da Rev12 (o endossado somado a uma
  constante).
- `lifted_example`: a forma da Rev13 é descarregada.

## 2. Mecanismos implementados

- **Reserva de compensação** (ARX e Python).
  - `Meter.held_*`; `reserve` confere gasto + reserva + custo.
  - A reserva do compensador é feita só dentro de saga e liberada no commit, no abort e no `R0611`.
  - `compensation_overrun` registra quando a hipótese `cc ≤ ccost` falha.
- **`R0611` no registro** (ARX, Python, vistas do núcleo). A operação refutada sai do quadro. O
  `resolved` de `Done` mantém o formato antigo, e as gravações reais da Groq continuam
  reproduzindo byte a byte.
- **WAL** (`arx::wal`, `aletheia_runtime.wal`).
  - A intenção é persistida antes do envio: chave, ferramenta, argumentos, provedor, obrigação e
    saga.
  - O desfecho é persistido com a evidência: `reply`, `reconcile` ou `recovery:*`.
  - O log também guarda as sagas: início, operações, compensações, fim e `saga_unlog`.
  - Cada registro é encadeado por BLAKE3 e sincronizado no disco; uma linha rasgada é descartada.
  - `Arx::recover` e `Runtime.recover` reconciliam por classe e escalam o que não se resolve. Depois
    compensam as sagas interrompidas, sem estornar as refutadas.
- **Obrigações levantadas** (`ath_syntax::obligations`, `athc obligations`).
  - A análise rastreia por `let`, `match` e `for`, com caminhos em campos e elementos, a origem de
    cada argumento que o `requires` lê.
  - Se todos são campos do mesmo elemento de um valor endossado, o contrato é conferido no `refine`
    (`obligation_reject`, com retentativa e `else`) e fica descarregado no efeito.
  - Uma violação de obrigação descarregada é `R0631`, defeito da análise; nunca foi observada.
  - Caso contrário, a análise reporta uma lacuna dinâmica com o motivo, e o monitor `R0630` fica.
- **Intenção autorizada** (`s.binds("r", x1, …, xn)`).
  - A tupla operação–destinatário–valor precisa ser uma linha de uma relação confiável.
  - O journal registra no efeito a linha e o BLAKE3 dela (`authorized`).
  - O endereço vale também se estiver num arquivo que o próprio pedido nomeia (`delegated`).
- **Dois registros** (relatórios do `athc run` e do Python, pipeline do AgentDojo).
  - `digests.semantic` é a cabeça da cadeia da vista do núcleo: sem tempo, é o que o programa fez.
  - `digests.evidence` é a cabeça do journal integral: com as medições, é como e quando.

## 3. Resultados reproduzíveis

| evidência | resultado |
|---|---|
| tripé com chaves, bytes crus Lean = ARX (com `R0611` reconciliado) | 600/600 |
| tripé com sagas, quatro backends | 600/600 |
| tripé base, quatro backends | 400/400 |
| M-IV com custos exatos (1/1024 US$) | 400 programas, modelo = ARX = Python no gasto e no journal, todos ≤ limite, reserva zerada no fim |
| M-IV, cobertura | 39 compensações pagas, 54 recusas pela reserva no topo, 45 recusas dentro de saga, 18 no fio do limite, 11 `R0611` |
| contraexemplo da Rev12 nos runtimes | sob limite 1: `B0202` estático e `R0801` dinâmico, gasto 0; sob limite 2: gasto 2 nos três |
| M-III, queda injetada | 1 190 quedas em 400 programas (4 pontos × até 3 operações com chave): nenhuma obrigação perdida, todo desfecho conferido contra a verdade do provedor, nenhum reenvio a `queryable`/`opaque`, toda saga interrompida encerrada |
| M-III, WAL recuperado ARX × Python | 122/122 byte a byte |
| M-I + M-II, `banking_intents.ath` | 6/6 sítios descarregados |
| matriz `I` (SafePlan + intenção) | 20/20 recusas, 5/5 controles, 0 proibidos, 0 resíduos |
| matriz `IP` (política que aceita tudo) | 20/20 recusas, 5/5 controles, 0 proibidos; 18 recusas no `refine`, 0 no efeito |
| R3 (2000.00 a beneficiário legítimo) | recusado (era resíduo) |
| U1 (endereço do arquivo nomeado) | executa, com utilidade oficial (era custo) |
| KP-N3 / IP-N3 (sem política) | executa o plano legítimo (era custo) |
| novos ataques B1, B2, B3, U2 | recusados no `refine` |
| obrigação levantada × lacuna, journal completo ARX = Python | `lifted_obligations_refuse_at_refine_and_bind_the_intent` |
| M-V, latências diferentes | registro semântico igual em 80/80; registro de evidências diferente em 76/80 (nos 4 restantes nenhuma ferramenta executa: não há latência a medir); ARX = Python nos dois |
| matriz adversarial oficial | 29/29, 4/4, 3/3 (inalterada) |

## 4. Limitações restantes

**M-IV**
- A hipótese `cc ≤ ccost` é necessária. O runtime a observa (`compensation_overrun`) e não a impõe.
- O λ_Ath contabiliza `acost` sempre; o ARX reserva só dentro de saga. O tipo, portanto, é mais
  conservador que o runtime.
- O verificador estático do `athc` usa o próprio pior caso. Ele não está provado igual ao tipo do
  Lean.
- O diferencial pago compara só a dimensão US$.

**M-III**
- O modelo do WAL é por operação. As hipóteses são:
  - decisão do provedor fixa por chave;
  - status consultado verdadeiro;
  - deduplicação no `idempotent`;
  - registro inteiro ou ausente;
  - intenção persistida antes do envio.
- O `SimWorld` permite recusar uma tentativa e aceitar a seguinte. A evidência ainda vale porque o
  desfecho reflete o estado no momento da reconciliação.
- Efeitos sem chave não são duráveis. Uma ação sem chave cuja resposta se perdeu na queda não tem o
  que reconciliar e não entra na compensação da saga interrompida.
- A recuperação não continua o programa, porque continuações não são persistidas. Ela encerra as
  sagas interrompidas por compensação. As operações escaladas são compensadas, e isso exige
  compensador total, como no `T6_keys`.
- A queda é simulada no processo (pânico / `BaseException`). A durabilidade de `fsync` é do sistema
  de arquivos.
- Não há comando `athc recover`: a recuperação é API (`Arx::recover`) e, no Python, o comando
  `crash`, usado nos testes.

**M-I + M-II**
- `lifting_sound` cobre o fragmento do λ_Ath. A análise de superfície, com caminhos em ADTs e
  listas, é verificada por testes, não por prova.
- A análise supõe uma fotografia confiável única por execução, o que vale nos dois runtimes.
- As relações `intents` e `delegated` do harness vêm de um extrator mínimo do pedido, que substitui
  uma confirmação estruturada. A correção dele está fora da linguagem.
  - O extrator é permissivo: liga cada IBAN a cada valor escrito no pedido.
  - A delegação confia no arquivo que o usuário nomeou.
- Com o levantamento, a tabela da Rev12 (`--contracts`) continua toda verde. Mas as recusas passam
  do efeito para o `refine`, e o KP-N3 executa o plano legítimo. O `CONTRACTS.md` versionado é o da
  Rev12 e só se reproduz com o código da Rev12.

**M-V**
- A correspondência entre o modelo do ARX e o binário continua empírica: é validação por execução
  (vista do núcleo do modelo = a do binário em cada programa gerado), não uma prova sobre o Rust.
- `obligation_reject`, `authorized` e o WAL ficam fora da vista do núcleo. Entram só no registro de
  evidências.

## 5. Problemas abertos

1. Estender `lifting_sound` a `match`/`for` sobre ADTs e listas, cobrindo o que a análise do `athc`
   já faz.
2. Durabilidade de efeitos sem chave: exigir chave para todo efeito irrevogável dentro de saga, ou
   persistir a intenção também sem chave.
3. Persistir continuações para retomar, e não só encerrar, uma saga interrompida.
4. Unificar o pior caso do verificador estático com `acost` e provar a equivalência.
5. Extrair o núcleo do ARX do modelo Lean (ou validar a tradução por certificado) para tirar o
   binário da dependência exclusiva de testes.
