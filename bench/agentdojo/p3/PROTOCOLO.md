# P3 — esforço de mudança com pedidos cegos (critério 5 do Ciclo 1)

Pré-registro, escrito em 26/11/2026 **antes** de os pedidos existirem.

## Pergunta

Mudar o agente bancário custa, em Aletheia, no máximo 25% mais linhas alteradas que no Python
endurecido (P1)? Busca-se menos. A margem é a de `docs/CICLO-1.md`, fixada em 17/11.

## Procedimento

1. **Autor cego.** Um agente que recebe apenas [`FUNCIONAL.md`](FUNCIONAL.md) escreve 6 pedidos de
   mudança realistas de manutenção, cada um com 3 a 5 casos de aceitação no formato do documento e
   com uma "omissão plausível" (o passo que um implementador apressado esqueceria), descrita em
   termos funcionais. Não vê programas, linguagem, correções do ciclo nem resultados.
2. **Congelamento.** `pedidos.json` recebe hash SHA-256 e é versionado antes de qualquer implementação.
   Um pedido só é descartado se for impossível nas ferramentas do banco; o descarte é declarado.
3. **Implementadores separados.** Um agente implementa em Aletheia (`banking_v3_keeping.ath` +
   `aletheia_pipeline.py`), outro em Python (`hardened/banking_hardened.py` +
   `hardened/agentdojo_adapter.py`). Cada um trabalha numa cópia sem os arquivos do outro lado. Cada
   pedido parte da mesma base (`cbfe4d1`), não acumula.
4. **Pronto** = casos de aceitação do pedido passam em [`aceite.py`](aceite.py) **e** os testes
   existentes do lado continuam passando (ajuste de teste existente só quando o pedido muda o
   comportamento testado; contado à parte).
5. **Versão incompleta.** Cada implementador entrega também a versão com a omissão do pedido. Eu
   verifico, aplicando o diff, em que nível ela é detectada: `estático` (Aletheia: `athc check`;
   Python: `py_compile` + `pyflakes`), `testes existentes`, `casos de aceitação` ou `não detectado`.

## Métricas

- **Primária:** linhas alteradas (adicionadas + removidas, `git diff --numstat`) nos arquivos de
  produção de cada lado, somadas nos 6 pedidos. Critério: Aletheia ≤ 1,25 × P1.
- Secundárias: linhas sem comentário nem branco; arquivos tocados; execuções de teste até passar
  (relato do implementador); defeitos introduzidos (casos de aceitação ou testes que falham na entrega);
  nível de detecção da versão incompleta.

## Limites conhecidos antes de medir

Uma suíte, 6 pedidos, um implementador por lado, ambos o mesmo modelo de linguagem, que conhece Python
do treino e a Aletheia só pelo código da base e por `bench/m10/guides/aletheia.md` mais uma nota das
construções pós-M10. O isolamento dos agentes é por instrução e por cópia de trabalho, não por
sandbox. O P1 guarda valores como texto decimal; a Aletheia v3, como centavos inteiros (diferença
pré-existente, resultado do D10).

O resultado é registrado como sair. O critério não é reescrito depois.

## Registro (26/11/2026)

- Pedidos congelados: `pedidos.json`, sha256 `d6d9b69d77532baa8b2d4b7575249d8dc756a07ba2776634ff42c91637ca7f42`.
  Nenhum descartado.
- Base das cópias de trabalho: `09ed919` (este branch), reduzida aos arquivos de cada lado.
  **Desvio:** o pré-registro citava `cbfe4d1`; a base real inclui os dois commits de P3 sobre ele, sem
  mudança em arquivos de produção.
- **Desvio:** o teste de paridade de inventários do P1 com o host da Aletheia foi removido da cópia
  do lado P (exigiria ver o outro lado). Favorece o P1.
- **Correção do executor depois das entregas:** `aceite.py` descartava chamadas de escrita que o banco
  devolveu com erro; passou a observar tudo o que chega ao `FunctionsRuntime` oficial. Os dois
  implementadores apontaram a falha de forma independente. As entregas foram reconferidas com a
  versão corrigida; a contagem de linhas não depende do executor.

## Resultado (26/11/2026)

Conferido por mim, aplicando cada diff sobre a base ([`runs/p3/verificacao.json`](../runs/p3/verificacao.json),
script em `runs/p3/verifica.py`). As 12 entregas completas aplicam, passam nos casos de aceitação com
o executor corrigido (27/27 de cada lado) e nos testes do lado.

**Critério primário: NÃO ATENDIDO.** Aletheia 78 linhas, P1 50: **+56%**, acima da margem de 25%.

| pedido | Aletheia (+/−) | P1 (+/−) | tentativas A / P | onde a regra ficou, na Aletheia |
| --- | --- | --- | --- | --- |
| C1 operação `Reschedule` | 6/0 | 11/3 | 1 / 1 | política |
| C2 força da senha | 8/2 | 3/1 | 2 / 1 | tamanho na política; letra+dígito em inventário do host |
| C3 teto corrido de 500,00 | 20/12 | 17/5 | 8 / 1 | teto por operação na política; **total corrido no host** |
| C4 lista `request_ibans` | 4/1 | 3/1 | 1 / 1 | política + inventário novo |
| C5 tudo ou nada | 13/2 | 2/1 | 6 / 1 | **host bloqueia as escritas** |
| C6 sem links no assunto | 8/2 | 2/1 | 1 / 1 | política (`links_within`) + **resguardo no host** |
| **total** | **78** | **50** | **19 / 6** | |

Secundárias: sem comentário nem branco, 68 × 45 (+51%); linhas de teste editadas, 10 × 8; arquivos de
produção tocados por pedido, Aletheia 1–2, P1 sempre 1.

Versão incompleta (mesma omissão dos dois lados): nenhuma das 12 é pega estaticamente, em nenhum dos
lados. C5 é pega pelos testes do lado nos dois; as outras cinco, só pelos casos de aceitação, nos
dois. **Sem vantagem de detecção para a Aletheia neste conjunto.**

### Leitura

O resultado é pior que o M3 (+32%) e é contra a Aletheia. Onde o pedido cabe na política (C1, C4) ela
empata ou vence. Onde não cabe, o implementador teve de levar a decisão para o host, que é
justamente o que a linguagem promete evitar (critério 4 da régua):

- **C3:** não há dobra dependente de ordem; `keeping` julga cada item sem estado, e `sum` só dá o total.
- **C5:** trocar execução parcial por tudo-ou-nada mantendo a contagem não é expressável: `refine` sem
  `keeping` não informa quantas operações foram recusadas, e o compilador recusa (E0412) condicionar
  um `commit` a `len(fora)`.
- **C2, C6:** a política não testa conteúdo de texto (classe de caractere, subcadeia sem distinção de
  caixa); `links_within` cobre links, não `www.` colado a outra palavra.

Esses três pontos viram os próximos itens de linguagem: predicados de texto em política, `keeping`
com acumulador, e contagem das recusas disponível à decisão de executar.

Limites: os de antes (um implementador por lado, mesmo modelo, 6 pedidos, uma suíte). O critério não
é reescrito.
