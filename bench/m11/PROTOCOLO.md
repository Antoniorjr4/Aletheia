# M11 — autoria por modelo, com as capacidades pós-P3 (pré-registro)

Escrito em 26/11/2026, **antes** de as especificações existirem. Não muda depois de ver resultados;
desvios são registrados como desvios. O M10 (NÃO SUPERIOR, `bench/m10/PROTOCOLO.md`) fica como está.

## Pergunta

Com o compilador e o guia congelados nesta data, um modelo produz agentes corretos e seguros com mais
frequência em Aletheia do que em Python endurecido? É a pergunta 2 da diretiva de 26/11 (autoria). A
pergunta 3 (manutenção) é medida à parte, pelo P3b.

## O que fica igual ao M10

- Executor, mundo comum, modelo adversarial e avaliador: o código congelado do M10
  (`bench/m10/harness.py`, `author.py`, `evaluate_all.py`), chamado por `bench/m11/run.py`, que só troca
  os caminhos de pacotes, guias e saídas.
- Modelo autor: Groq `openai/gpt-oss-120b`, mesmos parâmetros, até 5 ciclos, 3 repetições,
  8 especificações × 2 linguagens = 48 autorias.
- Ataques: 30 execuções do modelo adversarial por cenário (viola se qualquer uma violar), sem custo.
  Legítimos: uma execução com o modelo real.
- Métricas A1–A7 e **critério de decisão** da seção 6 do M10, sem mudança: "melhor que Python" exige
  A1 maior com McNemar pareado (α = 0,05), A3 não pior e A5 no máximo 25% maior.
- Resultados por especificação e por repetição. Repetições da mesma especificação não são problemas
  independentes; o McNemar pareia especificação × repetição, como no M10, e o relatório traz também a
  contagem por especificação (quantas das 8 a Aletheia vence, empata, perde).

## O que muda, e por quê

1. **Compilador:** o do commit congelado (registrado em `FREEZE.json` antes da etapa 3), com as
   capacidades pós-P3 e as correções da revisão independente.
2. **Guias v2** (`bench/m11/guides/`), escritos antes das especificações, revisados por um agente
   independente quanto à paridade de tópicos, tamanho (Aletheia 11,5 mil caracteres, Python 10,7 mil) e
   ausência de nomes de domínio; o exemplo completo é o mesmo nos dois e executa sem violação contra o
   pacote de exemplo.
3. **Domínios novos**, disjuntos dos do M10 e do exemplo dos guias: manutenção industrial (ordens de
   serviço), almoxarifado (requisição de peças), despesas de viagem, controle de acesso a sistemas,
   expedição e logística, CRM de vendas, contratos e assinaturas, frota de veículos.
4. **Auditoria do oráculo ampliada:** além de `harness.py validate`, cada pacote passa em
   `bench/m10/dup_oracle.py` (nenhum cenário legítimo pode aceitar um efeito duplicado que mude o mundo)
   e numa revisão independente que confere o oráculo contra a especificação, com tratamento simétrico
   das duas linguagens. Os pacotes originais ficam em `packs/`; correções, se houver, em `packs_v2/`,
   com cada mudança justificada.

## Orçamento

**O teto de US$ 0,60 inclui autoria e avaliação:** autoria até US$ 0,42, avaliação (cenários legítimos
com o modelo real) até US$ 0,18. Gasto anterior do ciclo: US$ 0,7294; o ciclo fica em no máximo
US$ 1,33, abaixo do teto de US$ 1,50. Se a autoria parar no teto antes das 48 autorias, a medição para e
o dono do projeto é avisado; nenhum teto sobe sem autorização expressa.

## Ordem

1. Congelamento do compilador e dos guias (commit e hashes em `FREEZE.json`).
2. Especificações e oráculos por um agente que não viu o repositório, as linguagens, os guias nem os
   resultados (só `bench/m10/FORMAT.md`, o pacote de exemplo sem agentes de referência e o validador
   como ferramenta). Hash dos pacotes registrado antes da etapa 3.
3. Auditoria do oráculo (seção "O que muda", item 4).
4. Autoria (48) e avaliação.
5. Relatório com o critério acima, o resultado por especificação, os limites e a comparação com o M10.

## Limites conhecidos antes de medir

- Os guias v2 foram escritos por quem conhece os resultados do M10 e do P3 (eu), embora antes das
  especificações do M11 e sem nomes de domínio. O guia de Python recebeu os mesmos tópicos.
- Autor e adversário são do mesmo modelo nas duas linguagens; o modelo conhece Python do treino e a
  Aletheia só pelo guia.
- Oito especificações escritas por um único autor.
