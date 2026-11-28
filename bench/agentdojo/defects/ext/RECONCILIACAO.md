# Mutação × M4: o que já estava medido e o que falta medir (27/11/2026)

Escrito com a regressão já em andamento (só três casos dela tinham saído, iguais ao M4) e antes de
qualquer resultado da extensão.

## O que o M4 já mediu (17/11, base `banking_v2_keeping.ath` × P1 endurecido)

| registro | casos | Aletheia | P1 | onde |
| --- | --- | --- | --- | --- |
| catálogo do autor: garantias | 17 (4 corrigidos pós-auditoria) | estático 16, testes 1 | estático 0, testes 9, não detectado 8 | [m4.md](../../runs/m4/m4.md), [m4-posthoc.md](../../runs/m4/m4-posthoc.md) |
| catálogo do autor: semânticos (controle) | 3 | testes 2, não detectado 1 | testes 2, não detectado 1 | idem |
| catálogo do autor: assimétrico (orçamento) | 1 | estático | não detectado | idem |
| catálogo cego, exprimível nos dois | 7 | estático 1, testes 3, não detectado 3 | testes 4, não detectado 3 | [blind.json](../../runs/m4/blind.json) |
| catálogo cego, maquinaria de segurança | 7 | inexprimível no programa | testes 5, não detectado 2 | idem |
| Pysa no P1 | 21 + 15 | — | 1/21, 0/15 | [pysa.json](../../runs/m4/pysa.json), [pysa-blind.json](../../runs/m4/pysa-blind.json) |

Os estágios foram estático, testes congelados e não detectado. O efeito com o modelo obediente ficou
registrado à parte, em agregado: ataques, utilidade, operações executadas e abortos.

## O que mudou depois de 17/11 e se exprime nessa base

| mecanismo (data) | o que pega | exprimível na base do M4? |
| --- | --- | --- |
| D15 com piso de saída (26/11) | efeito irrevogável cuja ocorrência é decidida por dado não confiável, inclusive por `return` antecipado | **sim**: caso X1 |
| D16 (26/11) | segredo que escapa por ramo, `return` ou resultado de política | não: a base não tem dado `Secret` |
| E0451 (26/11) | aritmética sem limite provado em política | não: as políticas da base não fazem aritmética |
| `keeping` com aceitos, `keeping all`, E0437 (26/11) | forma da política ordenada | não como defeito estrutural: são construções novas, e um erro nelas é de regra da missão (medido no P3b) |
| renomeação em padrão, E0422, palavra reservada (26/11) | sintaxe e texto de diagnóstico | não é classe de defeito |
| B1 e B2 (27/11) | afirmação falsa de obrigação descarregada | **parcialmente**: a base não tem `requires`. A classe do B1 ("argumentos de elementos diferentes") se exprime nos dois lados como erro do executor, e é o caso X2 |

## O que ainda não estava medido

1. **X1 e X2**, acima: duas classes posteriores ao M4 que se exprimem nos dois lados da mesma base.
2. **A classificação do efeito por execução.** O M4 só deu o agregado. A diretiva pede quatro
   categorias para quem executa: efeito proibido executado; recusa antes de qualquer efeito; recusa
   depois de efeitos legítimos; sem recusa.
3. **Obrigação comprovada × obrigação só barrada em execução.** Não há base pareada com
   contratos `requires` dos dois lados: o P1 não tem contratos, e escrever um agora, conhecendo os
   casos, seria uma base nova feita sob medida. O lado Aletheia já tem um banco só dele,
   [bench/adversarial](../../../adversarial/results/CONTRACTS.md) (Rev12, 42 casos, de antes do
   levantamento de obrigações). Reexecutar esse banco hoje mostra em que estrato cada recusa
   acontece: no `refine`, com a obrigação demonstrada, ou na fronteira do efeito. Isso é
   **caracterização da Aletheia**, não comparação, e fica registrado assim.

## O que é regressão

Reexecutar o catálogo do autor, as correções pós-auditoria, o catálogo cego e o Pysa com o
compilador atual (reparo de B1 e B2 incluído) confere que nada regrediu. Não é medição nova.
