# Aletheia v1.0-RC5: o agente inteiro na linguagem

Até a RC4, um agente Aletheia era verificado, autorizado e auditável, mas ainda dependia de duas
camadas de fora da linguagem para existir no mundo: **cola** para alcançar uma API real (um
adaptador por ferramenta, e algum lugar para guardar o token) e **um operador** para o que
acontecia depois de uma queda (`athc recover` fechava as obrigações, e o programa morria). A RC5
tira as duas. Um arquivo `.ath` declara o agente, as ferramentas, a quem elas se ligam e com que
segredo; o mesmo programa continua, com `athc resume`, depois de um `SIGKILL`.

> **RC5.1 (consolidação).** A revisão da RC5 encontrou quatro defeitos, todos reproduzidos como
> testes que falhavam (`docs/evidence/rc5.1-achados-na-rc5.txt`) e corrigidos: o monitor de tipo
> era contornado pela retomada; a fita reescrita era aceita; a classificação de "próprio host"
> aceitava qualquer literal IPv6; e `202 Accepted` era tratado como efeito concluído. A seção 9
> descreve cada um. O texto abaixo já é o da RC5.1, com o alcance de cada prova delimitado.

```sh
scripts/rc5-smoke.sh              # a demonstração, num comando (exit 0 = todas as conferências)
cargo test -p athc --test rc5     # quinze testes com processos reais, HTTP e SIGKILL
cd lean && lake build --wfail     # Resume.lean, Boundary.lean e as 153 guardas de axiomas
```

As RC1 a RC4 continuam passando sem alteração: `tests/rc1.rs` a `tests/rc4.rs`, os quatro
scripts de demonstração e os programas de `examples/rc1` a `examples/rc4`. O WAL de uma execução
é byte a byte o da RC4 (o que a RC5 grava vai para uma fita ao lado), e a raiz do Codex da
tesouraria da RC2 continua `#4877f356ba52`.

## 1. Os dois limites, reproduzidos antes de corrigir

O commit `RC5: dois limites da RC4 como testes que falham` só acrescenta testes. Os dois
**falham** nele; a saída está em `docs/evidence/rc5-limites-na-rc4.txt`.

| Limite | Na RC4 | Fechado por | Teste |
|---|---|---|---|
| **A queda mata o agente** | `athc recover` fecha as obrigações; o que faltava executar nunca acontece (limite declarado em `docs/RC4.md`, §9) | `athc resume`: re-execução conferida a partir da fita das entradas | `a_killed_agent_resumes_and_finishes_exactly_once` |
| **O tipo do resultado não é conferido** | uma ferramenta `-> Text[..24]` aceita 5000 caracteres, um objeto ou um inteiro do provedor | o monitor de tipo na fronteira (`R0640`) | `a_provider_cannot_hand_the_program_an_ill_typed_value` |

A ligação nativa (`bind`) não é um defeito corrigido: é uma capacidade que não existia. Ela não
está na tabela.

## 2. Missão I: a retomada verificada (`athc resume`)

**A ideia.** Um programa Aletheia é determinístico dadas as suas entradas: as amostras dos
modelos, os resultados das ferramentas, a fotografia confiável e os argumentos. Com `--wal`, o
ARX grava cada entrada numa **fita** ao lado do WAL (`<wal>.tape`: durável, encadeada, sob
trava) no instante em que o programa a observa:

* `input`: a resposta de um modelo, pela ordem da amostra, com o hash do prompt;
* `ret`: o que uma chamada devolveu **ao programa** (valor, erro ou desvio do tratador), com o
  relógio e o medidor de orçamento depois dela;
* `value`: o valor que o provedor devolveu, gravado antes de o desfecho ser fechado no WAL.

**As duas fases.** `athc resume`, num processo novo:

1. fecha as intenções em aberto pelo protocolo da classe de cada provedor (a recuperação das RC1
   a RC4, inclusive a ancorada), **sem compensar as sagas**: elas continuam;
2. re-executa o programa desde o início. Cada entrada gravada vem da fita, sem tocar no mundo, e
   é conferida contra o que o programa pede (ferramenta e argumentos, modelo e prompt). O modelo
   não é consultado de novo, nenhum provedor é chamado, e o medidor volta ao que era.

**A fronteira da queda.** A operação que a queda interrompeu é decidida pelo desfecho que a
fase 1 gravou:

| Desfecho no WAL | A chamada, na retomada |
|---|---|
| `done` | devolve o valor: o efeito aconteceu uma vez e não é reenviado |
| `failed`, provadamente não enviada (`recovery:unspent`, `anchor:unsent`, `anchor:unreserved`) | é executada agora, com uma **chave nova** (a antiga nunca é reaproveitada) |
| `failed`, o provedor responde que não aplicou (`recovery:status`), sem concessão | idem |
| `failed`, idem, **com** concessão | falha (`R0600`): a autorização daquela tentativa foi consumida |
| `failed`, recusa do provedor | falha, como falharia sem queda |
| `escalated` | é o `TimeoutUnknown` da linguagem: o tratador do programa decide |

Daí em diante a execução é ao vivo, com as mesmas conferências de sempre: contratos, concessões
(válidas no tempo atestado **agora**), ciclo de vida ancorado.

**O que a retomada recusa.** Outra fotografia confiável ou outros argumentos (`R0652`); uma
execução que `athc recover` já encerrou, compensando a saga (`R0653`); outro programa (`W0004`).
Se o programa pedir algo que a fita não registra, a execução é envenenada (`R0650`): nenhuma
chamada, amostra ou compensação sai depois da divergência, e o resultado é descartado.

**O modelo de confiança da fita.** A fita é um arquivo local. O encadeamento detecta corrupção,
não um adversário: quem escreve no disco do executor pode reescrevê-la e refazer o encadeamento.
O que depende dela, e o que não depende:

| Propriedade | Depende da integridade da fita? | O que a sustenta |
|---|---|---|
| autoridade: nenhum efeito sem concessão válida, nenhuma concessão usada duas vezes | não | o registro de consumo (relido sob trava) e a âncora; as concessões das operações já decididas só são vistas como livres pela conferência antecipada (`refine`) |
| política e contratos sobre o plano | não | são reavaliados na re-execução, sobre o que a fita entregar |
| o tipo dos valores que entram no programa | não (desde a RC5.1) | toda variante que a fita pode gravar é conferida: o valor de uma chamada contra o tipo da ferramenta, o desvio do tratador de `TimeoutUnknown` contra o tipo do tratador (`R0640`); uma variante que uma chamada não produz envenena a retomada (`R0650`); um tipo que a conferência não cobre não é aceito às cegas |
| o segredo | não | nunca é gravado |
| **o orçamento gasto** (tokens, dólares, tempo) | **sim** | o medidor é reposto a partir da fita |
| **o caminho** que o programa retoma (valores, erros, desvios de tratador) | **sim** | a fita é a memória do que foi observado |

Para as duas últimas linhas, numa execução com autoridade temporal (em que a âncora já é parte
do protocolo) a âncora guarda a **marca da fita**: antes de cada operação com chave sair, e ao
fim da execução, ela registra (só avançando) o compromisso com o comprimento e a cabeça da
fita. A retomada só aceita uma fita que passa por uma marca da sua execução; reescrita,
truncada, apagada ou trocada, é recusada (`L0008`) **antes da fase de recuperação**, que é a
primeira que pode enviar algo (ela reenvia operações idempotentes em aberto): zero chamadas ao
provedor com a fita adulterada. O que foi observado
depois da última marca (no máximo o trecho entre duas operações com chave) continua confiado ao
disco, e numa execução sem âncora a fita inteira é: quem a reescreve pode subdeclarar o gasto e
escolher o caminho, dentro do que a política, os contratos e as concessões permitem.

**Medido** (`tests/rc5.rs`, processos reais e `SIGKILL`):

* a matriz inteira: 6 pontos de queda × 4 operações com chave, 24 casos. Em 22 o processo é
  morto e o estado do provedor depois da retomada é o da execução sem queda. Em 1 (`reserved`
  na quarta operação, que não tem concessão e portanto não tem esse ponto) a execução termina
  sem queda, e retomá-la não muda nada. No caso restante (o agendamento morto entre a
  autorização de envio e o envio) a chamada falha com `R0600`, com os pagamentos no lugar e
  nada duplicado: é a linha "com concessão" da tabela;
* três quedas seguidas (a retomada também é morta, duas vezes) e a quarta execução termina;
* retomar uma execução terminada não chama o provedor (zero chamadas ao vivo);
* o gasto da execução retomada (tokens e dólares) é o da execução sem queda, e um modelo que
  responderia outra coisa na retomada não é consultado.

## 3. Missão II: a fronteira nativa

### 3.1 `bind`

```
bind bank_api {
  url       = env("BANK_URL")
  auth      = secret("BANK_TOKEN")
  status    = "GET /v1/operations"
  statement = "GET /v1/statement -> id"
  pay       = "POST /v1/payments -> id"
  schedule  = "POST /v1/schedules -> id"
  …
}
```

Cada rota tem o nome de uma `tool` declarada. O corpo da requisição são os parâmetros
**nomeados** da ferramenta (capacidades e a chave não são dados do provedor); num `GET`, vão na
query. A chave `own IdemKey` vai no cabeçalho `Idempotency-Key`, com a identidade da execução. O
resultado é a resposta, ou o campo que `->` nomeia. `status` é a consulta por chave dos
provedores `queryable`. O recibo assinado do provedor, quando vem na resposta, alimenta o ciclo
de vida ancorado da RC4 sem mudança.

| O que o HTTP diz | O que a linguagem vê |
|---|---|
| a conexão não abriu | `Unreachable`: nada saiu do processo |
| 200, 201, 204 | o valor, conferido contra o tipo |
| 202 e os outros 2xx | `Timeout`: o pedido foi aceito, o efeito que a ferramenta promete não está concluído; a consulta por chave (`status`) é quem decide |
| 4xx (menos 408) | `Failed`: recusa do provedor, o efeito não aconteceu |
| 408, 5xx, prazo, resposta cortada | `Timeout`: o `TimeoutUnknown` da linguagem |

Esse mapeamento é uma convenção, não o contrato de cada API: uma API que responde 200 antes de
concluir o efeito, ou que usa 4xx para um estado transitório, precisa de uma ligação que diga
isso. Ainda não há como declará-lo por rota (seção 7).

A ligação entra no Codex (endpoint, nome do segredo, rotas em ordem de ferramenta): é
identidade do programa, e é ela que o WAL e as concessões citam.

Conferido antes de rodar (`athc check`): rota para o que não é ferramenta (`E0431`), ferramenta
ligada duas vezes (`E0432`), segredo em claro para outro host (`E0433`), operação com chave ou
não `reversible` num `GET` (`E0434`). `athc check` imprime cada ligação: quantas ferramentas,
para onde, e o **nome** do segredo.

### 3.2 Segredos por referência

`auth = secret("NOME")` nomeia uma variável do ambiente do executor. Um segredo literal não é
sintaxe, e a linguagem não tem expressão que leia um segredo. O valor só existe no cabeçalho
`Authorization` da requisição: não é argumento, não vai ao journal, ao WAL, à fita, ao relatório
nem ao prompt de modelo algum. Se o provedor o devolver numa resposta ou num erro, ele é trocado
por `‹secret:NOME›` antes de o valor entrar no programa. Sem a variável no ambiente, nada executa
(`R0642`); sobre `http` sem TLS, um segredo só viaja para o próprio host (`R0643`). "Próprio
host" é `localhost` ou um endereço de loopback (`127.0.0.0/8`, `[::1]`), decidido pelo endereço
analisado; qualquer outra forma (outro literal IPv6, endereço mapeado, nome que começa com
`localhost`) é outro host, e uma url com credenciais (`usuário@`) é recusada.

### 3.3 O monitor de tipo

O que um provedor real devolve (por `bind` ou pelo protocolo de linha) é conferido contra o tipo
declarado da ferramenta: `Int`, `Bool`, `Text`, com os limites, e listas deles. A contabilidade
do efeito segue normalmente (ele aconteceu, e numa saga é compensado); o valor fora do tipo não
entra no programa, e a chamada falha com a culpa atribuída ao provedor (`R0640`). O mundo
simulado em memória não é um provedor e não passa pelo monitor.

O monitor está em **todo caminho** por onde um valor de ferramenta chega ao programa: a chamada
ao vivo, a reconciliação (`reconcile`), o valor que a retomada lê da fita e o valor que a
recuperação ou o recibo ancorado trouxeram. A culpa decidida ao vivo é gravada com o valor,
antes de o desfecho ser fechado: uma queda entre o fechamento e o erro não a perde.

## 4. Missão III: os modelos em Lean

`lean/Aletheia/Resume.lean` e `lean/Aletheia/Boundary.lean`, Lean 4.20.0 sem Mathlib. Dezoito
teoremas novos, 153 guardas `#print axioms` no total (eram 135), nenhum `sorry`, nenhum axioma
além dos padrões do Lean (`propext`, e `Quot.sound` em `budget_survives`). T1, T2, T3, T6, T9 e
os modelos das RC2 a RC4 não foram tocados.

**`Resume.lean`.** O programa é uma árvore de interação; o adversário é a queda, que pode
ocorrer em qualquer configuração, quantas vezes quiser, inclusive entre o envio e a gravação.

| Teorema | Formulação | Hipóteses |
|---|---|---|
| `replay_fidelity` | em toda configuração alcançável, re-executar do início com a fita devolve o programa em curso | R1, R3 |
| `crash_is_identity`, `crash_refines` | o passo de queda e retomada não muda programa, mundo nem fita; tudo o que é alcançável com quedas é alcançável sem nenhuma | R1, R3 |
| `at_most_once` | com quantas quedas for, o provedor aplica cada chave **no máximo** uma vez. É segurança; não diz que toda operação termina | R2 |
| `replayC_checked`, `replayC_agrees` | nada entra pela fita sem passar pelo monitor, qualquer que seja a fita; numa fita íntegra, o monitor não muda o que é retomado | — |
| `tape_is_evidence` | toda resposta de operação na fita é a que o provedor deu para aquela chave | R2 |
| `budget_survives` | o gasto **gravado** é função da fita: nenhum passo o diminui, e a queda o deixa como estava | R3 |
| `model_not_asked_again` | a amostra gravada é a que a re-execução entrega; `replay` não tem modelo | — |
| `divergence_stops` | se o programa pede outra coisa que a fita registra, não há passo de retomada | — |

**R1** o programa é função das suas entradas (a fotografia e os argumentos são os mesmos: o
executável recusa outros). **R2** o provedor deduplica pela chave, ou o desfecho da operação
interrompida foi decidido pela recuperação (`Wal.lean`, `Lifecycle.lean`). **R3** a fita é um
prefixo do que foi observado (sustentada pela marca na âncora, ou pela confiança no disco).

**O alcance destas provas.** `crash_refines` vale no modelo de `Resume.lean`: um provedor que
deduplica, sem concessões nem ciclo de vida. A composição da retomada com as chaves novas, as
concessões e a âncora é sustentada pelos testes (a matriz de quedas), **não por uma prova
integrada**. `at_most_once` não é "exatamente uma vez": 1 dos 24 casos da matriz não termina.
`budget_survives` cobre o que foi gravado: a amostra perdida entre o modelo e a fita gera uma
nova chamada e um custo sem registro.

**`Boundary.lean`.**

| Teorema | Formulação | Hipóteses |
|---|---|---|
| `check_sound`, `check_complete` | a conferência executável decide exatamente "o valor habita o tipo" | — |
| `boundary_sound` | qualquer que seja o provedor, o que entra no programa por uma ferramenta de tipo `τ` habita `τ` | — |
| `boundary_transparent`, `blame_is_earned` | quem devolve um valor do tipo nunca é culpado, e o valor passa sem alteração | — |
| `secret_noninterference` | no modelo simbólico (o provedor só copia o segredo), o que o programa observa é o mesmo para quaisquer dois valores do segredo. Não é proteção contra qualquer comportamento de uma API que recebe a credencial | B1, B2 |
| `without_scrub_it_leaks` | sem a redação, um provedor que copia o segredo faz o programa depender do valor | (o contraexemplo) |

**B1** o provedor pode copiar o segredo para a resposta, mas não transformá-lo. **B2** o canal
não é modelado. **B3** (por construção da gramática, não é teorema) a linguagem não tem
expressão que leia um segredo.

A ligação entre o Lean e o Rust continua sendo por correspondência de enunciados, não por
extração de código.

## 5. O runtime Python

Continua um backend independente de comparação. Um programa com `bind` é recusado por ele com o
mesmo código e o mesmo texto do ARX sem a ligação (`R0641`), antes de qualquer chamada
(`a_bound_program_is_refused_identically_without_the_binding`): a garantia nunca é simulada. O
backend Python não implementa a fita nem `resume`.

## 6. Verificação

* `cargo test --all`: 198 aprovados, 0 falhas (179 da RC4, 3 unitários de `arx`, 15 de
  `tests/rc5.rs`, 1 diferencial novo);
* `lake build --wfail`: 153 guardas de axiomas, sem `sorry`;
* runtime Python: 12 testes;
* `scripts/rc5-smoke.sh`: sai com 0; os quatro scripts anteriores também;
* `cargo fmt --check` e `cargo clippy -D warnings` limpos;
* Groq: US$ 0,00. Nenhuma chamada paga: o modelo de todos os testes é um roteiro local.

## 7. Limites

* **Não há supervisor.** `athc resume` continua a execução quando um processo novo o executa;
  nos testes, quem o inicia é o script. Detectar a queda e disparar a retomada sem intervenção
  é um componente que ainda não existe.
* **O significado do HTTP é uma convenção fixa.** 200, 201 e 204 são efeito concluído; não há
  como declarar por rota o que cada status significa para o contrato daquela ferramenta.
* **A fita só é protegida até a última marca, e só em execuções com autoridade temporal**
  (seção 2). Sem âncora, o orçamento gasto e o caminho retomado são confiados ao disco.
* **MCP não está nesta revisão.** O ambiente de construção não permitiu executar um servidor
  MCP oficial, e um cliente que nunca falou com um servidor real seria uma integração simulada.
  `bind` cobre HTTP; MCP é o próximo transporte da mesma construção.
* **`bind` tem uma convenção fixa.** Corpo JSON com os parâmetros nomeados (ou query, num
  `GET`), resultado na resposta ou num campo dela, `Authorization: Bearer`. Não há parâmetros
  no caminho, cabeçalhos próprios, OAuth, paginação nem tipos algébricos no resultado.
* **HTTPS não foi exercitado.** Os testes usam `http` para o próprio host. O caminho `https`
  é o do cliente HTTP já usado para os modelos, mas nenhum teste desta revisão o percorre.
* **A redação do segredo é simbólica (B1).** Um provedor que devolve o segredo codificado ou
  cortado não é detectado. O provedor recebe o segredo: ele é confiado com ele.
* **A autorização consumida não volta.** Uma operação com concessão morta entre a autorização
  de envio e o envio, num provedor `queryable`, falha na retomada: a âncora não libera uma
  concessão depois de autorizar o envio. É 1 dos 24 casos da matriz.
* **Uma amostra pode ser perdida.** Se a queda vem entre a resposta do modelo e a gravação na
  fita, a retomada consulta o modelo de novo: o custo daquela chamada não fica registrado.
* **O tempo de parede do orçamento é o lógico.** O tempo em que o agente ficou morto não conta.
* **As fontes assinadas têm prazo.** A retomada confere a fotografia, o pedido e as concessões
  no tempo atestado de agora; se expiraram, é preciso assinar de novo.
* **`recover` e `resume` são alternativas.** Depois de `athc recover` compensar a saga, a
  execução está encerrada (`R0653`). Uma execução no mundo simulado não é retomável.
* **Não há catálogo único de códigos** (`athc explain`): os códigos estão nas tabelas de
  `docs/RC1.md` a `docs/RC5.md`.
* **O monitor cobre escalares e listas deles**, em provedores reais. Na retomada, um valor de
  ferramenta (ou de tratador) de outro tipo, como um tipo algébrico, não é aceito: a execução
  que dependa dele não é retomável. `Unit` é sempre `Unit`, diga a fita o que disser.
* **O modelo de `Resume.lean` é o do provedor que deduplica.** A chave nova, as concessões e o
  ciclo de vida ancorado na retomada estão nos testes, não no modelo.
* Continuam valendo os limites da RC4: o provedor opaco depois de um envio possível
  (`opaque_undecidable`), as hipóteses H1 a H4, a falha fechada sem âncora ou testemunha.

## 8. Códigos novos

| Código | Significado |
|---|---|
| `E0431` | rota de `bind` para o que não é ferramenta, ou url que não é http(s) |
| `E0432` | ferramenta ligada mais de uma vez |
| `E0433` | segredo em claro (http sem TLS) para outro host |
| `E0434` | operação com chave, ou não `reversible`, ligada a um `GET` |
| `R0640` | o provedor devolveu um valor fora do tipo declarado |
| `R0641` | programa com `bind` sem a ligação (ARX com `--no-bind`, e o backend Python) |
| `R0642` | variável de ambiente da url ou do segredo ausente |
| `R0643` | o segredo viajaria em claro para outro host |
| `R0650` | retomada divergente: o programa pede o que a fita não registra |
| `R0651` | desfecho sem valor na fita, ou intenção sem desfecho: a retomada não inventa resultados |
| `R0652` | outra fotografia confiável ou outros argumentos |
| `R0653` | execução já encerrada por `athc recover` |
| `L0008` | a fita não passa por nenhuma marca da âncora: reescrita, truncada, apagada ou trocada |

## 9. RC5.1: os quatro achados da revisão

O commit `RC5.1: os quatro achados da revisão da RC5 como testes que falham` só acrescenta
testes; os quatro falham nele.

| Achado | Na RC5 | Correção | Teste |
|---|---|---|---|
| **O monitor contornado pela retomada** | o provedor devolvia um valor fora do tipo, o processo morria depois de fechar o desfecho, e a retomada entregava o valor ao programa; um valor reescrito na fita também entrava | o monitor em todo caminho de entrada; a culpa gravada com o valor antes do fechamento | `an_ill_typed_value_does_not_enter_through_resume` |
| **A fita reescrita aceita** | reescrita com o encadeamento refeito (o medidor zerado), a retomada seguia | a marca da fita na âncora (`L0008`); o medidor só é reposto para cima; o modelo de confiança da seção 2 | `a_rewritten_tape_is_refused_by_the_anchor` (reescrita, truncada, apagada) |
| **"Próprio host" aceitava qualquer literal IPv6** | `http://[::ffff:198.51.100.10]` e `http://localhost:x@evil.example` passavam pela guarda do segredo em claro | o endereço é analisado (`loopback_url`), no verificador e no runtime; url com credenciais é recusada | `only_the_own_host_is_loopback`, `a_secret_never_leaves_in_clear_for_another_host` |
| **`202 Accepted` como efeito concluído** | todo 2xx fechava a operação como `done` | só 200, 201 e 204; o resto é resultado desconhecido | `an_accepted_request_is_not_a_completed_effect` |

**Segunda rodada.** A revisão da consolidação apontou, por inspeção, dois caminhos ainda
abertos. Os dois se confirmaram em execução (`docs/evidence/rc5.1-segunda-rodada.txt`) e foram
fechados:

| Achado | Antes | Correção | Teste |
|---|---|---|---|
| **A recuperação enviava antes de a fita ser conferida** | com uma operação idempotente morta antes do envio e a fita ancorada reescrita, a fase 1 da retomada reenviava o pagamento e só então vinha o `L0008` | `athc resume` confere a fita contra a âncora antes da fase de recuperação (`tape_verify`) | `a_rewritten_tape_is_refused_before_recovery_sends_anything` (zero chamadas ao provedor) |
| **Variantes de retorno sem conferência** | um registro `ok` trocado por um desvio de tratador com um inteiro fazia o programa devolver `7` onde o tipo é `Text`; `ret` e `resume` eram reconstruídos direto | o desvio do tratador é conferido contra o tipo declarado do tratador; `ret`, `resume` e qualquer outra variante não existem como retorno de chamada e envenenam a retomada; não são mais gravadas | `every_recorded_variant_is_checked_on_replay` (cinco variantes) |

Um quinto caminho apareceu ao corrigir o primeiro: o valor obtido por `reconcile` (reenvio ou
consulta, dentro do tratador de `TimeoutUnknown`) também entrava no programa sem o monitor, ao
vivo. Passa por ele agora.
