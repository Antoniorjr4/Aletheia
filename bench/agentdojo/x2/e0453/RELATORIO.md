# E0453: mistura de operações aprovadas (28/11/2026)

Implementação autorizada pela decisão arquitetural X2 / E0453 de 28/11. Não houve merge e não houve
gasto com API (orçamento adicional US$ 0,00). Nenhum resultado histórico foi recalculado:
`../resultado.json`, `../relacional/resultado.json`, `bench/obrigacoes/*.json` e as métricas de autoria
ficam como estavam. O que é de depois da E0453 fica nesta pasta.

## 1. A garantia, classificada

**O que vale (estático, `athc check`, estrato E3):** uma chamada de ferramenta aceita não combina dados
de duas ou mais operações aprovadas distintas. "Operação aprovada" é um valor endossado por um
`refine`: o valor inteiro, ou um elemento de uma lista dele escolhido por um laço. Os dados contam
mesmo se passarem por variáveis, reconstrução de construtor, transformação, seleção de campo (por
padrão ou por ponto), junção de `if`/`match`/`else`, lista literal ou quantificador.

**O que não vale, e o diagnóstico diz:**

- **não é fidelidade nem identidade.** Os campos de uma só operação ainda podem ser transformados
  (`fit(memo ++ "!", 12)`), e a chamada recebe então um valor que a política não viu. Esses sítios
  são aceitos e classificados como "uma operação, transformada" (`athc operacoes`); não há prova de
  que sejam os aprovados;
- **a agregação pura sobre uma lista inteira** (`join(ops.memo, ",")`, `len(ops)`, `sum w in ops => …`
  sozinhos) é aceita. Ela depende de todos os elementos, isto é, de várias operações; a E0453 não a
  cobre. Ver a seção 6;
- **dependência de controle:** a regra é sobre dados. Um `if` cuja condição vem de outro elemento não
  é mistura para a E0453;
- **quantidade:** duas chamadas com a mesma operação, ou uma operação aprovada que não é executada,
  não são assunto da E0453 (seção 7).

Texto do diagnóstico (o caminho autorizado citado existe hoje na linguagem):

```
error[E0453]: a chamada de `pay` combina dados de 2 operações aprovadas distintas
   |     for a in ops { match a { Pay { to } => for b in ops { …
   |         - origem: um elemento de `Plan.ops`
   |                                               - origem: um elemento de `Plan.ops`
   = cada operação foi aprovada sozinha pela política; a combinação delas não foi
   = a E0453 não prova que os argumentos sejam os aprovados: os campos de uma só operação podem
     ser transformados, e a agregação sobre uma lista inteira, sozinha, não é coberta
   = ajuda: use os campos de uma só operação; se a combinação é legítima, declare-a como uma
     variante do plano, proposta pelo modelo e decidida pela política no `refine`
```

## 2. Preservação da origem: o que foi testado antes de virar erro

Sondas em [sondas/](sondas/) (`gen.py` gera; o teste `ciclo1_e0453` confere que os arquivos são os
gerados). Todas eram aceitas pelo compilador anterior (2950dc5).

| sonda | via | E0453 |
| --- | --- | --- |
| m1 | dois laços sobre a mesma lista | recusa |
| m2 | variáveis intermediárias | recusa |
| m3 | reconstrução `Pay { to: to, memo: memo }` com campos de dois elementos | recusa |
| m4 | transformação de um dos campos | recusa |
| m5 | junção `if c { a } else { b }` | recusa |
| m6 | elemento + `join(ops.memo)` (lista inteira) | recusa |
| m7 | elemento + campo da raiz do plano (`note`) | recusa |
| m8 | elementos de duas listas do plano | recusa |
| m9 | elemento + quantificador sobre a lista | recusa |
| m11 | lista literal `[a, b]` | recusa |
| m12 | campos por ponto `a.to`, `b.memo` | recusa |
| m13 | valores de dois `refine` | recusa |
| m14 | `if c { a.to } else { b.to }` com `a.memo` | recusa |
| m15 | `refine … else { o }` com outro elemento | recusa |
| m16 | `else` aninhado: elemento + a lista inteira | recusa |
| m10 | só agregados: `pay(fit(join(ops.to)), join(ops.memo))` | **aceita** (fora da garantia) |
| u1–u11 | mesmo elemento; literal e dado confiável; transformada; alias e `match` duplo; `if` com o mesmo elemento; lista literal de elementos inteiros; ponto; `else` aninhado agregado; `refine … else { o }`; `if` entre campo e campo transformado; `if` entre dois campos do mesmo elemento | aceitas |

Os controles aceitos (u1–u11 e m10) rodam no ARX e no Python com as mesmas chamadas, conferidas uma
a uma (`os_controles_aceitos_executam_igual_nos_dois_runtimes`).

**O plano inteiro como origem única.** No protótipo de 27/11, o valor endossado inteiro era uma
origem só, e `join(ops.memo)` (todos os elementos) junto com um elemento passava como "uma operação".
Isso permitia recombinar. Agora a dependência da lista inteira é a origem `[∀]`, distinta de cada
elemento e do valor inteiro: elemento + `[∀]` (m6, m9, m16) e elemento + campo da raiz (m7) são
mistura.

**Falsos positivos achados e corrigidos durante a implementação.** A primeira versão recusava três
programas legítimos, todos com junção:

- `refine A else { refine B }` seguido de agregação sobre o resultado (achado no teste diferencial
  `composed/else-aninhado`): o resultado vem de A ou de B, nunca dos dois;
- `refine … by PO else { o }` dentro de `for o in ops` (`bench/obrigacoes/revisao2/m/m2`, que a análise
  de obrigações descarregava): a operação nova ou o elemento;
- `if c { to } else { to ++ "" }` sobre o mesmo elemento (`revisao3/g/g02d`).

Correção: a operação passou a ser identificada pelo último ponto de escolha (o laço ou a junção que
escolheu o valor em execução), e a junção só registra um ponto de escolha quando os ramos têm
operações diferentes. Os três viraram controles (u8, u9, u10, u11), com as tentativas de fuga
correspondentes (m14–m16).

## 3. Exemplo executável: antes permitia, agora recusa

A testemunha X2 de 27/11 (`../testemunha.py`), com o espião no `FunctionsRuntime.run_function` do
AgentDojo, rodada com o compilador anterior ([testemunha_antes.json](testemunha_antes.json),
idêntica ao `../resultado.json` histórico) e com o atual ([testemunha_depois.json](testemunha_depois.json)).
Mutação X2 (`banking_v2_keeping.ath`, campo `amount` tirado de outro elemento):

| caso | executor | antes | depois |
| --- | --- | --- | --- |
| duas operações | A-ARX | 4 chamadas, 2 recombinadas | recusado na compilação (E0453), 0 chamadas |
| duas operações | A-Py | 4 chamadas, 2 recombinadas | recusado na compilação (E0453), 0 chamadas |
| duas operações | P1 (Python endurecido) | 2 chamadas, 1 recombinada | igual (não passa pelo compilador) |
| duplicada | A-ARX / A-Py | 4 chamadas para 2 aprovadas | recusado na compilação |
| uma operação | A-ARX / A-Py | 1 chamada, a aprovada | recusado na compilação |

O último caso mostra o custo de uma recusa estática: o programa mutante não roda nem quando o plano
tem uma operação só. A versão correta roda igual antes e depois, nos três executores.

## 4. Composições legítimas e `requires`

- `../composicao/c1` (operação aprovada + pedido confiável + literal) e `../composicao/c2` (composição
  proposta como variante do plano e decidida pela política no `refine`): aceitas;
- `../composicao/c3` (composição feita pelo programa com campos de duas operações): recusada;
- `../relacional/correto.ath`, com `requires s.binds("allowed", recipient, amount)`: aceito; nos dois
  runtimes executa as duas chamadas aprovadas e recusa no `refine` o plano com par fora da lista
  (obrigação demonstrada), igual ao resultado de 27/11 ([relacional_depois.json](relacional_depois.json));
- `../relacional/misturado.ath`: recusado na compilação. Antes, com a lista que permitia as quatro
  combinações, o contrato passava e a recombinação executava.

## 5. Regressão

- **Corpus versionado** ([corpus.json](corpus.json), `corpus.py`): 312 programas `.ath`, incluindo as
  141 versões de autoria do M10 e do M11. Mudaram de aceitação 30, todos para E0453, todos programas
  construídos para misturar (`bench/obrigacoes` casos e revisões, `x2/relacional/misturado`,
  `x2/composicao/c3`). Nos 29 com `requires`, a análise de obrigações já dizia "dinâmica: vem de outro
  elemento" antes. Nenhum exemplo, nenhuma autoria e nenhum programa de benchmark mudou;
- **triagem RC8–RC10:** o resumo `join(notes.issue, ", #")` continua aceito, como agregação pura;
- **testes:** `cargo test --all` (335), clippy, fmt, as suítes Python, os smokes RC1–RC10;
- **tripés gerados:** o gerador do núcleo não conhece a regra e, em 7 de 4000 sementes (núcleo 3,
  sagas 3, chaves 1), produz uma chamada com valores de dois `refine` (`t2(c, x1 <= x3)`). Esses saem
  do tripé com registro; qualquer outra recusa continua falha. Nas sementes padrão, só `core/99`.

**Desvios declarados.** Os testes B1 e F2 (`ciclo1_obrigacoes`) rodavam as reproduções nos dois
runtimes; elas não compilam mais, e o teste agora confere a E0453 e a análise de obrigações sobre a
AST. O comportamento em execução de antes fica em `bench/obrigacoes/depois.json`. `testemunha.py` e
`relacional.py` passaram a registrar a recusa na compilação em vez de abortar.

## 6. Impedimento: a agregação pura

Fechar a garantia também para a agregação exigiria recusar `join(notes.issue, ", #")` na triagem
RC8–RC10. É um resumo composto pelo programa a partir das notas enviadas, de propósito, porque o
modelo não sabe quais notas a política deixou de fora. Hoje esse resumo não tem caminho autorizado
na linguagem além de ser aceito sem garantia: propô-lo como variante do plano o devolve ao modelo,
que é o que o desenho evita. O caminho seria algo como `authorize`, fora deste ciclo.

| opção | consequência |
| --- | --- |
| A. manter a agregação fora da garantia (estado atual) | a garantia vale como escrita na seção 1; m10 mostra o que passa; nenhum programa histórico muda |
| B. recusar a agregação | quebra a triagem RC8–RC10 (exemplos, testes `rc7`–`rc9`, CI) e deixa sem caminho um resumo legítimo até existir `authorize` |

Não se declara a garantia completa sobre agregação. A decisão é do Antonio.

## 7. Questões separadas, não implementadas

Registradas para decisão própria; nenhuma faz parte da E0453:

- **`authorize`:** composição feita pelo programa, decidida pela política depois de composta;
- **consumo linear:** "no máximo uma vez por operação aprovada";
- **deduplicação automática:** o P1 deduplica, a Aletheia não (caso "duplicada" acima);
- **operações idênticas:** duas operações iguais aprovadas são duas autorizações ou uma?
- **idempotência:** repetir a chamada da mesma operação depois de uma falha;
- **efeitos parciais:** a operação aprovada executada pela metade;
- **recuperação depois de falhas:** o que o journal deve provar sobre as operações que já executaram.
