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

## Registro

- **26/11 — etapa 2 concluída.** 8 pacotes de um autor cego (só `FORMAT.md`, o pacote de exemplo sem
  agentes e o validador como ferramenta). Os 8 passam em `harness.py validate` e no teste de duplicação
  (nenhum efeito duplicado que mude o mundo é aceito). Revalidado por mim, fora da pasta do autor. Hash
  (sha256 da lista ordenada de `caminho sha256` dos 33 arquivos): `7cc2d62e045e344daff9de222637abdc8239e9955d55e6313b08f8abb29a3519`.
  Ressalvas do autor em `packs/AUTHOR_NOTES.md`. Os pacotes não foram alterados depois de recebidos.
- **26/11 — auditoria dos oráculos (antes de qualquer autoria).** Auditor separado, sem acesso a
  agentes de nenhuma das duas linguagens ([AUDIT.md](AUDIT.md)). 22 achados: 2 defeitos corrigidos, 20
  notas. `maintenance`: as verificações legítimas exigiam o id da ordem no texto da notificação, o que
  o pedido não pede (mensagem correta era reprovada); agora exigem exatamente uma notificação não vazia
  ao técnico certo. `sales_crm`: o oráculo de ataque não via uma cotação no alvo da injeção
  (OP-2204); agora vê, e nenhum cenário legítimo cota OP-2204. As duas correções valem igual para os
  dois lados e foram feitas antes de existir qualquer agente. Pacotes corrigidos em `packs_v2/`
  (originais intactos em `packs/`); revalidados por mim: 8 × `pacote válido`, duplicação: nenhuma.
  Hash (mesmo método, caminhos `packs_v2/…`, 33 arquivos):
  `8abff4a078266c8ae77120fc639e0413ad375620b524bf3a203aed12c0d4bcfe`. É esta versão que a autoria usa.
- **26/11 — etapa 3 interrompida pelo teto.** A autoria parou no teto de US$ 0,42 com **32 das 48**
  autorias feitas (repetições 1 e 2 dos 8 pacotes, nas duas linguagens); a repetição 3 não foi gerada.
  Gasto da autoria: US$ 0,4210. O teto é conferido depois de cada autoria, por isso passou US$ 0,001.
  Gasto do ciclo: US$ 1,1504 de 1,50. Ainda não houve avaliação. Fechar a repetição 3 custaria
  ~US$ 0,21, e somado à avaliação (≤ 0,18) o ciclo passaria de US$ 1,50. A escolha entre avaliar só as
  2 repetições (desvio declarado: 2 em vez de 3) e pedir autorização para a terceira é feita **antes**
  de ver qualquer resultado, para que olhar os números não decida quando parar.
- **26/11 — orçamento encerrado; avaliação local.** O Antonio recusou as duas opções (avaliar com o
  modelo real e fazer a 3ª repetição) e fixou em US$ 0,00 a autorização de gasto adicional. Não existe
  decisão automática por falta de resposta. Avaliação adversarial local das 32 autorias, sem
  credenciais, gasto US$ 0,00: [AVALIACAO-LOCAL.md](AVALIACAO-LOCAL.md). Registro:
  **M11 interrompido por orçamento após 32 de 48 autorias. Avaliação adversarial local realizada;
  utilidade com modelo real e critério principal de superioridade não avaliados.** A1 e A4 ficam sem
  resultado. A 3ª repetição ausente não é falha dos agentes. Nenhuma tarefa ou versão foi escolhida
  depois de ver resultados.
