# Checkpoint de 01/12/2026 (escrito em 27/11)

Gasto adicional com API neste pacote: **US$ 0,00**. O acumulado continua em US$ 1,1504. A trava dos
jobs pagos ([bench/orcamento/AUTORIZACAO.json](../bench/orcamento/AUTORIZACAO.json)) continua em
zero. Nenhum merge foi feito. A decisão final continua marcada para 08/12.

## 1. B1 e B2: reproduzidos, corrigidos e revisados

As reproduções e os controles foram versionados antes de cada reparo, em
[bench/obrigacoes/casos](../bench/obrigacoes/casos). O comportamento de cada etapa está em:

- [antes.json](../bench/obrigacoes/antes.json): compilador de `e79c04b`;
- [depois_b1b2.json](../bench/obrigacoes/depois_b1b2.json): depois do reparo de B1 e B2;
- [depois_f123_v1.json](../bench/obrigacoes/depois_f123_v1.json): depois do reparo de F1–F3;
- [depois.json](../bench/obrigacoes/depois.json): estado final.

As regressões, cada uma conferida para falhar sem o reparo, estão em
[crates/athc/tests/ciclo1_obrigacoes.rs](../crates/athc/tests/ciclo1_obrigacoes.rs). A análise
afirma "descarregada" quando a obrigação do `requires` foi levantada para o `refine` e conferida
sobre a proposta. Nesse caso, uma falha no efeito é defeito da análise (R0631). Quando a análise
afirma "dinâmica", só o monitor da fronteira do efeito confere (R0630).

**Os monitores dos dois runtimes não foram tocados:** ARX `call_tool_live` e Python `call_tool`. Todo
efeito com `requires` continua conferido antes da ferramenta, nos dois runtimes, qualquer que seja a
afirmação estática.

| reprodução | o compilador afirmava | passa a afirmar | chamadas que chegaram à ferramenta | efeito legítimo antes da recusa? |
| --- | --- | --- | --- | --- |
| B1: destinatário e memo de elementos diferentes | descarregada (falso) → R0631 | dinâmica → R0630 | `pay(alice, 100)`, antes e depois | **sim**: o primeiro par é o mesmo elemento e satisfaz o contrato; a 2ª chamada é recusada antes da ferramenta |
| B2: `as fora` sombreia uma lista endossada | descarregada (falso) → R0631 | dinâmica → R0630 | nenhuma | não |
| B2: o recusado satisfaz o `requires` | descarregada (falso; o efeito passou porque o contrato valia) | dinâmica, conferida no efeito | `pay(alice, outro)`, antes e depois | não há recusa: o programa escolheu usar um recusado, e o contrato vale |
| F1: `requires` com `if`, argumentos literais | descarregada sem `refine` (falso) → R0631 | dinâmica → R0630 | nenhuma | não |
| F1: parâmetro lido só dentro de `if` | descarregada com ligação incompleta; ARX R0100 e Python passava `None` (**divergência**) | descarregada com os dois parâmetros; o plano inválido é recusado no `refine` | plano válido: `pay(alice, ok)`; plano inválido: nenhuma | não |
| F2: um campo de cada proposta, por `if` | descarregada (falso) → R0631 | dinâmica → R0630 | nenhuma | não |
| G1: um parâmetro ligado a dois campos | descarregada (falso) → R0631 | dinâmica → R0630 | nenhuma | não |
| H1: `requires` que só lê a fotografia `s` (bandeira fechada) | descarregada sem `refine` (falso) → R0631 | dinâmica → R0630 | nenhuma | não |

Controles com origem comum demonstrável, que continuam descarregados e recusam o plano inválido no
`refine`, sem efeito nenhum:

- mesmo elemento;
- `let x = o`;
- laço interno sem uso;
- `keeping … as` sem sombra;
- `let o = if c { a } else { b }`;
- `for x in [a, b]`;
- `refine … else { a }`;
- campos por `match` de um braço.

**Revisão independente.** Três rodadas, sem custo de API:

- A 1ª ([REVISAO-B1B2.md](../bench/obrigacoes/REVISAO-B1B2.md)) confirmou B1 e B2 e achou F1, F2
  (ambos anteriores ao ciclo) e F3, excesso de cautela introduzido pelo reparo de B1.
- A 2ª ([REVISAO-F123.md](../bench/obrigacoes/REVISAO-F123.md)) achou G1, falso "descarregada": a
  maior parte já existia, e uma variante foi aberta pelo reparo de F2. Achou também G2 e G3, excesso
  de cautela. O ajuste proposto pela 2ª revisão foi aplicado com testes.
- A 3ª ([REVISAO-FINAL.md](../bench/obrigacoes/REVISAO-FINAL.md)) testou cerca de 50 ataques à mão e
  280 programas aleatórios (3.216 execuções): nenhum R0631, nenhuma divergência. No compilador
  anterior ao reparo, os 25 primeiros programas aleatórios dão 24 R0631. A revisão achou H1, uma
  obrigação que não lê parâmetro e era declarada descarregada (anterior ao ciclo); o ajuste de 5
  linhas foi aplicado com teste. Achou também, ambos anteriores ao ciclo e **não corrigidos**:
  - H2: a obrigação levantada vale para todo elemento, mesmo quando a chamada está atrás de um `if`
    sobre o elemento. Um plano válido pode ser recusado no `refine`. Não é falha de segurança;
  - H3: casos conservadores sem necessidade, como `o.to`, registro inteiro como argumento e o mesmo
    campo em variantes diferentes.

**Desvio declarado.** A autorização cobria B1 e B2. Corrigi também F1, F2 e G1, que são o mesmo
defeito: a análise afirmava uma obrigação que não estava demonstrada. As revisões exigidas pela
diretiva os acharam, e deixá-los abertos tornaria falsa a afirmação sobre o alcance estático.
H1 é do mesmo defeito, e foi corrigido pelo mesmo motivo. F3, G2 e G3 foram corrigidos para cumprir "preserve as situações em que a origem comum é realmente
comprovável". Nenhuma construção nova entrou na linguagem, e nenhuma obrigação foi convertida em
dinâmica por atacado.

**Regressão.**

- [Varredura](../bench/obrigacoes/varredura.py) de todos os casos das reproduções e das duas
  primeiras revisões, com todos os mundos, roteiros e valores de `c` (2.412 execuções):
  - R0631: **258 → 0**;
  - divergência entre ARX e Python: **27 → 0**.
- Corpus versionado ([regressao_corpus.json](../bench/obrigacoes/regressao_corpus.json)): só
  mudaram os casos de reprodução e de revisão. Os programas reais (`examples/`, `bench/agentdojo`,
  `bench/m10`, `bench/m11`) dão as mesmas afirmações e, segundo a 2ª revisão, o mesmo Python
  emitido, byte a byte.
- Testes Rust: 326 passam.
- Passam também os testes Python do runtime, do AgentDojo, do M10 e do orçamento.

## 2. Alcance atual das garantias

| camada | o que garante | onde fica |
| --- | --- | --- |
| estática (`athc check`) | efeito sem capacidade (E0502); dado não confiável como argumento de efeito ou decidindo a ocorrência de efeito irrevogável, inclusive por `return` antecipado (E0412, D15); segredo em saída pública, inclusive por ramo (E0440, D16); prompt sem o inventário em que a política decide (E0421); política por literal (E0422); operação sem regra ou sem executor (E0012); limites do plano × contrato (E0450); aritmética sem limite em política (E0451); orçamento de pior caso (B0201) | compilador |
| obrigação demonstrada | contrato `requires` levantado para o `refine` quando todos os argumentos que `φ` lê vêm, sem transformação, de campos do mesmo valor endossado; a proposta que viola é recusada antes de qualquer efeito | análise de obrigações (corrigida neste pacote) |
| barreira em execução | todo `requires` conferido na fronteira do efeito (R0630), nos dois runtimes; decodificação tipada; política no `refine`; journal encadeado | runtimes |
| estática parcial, desde 28/11 | a E0453 recusa as misturas de operações aprovadas que detecta (X2). **Garantia não estabelecida:** 5 fugas conhecidas depois de três revisões; implementação interrompida ([e0453/RELATORIO.md](../bench/agentdojo/x2/e0453/RELATORIO.md), seções 0, 8 e 9) | compilador |
| **não coberto** | lógica errada da política (o erro mais comum do catálogo cego do M4); campos transformados de uma só operação e agregação pura sobre a lista inteira (fora da E0453); acesso por ponto `o.campo` sempre tratado como dinâmico (conservador); faixa numérica vinda de linha confiável (limitação) | — |

Premissa da análise, registrada pela 2ª revisão: não há funções nem estado mutável, e um `for` não
devolve valor. Por isso cada ponto de escolha produz um valor por escopo. Uma construção futura com
closures, mutação ou acumuladores obriga a rever a regra do último ponto de escolha.

## 3. O que a extensão acrescentou ao M4

A reconciliação com o que o M4 já tinha medido está em
[RECONCILIACAO.md](../bench/agentdojo/defects/ext/RECONCILIACAO.md).

**Regressão do M4**, com o compilador de hoje: o catálogo do autor (21 + 4 pós-auditoria), o
catálogo cego (15) e o Pysa no catálogo do autor saíram **idênticos** ao registro de 17/11 em
estágio, códigos e efeito com o modelo obediente. O Pysa no catálogo cego está registrado em
[runs/m4-regressao](../bench/agentdojo/runs/m4-regressao/README.md).

O banco de contratos da Rev12 (`banking_contracts.ath`) não serve mais para caracterizar
"obrigação demonstrada × barrada em execução". Ele deixou de compilar quando o E0421 entrou, e o
compilador congelado também o recusa. Corrigi-lo seria alterar um artefato histórico. Essa
caracterização fica só com os casos de [bench/obrigacoes](../bench/obrigacoes), que comprovam o
reparo e não são comparação.

**Extensão M4-ext.** O pré-registro ([PROTOCOLO.md](../bench/agentdojo/defects/ext/PROTOCOLO.md)) foi
versionado antes da execução. A base é a do M4, uma missão só. Os casos não são cegos.

| caso | Aletheia | P1 (ruff + mypy, testes) | Pysa | efeito em execução (304 execuções do modelo obediente por lado) |
| --- | --- | --- | --- | --- |
| X1: dado de ferramenta decide, por `return` antecipado, se os efeitos irrevogáveis acontecem | **estático** (E0412: o diagnóstico aponta o `if` mutado e o efeito; auditado) | não detectado | não detectado | P1: sem recusa e sem diferença da base nas 304. O marcador não aparece nos dados da suíte: defeito latente |
| X2: transferência com o destinatário de uma operação aprovada e o valor de outra | **não detectado** | não detectado | não detectado | nos dois lados, sem recusa e sem diferença da base: nenhuma tarefa da suíte aprova dois `Send` juntos. Defeito latente nos dois |

O que isso acrescenta:

- **X1** confirma, num mecanismo posterior ao M4, o padrão do M13. Fluxo implícito que decide a
  ocorrência de um efeito é pego pela Aletheia na compilação e passa pelo P1 e pelo Pysa.
- **X2** mostra uma lacuna da Aletheia com o mesmo peso. O endosso é por elemento, não pela relação
  entre os campos de elementos diferentes. Sem um `requires` na ferramenta, nenhuma camada pega a
  mistura. Com `requires`, depois deste pacote, a análise não afirma mais que ela está demonstrada,
  e o monitor do efeito a recusa (caso B1).
- Nenhum dos dois casos produziu efeito observável na suíte. O que se mediu foi detecção, não dano.

## 4. Resultados anteriores, preservados

| medição | método | resultado |
| --- | --- | --- |
| M3 (17/11) | um agente implementa 6 mudanças nos dois lados, com versão incompleta de cada | Aletheia 90 linhas × P1 68 (+32%, acima da margem de 25%): eixo II refutado nesta suíte |
| M4 (17/11) | defeitos semeados, catálogo do autor e catálogo cego; Pysa | autor: 16/17 × 0/17 na compilação (1/21 com o Pysa); cego, exprimível nos dois: 1 × 0 em 7; maquinaria de segurança: 7 casos inexprimíveis no programa Aletheia e 2 não detectados no P1 |
| M10 (20/11) | autoria por modelo, 8 especificações cegas × 3 repetições, avaliação com modelo real | **não superior**: A1 2/24 × 10/24 (p = 0,0215, a favor do Python); final segura 16/24 × 23/24; tokens 30% menores na Aletheia |
| P3 (26/11) | pedidos de mudança cegos, implementadores separados, executor neutro | 78 × 50 linhas: critério **não atendido** |
| P3b (26/11) | pedidos cegos novos, versão e guias congelados antes | 83 × 102 (0,81): critério **atendido**; tentativas 16 × 6; D3 implementado no handler; D5 dependente do host |
| M11 (26/11) | autoria por modelo, guias v2 e pacotes novos | interrompido por orçamento após 32 de 48 autorias. Avaliação adversarial local: 1ª versão segura 0/16 × 11/16, final segura 6/16 × 13/16. **Utilidade com modelo real e critério principal não avaliados** |
| pós-M11 (26/11) | correções de sintaxe e diagnóstico, reanálise dos arquivos | diagnóstico: primeiras versões que compilam 0 → 1, finais 9 → 9. Não mede economia de tentativas nem de tokens |

As diferenças de método importam:

- M3, P3 e P3b medem manutenção por agentes implementadores; M10 e M11, autoria por modelo.
- P3 e P3b usam pedidos cegos; M3 usa os pedidos do autor.
- M10 tem avaliação com modelo real; M11 só tem a local.
- Os resultados de manutenção divergem entre si (M3 e P3 contra a Aletheia, P3b a favor), e nenhum
  substitui os de autoria.

## 5. Limitações que ainda impedem a consolidação

1. **Autoria por modelo:**
   - M10 não superior;
   - M11 com resultado local desfavorável, e sem utilidade nem critério principal avaliados;
   - a contribuição da familiaridade com Python não foi isolada da contribuição da linguagem, do
     guia e dos diagnósticos.
2. **Manutenção:** três medições, duas contra (M3, P3) e uma a favor (P3b).
3. **Lógica de política:** o erro mais frequente no catálogo cego não é pego por nenhum dos dois
   lados.
4. **Endosso por elemento:** campos de elementos diferentes numa ferramenta sem `requires` (X2).
   Desde 28/11 a E0453 recusa as misturas que detecta, sem garantia estabelecida (fugas conhecidas;
   redesenho pendente de decisão); continuam abertas a agregação pura e as questões de quantidade
   (consumo linear, deduplicação, idempotência).

Pendências para a decisão, sem frente aberta neste pacote:

| pendência | problema concreto | alcance de uma solução |
| --- | --- | --- |
| sintaxe | os autores escreveram Rust dentro da Aletheia: `let` em posição de expressão, `then`, `!`. Depois da renomeação, 22 versões arquivadas pararam no erro seguinte | aceitar ou diagnosticar com correção exata as formas mais frequentes; medir sobre as 71 versões arquivadas, como diagnóstico; o efeito na autoria só se mede com nova autoria paga |
| faixa numérica | `CT\|campo\|mín\|máx` não é conferível na política (`contracts`, M11) | busca por chave composta com valor numérico limitado vindo do dado confiável, geral para tabelas; muda gramática e checker |
| relação entre campos | X2: endosso por elemento | E0453 implementada em 28/11 e interrompida: recusa parcial, garantia não estabelecida. Pendentes: manter, rebaixar a aviso ou redesenhar (origem no sistema de rótulos, proveniência no runtime); e a agregação pura |
| acesso por ponto | `o.campo` é sempre dinâmico na análise de obrigações | rastrear `Field` com informação de tipo; ganho de precisão, não de garantia |
| obrigação sob condição (H2) | a obrigação levantada vale para todo elemento, mesmo se a chamada está atrás de `if` sobre ele; um plano válido pode ser recusado no `refine` | levantar `cond ⇒ φ`, ou tratar como dinâmico o sítio guardado por condição sobre o elemento; precisão, não segurança |
| conjunto de caracteres | D5 (P3b) foi para o host | primitiva de texto "todo caractere pertence a um conjunto"; pequena |

A meta continua sendo uma linguagem independente melhor para construir agentes. O que está
demonstrado até aqui é uma vantagem em detecção estrutural, e ela não substitui autoria, utilidade e
manutenção.
