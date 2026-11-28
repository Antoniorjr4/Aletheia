# E0453: mistura de operações aprovadas (28/11/2026)

Implementação autorizada pela decisão arquitetural X2 / E0453 de 28/11. Não houve merge e não houve
gasto com API (orçamento adicional US$ 0,00). Nenhum resultado histórico foi recalculado:
`../resultado.json`, `../relacional/resultado.json`, `bench/obrigacoes/*.json` e as métricas de autoria
ficam como estavam. O que é de depois da E0453 fica nesta pasta.

## 0. Situação: aviso experimental (decisão final de 28/11)

A E0453 é um **aviso experimental**: aponta chamadas que parecem combinar dados de origens aprovadas
distintas e **não recusa** o programa. A garantia descrita na seção 1 **não está estabelecida**: três
revisões independentes acharam, cada uma, uma classe nova de fuga (seção 8), e restam cinco fugas
conhecidas, que compilam sem aviso ([fugas_conhecidas.json](fugas_conhecidas.json)). A ausência do
aviso não certifica nada. O texto do aviso diz isso. O redesenho (seção 9, opção C) não foi
autorizado neste ciclo; a agregação pura continua fora da cobertura. As seções 1 a 9 registram a
implementação como recusa, antes do rebaixamento; a seção 10 registra o estado final.

## 1. A garantia pretendida, classificada

**O que se pretende (estático, `athc check`, estrato E3), ainda não sustentado (seção 0):** uma chamada de ferramenta aceita não combina dados
de duas ou mais origens aprovadas distintas. Uma origem é um valor endossado por um `refine` (o
valor inteiro), um elemento de uma lista dele escolhido por um laço, ou a lista inteira (`[∀]`). Os
dados contam mesmo se passarem por variáveis, reconstrução de construtor, transformação, seleção de
campo (por padrão ou por ponto), junção de `if`/`match`/`else`/`atomic`, `reconcile`, lista literal
ou quantificador.

**O que não vale, e o diagnóstico diz:**

- **não é fidelidade nem identidade.** Os campos de uma só operação ainda podem ser transformados
  (`fit(memo ++ "!", 12)`), e a chamada recebe então um valor que a política não viu. Esses sítios
  são aceitos e classificados como "uma operação, transformada" (`athc operacoes`); não há prova de
  que sejam os aprovados;
- **a agregação pura sobre uma lista inteira** (`join(ops.memo, ",")`, `len(ops)`, `sum w in ops => …`
  sozinhos) é aceita. Ela depende de todos os elementos, isto é, de várias operações; a E0453 não a
  cobre. Ver a seção 6;
- **dependência de controle:** a regra é sobre dados. Um `if` cuja condição vem de outro elemento não
  é mistura para a E0453, nem quando decide a ocorrência da chamada (`if b.n > 3 { pay(a.to, a.memo) }`,
  sonda l2), nem quando escolhe o valor do argumento (`pay(a.to, if b.memo == "z" { "z" } else { a.memo })`,
  l1: o `"z"` sai copiado de `b` por fluxo implícito);
- **valor que passa por uma ferramenta:** o retorno de ferramenta não carrega a origem aprovada.
  `let r = echo(bank, b.memo)` e depois `pay(bank, a.to, r)` é aceito, inclusive com o parâmetro
  `memo: Text ^ Endorsed`, porque o retorno sem rótulo não é Untrusted;
- **quantidade:** duas chamadas com a mesma operação, ou uma operação aprovada que não é executada,
  não são assunto da E0453 (seção 7).

Texto do diagnóstico (o caminho autorizado citado existe hoje na linguagem):

```
error[E0453]: a chamada de `pay` combina dados de 2 origens aprovadas distintas
   |     for a in ops { match a { Pay { to } => for b in ops { …
   |         - origem: um elemento de `Plan.ops`
   |                                               - origem: um elemento de `Plan.ops`
   = a política aprovou cada origem; nenhuma aprovou esta combinação numa chamada
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
| m17–m20 | ramo de junção que já misturava (`if c { Pay { to: a.to, memo: b.memo } } else { a }`, escalar, sem ramo igual, `if` em `if`) | recusa |
| m21–m22 | `for` sobre lista literal cujo item já misturava | recusa |
| m23 | `refine … else { Pay { to: a.to, memo: b.memo } }` | recusa |
| m24–m25 | valor de `atomic` (corpo e `on failure`) | recusa |
| m26 | `p.ops` e `p.extra` por ponto, agregados | recusa |
| m27 | reconstrução do plano com a lista, dois laços | recusa |
| m10 | só agregados: `pay(fit(join(ops.to)), join(ops.memo))` | **aceita** (fora da garantia) |
| u1–u13 | mesmo elemento; literal e dado confiável; transformada; alias e `match` duplo; `if` com o mesmo elemento; lista literal de elementos inteiros; ponto; `else` aninhado agregado; `refine … else { o }`; `if` entre campo e campo transformado; `if` entre dois campos do mesmo elemento; `atomic` com o mesmo elemento; reconstrução do plano com a lista e um laço | aceitas |
| l1–l2 | dependência de controle (seção 1) | aceitas, fora da garantia |
| c1–c3 | legítimas recusadas: duas junções com a mesma condição (`if c { a.to } else { b.to }` e `if c { a.memo } else { b.memo }`), elemento com `len(ops)`, `for x in [a]` | recusa conservadora |

Os controles aceitos (u1–u13, l1–l2 e m10) rodam no ARX e no Python com as mesmas chamadas, conferidas uma
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

**Revisão independente (28/11).** Sobre o commit 4185d7f, a revisão achou fugas de verdade, que
compilavam e, no ARX, executavam a recombinação (`pay A,b`):

- a junção unificava um ramo que já misturava: o ponto de escolha era registrado em todas as fontes
  quando os ramos diferiam, e um ramo com {a, b} virava uma operação só (m17–m20, m23);
- o `for` sobre lista literal unificava um item que já misturava (m21, m22);
- `atomic` e `reconcile` devolviam valor sem origem (m24, m25);
- `p.ops` por ponto não era lista para a análise, e a agregação de duas listas passava como uma
  operação (m26).

Correções (patch do revisor, validado aqui, mais uma minha): a junção só registra o ponto de escolha
quando cada ramo tem no máximo uma operação; o laço sobre lista literal não unifica um item que
misturava; `atomic` é junção do corpo e dos tratadores, e `reconcile` devolve o bloco; `p.f` sobre
registro liga a lista pelo campo; a reconstrução preserva a lista (antes, `G { xs: ops }` seguido de
`for o in xs` recusava o mesmo elemento, u13). Todas as sondas da revisão que eram fuga passaram a
E0453; as aceitas legítimas continuaram aceitas.

**Segunda revisão (28/11), sobre as correções (ec02825).** Achou mais três fugas, que compilavam:

- quantificador com junção no corpo: `sum i in [1, 2] => if i == 1 { a.n } else { b.n }` soma
  `a.n + b.n`. O corpo é analisado uma vez, e o ponto de escolha dentro dele unificava `a` e `b`;
- `refine` dentro de quantificador: um `refine` só no texto, uma aprovação por iteração, somadas;
- duas listas aninhadas por ponto: `join(p.left.ops.to)` com `join(p.right.ops.memo)` viravam a
  mesma origem.

Correções (6786e30): um ponto de escolha dentro do corpo de um quantificador não unifica, porque cada
iteração pode tomar um ramo; um `refine` no corpo conta como várias aprovações, agregadas (sozinho,
fica na agregação pura, fora da garantia; com outra origem, é E0453); a lista agregada é identificada
pelo passo que a selecionou, pelos nomes dos campos, e `p.left.ops` coincide com
`Plan.left/Group.ops`. As sondas das duas revisões estão versionadas ([revisao1/](revisao1/),
[revisao2/](revisao2/)), com o veredito de cada uma em [revisoes_esperado.json](revisoes_esperado.json),
conferido em teste: 62 recusadas pela E0453, 20 aceitas (limites declarados, agregação pura,
controles legítimos), as demais recusadas por outros códigos (programas inválidos por outras regras).
Uma terceira revisão, sobre estas correções, fica na seção 8.

**Recusas conservadoras, declaradas.** c1–c3 acima, e mais dois casos de desenho: um elemento com um
campo da raiz do plano (m7: o "assunto comum a todos os pagamentos" vira mistura) e um elemento com a
contagem de uma lista de no máximo um elemento (`len(one)` com `List[Op, 0..1]`). A 2ª revisão
acrescentou duas: um campo que é lista em outro tipo, com o mesmo nome, faz o acesso por ponto
`a.memo` contar como lista (a análise não tem o tipo do receptor); e uma sublista do próprio
elemento (`join(a.subs.x)`) conta como origem distinta do elemento, embora a política tenha aprovado
o elemento inteiro. O caminho, em todos, é pôr o dado na própria operação, proposta pelo modelo e
decidida pela política.

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
- a regressão do corpus e as testemunhas foram refeitas depois das correções da revisão: as mesmas
  30 mudanças fora das sondas, os mesmos resultados de execução;
- **tripés gerados:** o gerador do núcleo não conhece a regra e, em 13 de 3000 sementes (núcleo 3,
  sagas 6, chaves 4), produz uma chamada com valores de dois `refine` (`t2(c, x1 <= x3)`;
  `if … { x1 } else { x2 - x1 }`). Esses saem do tripé com registro; qualquer outra recusa continua
  falha. Nas sementes padrão, só `core/99`.

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

## 8. Terceira revisão (28/11), sobre 6786e30

As correções da 2ª rodada resistiram (os ids do corpo do quantificador; o caminho da lista pelo passo
que a selecionou). A revisão achou três classes novas:

- **A1, nó não visitado:** `await` caía no caso genérico da análise, e uma ferramenta chamada na
  condição do `await` nem virava sítio: `await send(bank, a.to, b.memo)` misturava dois elementos e
  compilava. **Corrigido** (o percurso agora lista todos os nós de expressão, para que um nó novo
  obrigue uma decisão); fica recusado;
- **A2, construtor sem campo:** `let w = W { sub: left }` e depois `join(w.sub.xs.to)` com
  `join(left.sub.xs.memo)`: a reconstrução não guarda em que campo entrou cada fonte, e o caminho
  embrulhado coincide com o de outra lista real. **Não corrigido** (b16, b17);
- **A3, lista literal de listas:** `for ll in [[ops, left.xs]] { … sum l in ll => … }`: o laço
  sobre o literal trata o item como elemento e perde a lista; a soma sobre duas listas distintas vira
  "uma operação, transformada". **Não corrigido** (b01, b02, b13).

Falsos positivos novos, registrados: dois campos de um `refine` repetido num quantificador contam como
duas origens; `if c { left } else { right }` seguido de `join(x.xs.to)`; a mesma lista embrulhada
e direta; dois laços sobre uma lista de no máximo um elemento. As sondas estão em
[revisao3/](revisao3/), com os vereditos em `revisoes_esperado.json`.

Avaliação do revisor, que subscrevo: a análise não está estável. Ela codifica a origem em caminhos de
texto e posições, e cada forma da linguagem que move dados (junção, lista literal, quantificador,
construtor, `await`) precisou de um caso especial, achado só depois de uma fuga.

## 9. O impedimento e as opções

Para sustentar a garantia, a origem precisa ser parte do que o compilador já calcula com rigor, e
não uma análise lateral sobre a AST: a origem de cada valor ligada ao tipo (como os rótulos de
integridade, com junção definida em cada forma da linguagem), construtores sensíveis ao campo, e uma
prova ou um teste diferencial em que o runtime registre a proveniência real de cada argumento de
ferramenta e a compare com o veredito estático sobre programas gerados. É uma ampliação
arquitetural: mexe no checker de rótulos, no runtime e no gerador do tripé.

| opção | o que fica | custo |
| --- | --- | --- |
| A. manter a E0453 como erro, com as fugas conhecidas registradas | recusa as misturas que detecta (as 30 do corpus, a X2, as sondas de mistura das quatro rodadas); nenhuma garantia declarada; documentação diz "verificação parcial" | as recusas conservadoras declaradas; risco de alguém ler a E0453 como garantia |
| B. rebaixar a aviso até o redesenho | nada muda na aceitação; o diagnóstico continua visível | a X2 volta a compilar |
| C. autorizar o redesenho (origem no sistema de rótulos + proveniência no runtime) | uma garantia que se pode afirmar | nova frente de arquitetura, fora deste ciclo |

O estado atual do ramo é A, sem merge. A decisão é do Antonio.

## 10. Estado final: o alcance efetivo depois do rebaixamento

- **O que a E0453 faz:** em `athc check`, imprime `aviso[E0453]` (na saída de erro, como nos demais
  comandos) e uma linha no resumo; a compilação segue e o programa roda. O aviso diz que a análise é
  parcial, tem fugas conhecidas e não certifica a integridade da operação;
- **onde avisa:** as 26 sondas `m*` de mistura (menos `m10`), as recusas conservadoras `c1`–`c3`, as
  sondas das revisões que misturam, e os 30 programas do corpus construídos para misturar
  (`bench/obrigacoes`, `x2/relacional/misturado`, `x2/composicao/c3`). Nenhum exemplo, nenhuma
  autoria do M10/M11 e nenhum programa de benchmark recebe o aviso;
- **onde não avisa e mistura:** as 5 fugas conhecidas; o valor que passa por uma ferramenta (t3 em
  `bench/ferramentas`); a dependência de controle (`l1`, `l2`); a agregação pura;
- **efeito na execução:** nenhum. A mutação X2 voltou a executar a recombinação, nos dois runtimes,
  igual ao comportamento anterior à E0453 ([testemunha_aviso.json](testemunha_aviso.json) é idêntica
  a [testemunha_antes.json](testemunha_antes.json)); o exemplo relacional é idêntico ao de 27/11
  ([relacional_aviso.json](relacional_aviso.json)). A sonda `m1` executa as quatro combinações nos
  dois runtimes (teste `uma_mistura_avisada_executa_igual_nos_dois_runtimes`);
- **regressão:** 478 programas versionados, nenhum muda de veredito em relação ao compilador anterior
  à E0453 (2950dc5) ([corpus.json](corpus.json)). Os 13 programas gerados que tinham saído do tripé
  voltaram e concordam no Lean, no ARX e no Python (núcleo com 1000 sementes, sagas com 700, chaves
  com 900). B1 e F2 voltaram a rodar nos dois runtimes, com o aviso.

Os resultados intermediários do período em que a E0453 recusava (`testemunha_depois.json`,
`relacional_depois.json`) ficam como registro.
