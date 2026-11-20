# M10 — autoria por modelo (pré-registro)

**Pergunta.** Quando alguém pede "crie um agente para mim", um modelo produz agentes corretos e
seguros com mais frequência e menos esforço escrevendo em Aletheia do que em Python endurecido?

Este documento é fixado **antes** de qualquer especificação, guia ou execução, e não muda depois de
ver resultados. Desvios são registrados como desvios.

## 1. Especificações cegas

- **Quantidade:** 8 especificações de agente, cada uma com ferramentas, dados não confiáveis e
  pelo menos um efeito irrevogável.
- **Autoria:** um agente que **não conhece a Aletheia nem o P1**, sem acesso ao repositório. Ele
  recebe só uma lista de domínios ("pagamentos", "suporte por e-mail", "agenda", "repositório de
  código", "atendimento com reembolso", "triagem de incidentes", "compras com teto", "publicação em
  site") e escreve, em linguagem natural, para cada uma:
  - o que o agente faz;
  - as ferramentas, com assinatura, efeito e se é reversível;
  - quais dados vêm de fontes não confiáveis;
  - as regras de segurança, em português corrente ("nunca pagar a quem o usuário não autorizou").
- **Oráculo:** o mesmo autor escreve, para cada especificação, um conjunto de cenários de teste:
  - tarefas legítimas, com o efeito esperado;
  - ataques, com o efeito proibido.

  Os cenários são executados contra um mundo simulado comum, independente das duas linguagens
  (como o `ath-world`).
- As especificações e os cenários são versionados (hash) antes da etapa 3.

## 2. Guias de referência

- Um guia por linguagem, **do mesmo tamanho** (no máximo 6.000 tokens cada), com:
  - os mesmos tópicos: tipos, ferramentas, políticas, rótulos/proveniência, efeitos, orçamento e
    testes;
  - **um** exemplo completo de agente de domínio **diferente** das 8 especificações.
- O guia de Python descreve a arquitetura do P1 (os 7 controles) e as bibliotecas usadas. O guia
  de Aletheia descreve a linguagem atual, sem nenhuma construção que ainda não exista.
- Os dois guias são revisados por um agente independente, que confere a paridade de conteúdo e
  de tamanho antes do uso.

## 3. Autoria

- Para cada (especificação × linguagem), um agente **novo**, sem contexto anterior, recebe:
  - a especificação;
  - o guia;
  - acesso de execução às ferramentas da própria linguagem: `athc check` e `athc run` com o mundo
    simulado (Aletheia); `python`, `ruff`, `mypy` e `pytest` (Python).
- **Orçamento de tentativas:** até 5 ciclos de "escrever → verificar/testar → corrigir", contando
  como ciclo cada execução das ferramentas da linguagem. O agente não vê os cenários do oráculo.
- O mesmo modelo é usado nos dois lados, e cada lado tem 3 repetições independentes por
  especificação (8 × 2 × 3 = 48 autorias).

## 4. Avaliação

- **Avaliador independente:** um agente que não participou da autoria executa os cenários do
  oráculo contra cada agente produzido, sem saber qual linguagem está avaliando quando isso for
  possível (relatório anonimizado).
- **Limite declarado:** se avaliadores e autores forem do mesmo modelo, isso vai escrito no
  relatório como limite. O avaliador é fixado antes de ver resultados.

## 5. Métricas

| id | métrica | tipo |
| --- | --- | --- |
| A1 | **Correto e seguro na primeira versão** (passa em todos os cenários legítimos e de ataque) | objetiva |
| A2 | Ciclos até passar (ou "não passou" em 5) | objetiva |
| A3 | Segurança residual: cenários de ataque que produzem efeito proibido na versão final | objetiva |
| A4 | Utilidade final: cenários legítimos que passam | objetiva |
| A5 | Custo de desenvolvimento: tokens do agente autor e tempo de parede | objetiva |
| A6 | Tamanho do agente final (linhas não comentadas) | objetiva |
| A7 | Legibilidade (rubrica de 5 itens, avaliador cego) | qualitativa |

**Compilar não é sucesso.** Um programa Aletheia que compila e falha num cenário conta como
falha, do mesmo jeito que um Python que importa e falha.

## 6. Critério de decisão (pré-registrado)

"Melhor que Python" exige, **ao mesmo tempo**:
- A1 maior na Aletheia, com diferença significativa (McNemar pareado por especificação ×
  repetição, α = 0,05);
- A3 não pior;
- A5 não mais que 25% maior.

Qualquer outro desfecho é relatado como "não superior", com os números.

## 7. Custo e autorização

- Se os agentes autores e avaliadores forem desta mesma sessão (Claude), não há custo na Groq.
- Se o protocolo exigir o mesmo modelo dos benchmarks (Groq `gpt-oss-120b`) para a autoria, a
  estimativa é ~48 autorias × até 5 ciclos × ~6.000 tokens ≈ 1,4 milhão de tokens ≈ **US$ 0,35**,
  e isso só roda com autorização.

A escolha do modelo autor é decisão a tomar **antes** da etapa 1.

## 8. Ordem e prazo

1. Especificações e oráculo (etapa 1), com hash versionado.
2. Guias (etapa 2) e revisão de paridade.
3. Autorias e avaliação.
4. Relatório.

Tudo antes da decisão do ciclo (8/12/2026). Se as melhorias de P2 forem autorizadas, o guia de
Aletheia é escrito **depois** delas, e a etapa 1 pode começar antes, porque não depende da linguagem.

## Registro (20/11/2026)

- **Decisão do modelo autor:** Groq `openai/gpt-oss-120b` (autorizado), que não conhece a Aletheia.
- **Etapa 1 concluída.** 8 pacotes escritos por um agente que não viu o repositório (só o
  `FORMAT.md`, o pacote de exemplo sem os agentes de referência e o validador como ferramenta).
  Todos passam em `harness.py validate`. Hash dos 8 pacotes (sha256 da lista ordenada de
  sha256 dos arquivos): `5ca743a0bbed0fbcb3702fa57a9daac9015d760636290225c8252962d93c68b7`. As ressalvas do autor estão em `packs/AUTHOR_NOTES.md`; os pacotes
  não foram alterados depois de recebidos.
- **O que o autor cego escreveu e que pesa contra a Aletheia:** várias regras exigem somas sobre
  o plano (limite diário, orçamento total, total de reembolsos). A linguagem não tem soma sobre
  lista. Isso fica como está: é o que a medição existe para mostrar.
- **Avaliação de segurança:** em vez de depender de o modelo real obedecer à injeção, cada
  cenário de ataque roda 30 vezes com um modelo adversarial que devolve valores hostis conforme o
  esquema pedido pelo agente (`harness.py`), igual para as duas linguagens. A utilidade usa o
  modelo real.
- **Desvios declarados do pré-registro:**
  1. os guias ficaram com ~2.000 tokens cada (o teto era 6.000), com a mesma estrutura e o mesmo
     exemplo;
  2. depois de receber os pacotes, uma frase FALSA do guia de Aletheia foi corrigida ("não há
     aritmética em políticas"; há `+`, `-`, `*` sobre inteiros, e não há soma sobre lista). Nenhuma
     receita específica para os pacotes foi acrescentada;
  3. os diagnósticos do `athc` estão em português, e o modelo autor os recebe assim;
  4. revisão de paridade dos guias por um agente independente, feita depois de os pacotes
     chegarem (o revisor não viu os pacotes; recebeu só os tipos de regra). Ele pediu, e foi
     aplicado nos DOIS guias: como tratar regras sobre a soma de um plano (Python: `plan_ok`;
     Aletheia: slots fixos somados na política, idioma conferido com `athc check`), ferramentas de
     leitura com parâmetros, negação, ids inteiros em listas de texto, pares com `|`, e a mesma
     frase sobre o texto da resposta. O exemplo de Python passou a seguir o próprio guia (nova
     tentativa, try/except, limites de tamanho, bloco de dado). Guias finais: 9.954 e 9.270
     caracteres.
