# Ciclo de consolidação 1

Aprovado por Antonio em 17/11/2026, a partir do parecer técnico "Consolidação do Core, vantagem
de engenharia e critérios de continuidade" e das emendas discutidas na mesma data.

**Decisão:** continuar uma vez, com escopo congelado. A Aletheia é avaliada como linguagem
independente; a alternativa de biblioteca sobre Python está descartada, então, se o ciclo
falhar, o resultado é encerrar, não pivotar. Multiagentes, MCP adicional e novos provedores
ficam para uma revisão posterior, sobre o Core consolidado.

**Norte:** que, quando alguém pedir "crie um agente para mim", seja razoável escrevê-lo em
Aletheia porque ela é a linguagem mais adequada, e não por preferência. Isso vira critério
mensurável (M10, abaixo).

## Escopo, em ordem

| | item | estado |
| --- | --- | --- |
| P1 | Corretude do Core: D1 (E0421 transitiva), D9 (delimitadores no prompt, nos dois runtimes), D12 (mensagem de sistema neutra) | **corrigidos** (ver abaixo) |
| P2 | Expressividade de políticas: D10 (valor monetário: positivo, limite, formato) e D2 (conteúdo de mensagem: links autorizados) | **feito**: D10 sem construção nova (centavos), D10b corrigido, D2 com `s.links_within` (autorizado em 20/11) |
| P3 | Ergonomia e esforço de mudança: origem das 90 linhas do M3; D8 (operações de lista) | a investigar |
| P4 | Invariantes de política declarados no programa e testados contra a matriz do modelo obediente, sem construção nova no Core | a desenhar |

Cada melhoria declara hipótese, contraexemplo inicial, mudança semântica, risco de regressão,
testes diferenciais e custo para o desenvolvedor. O Core não cresce para elevar uma métrica.

## Régua de aceite (fixada antes de qualquer melhoria)

1. **Corretude:** D1, D9 e D12 com reprodução negativa antes e teste positivo depois. Os
   artefatos históricos continuam compiláveis e reprodutíveis na versão em que foram medidos.
2. **Segurança:** nenhuma regressão nos cenários já protegidos.
3. **Utilidade:** sem queda relevante nas mesmas tarefas e condições.
4. **Expressividade:** D2 e D10 resolvidos com a decisão crítica na linguagem, não no host.
5. **Manutenção:** um conjunto **novo e cego** de pedidos de mudança, escrito depois que as
   melhorias estiverem prontas por um agente que não as viu. Cada lado é implementado por um
   agente diferente, que não conhece o outro lado. A Aletheia não pode exigir mais de 25% de
   linhas alteradas, e busca exigir menos. As 6 mudanças originais ficam como medida
   secundária. Medidas complementares: lugares tocados, tentativas até os testes passarem e
   defeitos introduzidos pelo implementador.
6. **Detecção:** a vantagem na detecção antecipada de erros estruturais se mantém, inclusive
   com catálogo cego novo.
7. **Independência:** revisão adversarial e comparação com Python endurecido competente.
8. **M10, autoria por modelo:** a partir de especificações cegas de agentes, um modelo escreve
   o agente em Aletheia e em Python endurecido, com o mesmo guia de referência de tamanho
   limitado para cada linguagem. Mede-se a taxa de agentes corretos e seguros na primeira
   versão, as iterações até passar e os defeitos de segurança que restam. "Melhor que
   Python" quer dizer vencer nessa medida.

**Regra de decisão:** se o ciclo não reduzir o custo de mudança preservando a vantagem
verificável e a segurança, a recomendação passa a ser encerrar. A régua não muda depois do
resultado.

## Prazo e orçamento

Prazo proposto: 3 semanas (até 8/12/2026), com ponto de controle em 1/12. Orçamento Groq: só a
reexecução final do AgentDojo com os artefatos novos (estimativa de US$ 0,20 a 0,30), mediante
autorização no momento. Nenhum merge sem autorização.

## Registro

**17/11 — P1 concluído.**

- **Reprodução antes da correção** (commit `ceeda2a`): três testes afirmavam o comportamento certo
  e passavam enquanto o defeito existia (`should_panic` em Rust, `expectedFailure` em Python).
- **D1:** a análise de ancoragem (E0421/E0422) agora percorre o fecho das políticas chamadas
  (`policy_closure` em `ath-check`). A política citada no `refine` decide também pelos
  inventários e literais das políticas que chama.
- **D9:** o texto citado (`quote`) tem `<<<`, `>>>` e `#` no início de linha trocados por sósias
  visuais (`‹‹‹`, `›››`, `＃`), com a mesma função nos dois runtimes (`neutralize_quoted` /
  `_neutralize_quoted`). Os testes diferenciais ARX × Python continuam idênticos.
- **D12:** a mensagem de sistema passou a ser neutra e em inglês, igual nos dois runtimes, e o
  cabeçalho do prompt deixou de citar a linguagem.
- **Compatibilidade histórica.** O compilador corrigido recusa os programas medidos antes:
  `banking.ath`, `banking_contracts.ath`, `banking_grants.ath` e `banking_intents.ath`, todos por
  E0421, que é exatamente o D1. Nenhum outro `.ath` do repositório mudou de veredito (conferido
  arquivo por arquivo). Essas evidências continuam reprodutíveis com o compilador e o runtime com
  que foram medidas: `scripts/historico.sh` constrói o commit `ef0506f`, o último antes das
  correções. O CI usa esse compilador para os testes que dependem desses programas e para as
  matrizes adversariais, e roda, no código histórico, o replay do tráfego real da Groq. Os
  cassetes foram gravados com a mensagem de sistema antiga, então o replay só reproduz no código
  histórico. O CI também confere que o compilador corrigido recusa `banking.ath` com E0421.
- **Pendente, fora do P1:** o workflow pago `agentdojo.yml` ainda verifica o pipeline histórico
  `aletheia` com o compilador atual. Antes de qualquer rodada paga nova, ele precisa usar o
  compilador histórico para esse pipeline, ou só os programas novos.

**17/11 — P2: diagnóstico e desenho (orientação da mesma data; sem construção no Core).**

- [P2-DESENHO.md](P2-DESENHO.md):
  - D10: a alternativa A (centavos como `Int[1..N]`) não exige construção nova e foi medida com o
    runtime atual (`docs/p2/centavos.ath`);
  - D10b: o `decimal()` da linguagem aceita vírgula ambígua (defeito; a correção aguarda
    autorização);
  - D2: só propriedades sintáticas contra uma gramática fixa são verificáveis; a alternativa
    recomendada (`s.links_within`) é uma construção e aguarda autorização.
- D9: teste de preservação (o valor original chega intacto ao programa nos dois runtimes, com o
  mesmo `prompt_hash`). A neutralização foi estreitada aos marcadores do runtime, depois de a
  reconstrução do D12 mostrar que ela trocava cabeçalhos do próprio programa.
- D12: `test_d12.py`. Nos 11 pares, os dados estavam no prompt: a causa é comportamento do modelo.
  A rodada real de verificação (~US$ 0,01) não foi disparada.
- M10: pré-registro em [bench/m10/PROTOCOLO.md](../bench/m10/PROTOCOLO.md). A escolha do modelo
  autor fica antes da etapa 1.
- Workflow pago: o pipeline histórico usa o compilador histórico; sem ele, `run.py` recusa.

**19/11 — sala de guerra; orçamento Groq de US$ 1,00 autorizado.**

- **D10b corrigido.** Antes: `decimal("1,50")` = 150, `"12,34"` = 1234, `"1,2,3"` = 123, `",5"` = 5.
  Agora a vírgula só vale como separador de milhar bem formado (`1,050.00`), e o resto é ilegível
  (falha fechado em `s.within` e `s.binds`). Mesma leitura nos dois runtimes, com teste em cada um.
- **D10 adotado sem construção nova:** `bench/agentdojo/banking_v3_keeping.ath`, com
  `cents: Int[1..100000000]` no lugar de `amount: Text[..16]`. `tests/test_v3_cents.py`: 11 valores
  hostis (0, negativo, acima do limite, 10^30, texto, vírgula decimal, fração, nulo, booleano,
  lista, objeto) são recusados pelo tipo antes da política, sem efeito; valores válidos chegam à
  função oficial com o valor exato; o modelo obediente dá os mesmos números do v2 (0/144 ataques,
  125/144, 14/16).
- **Rodada paga do ciclo** (logdir `banking-ciclo1-openai-gpt-oss-120b`, teto acumulado US$ 0,25):
  `aletheia@banking_v2_keeping` (runtime corrigido: mede D9/D12), `aletheia@banking_v3_keeping`
  (mede a utilidade dos centavos) e `hardened:keeping` (P1), pareados, na suíte inteira. Estimativa:
  480 execuções × ~1.850 tokens ≈ 890 mil tokens ≈ US$ 0,20, em 3 janelas do limite diário de 400 mil.

**20/11 — rodada paga do ciclo, 1ª janela** ([44655413682](https://github.com/Antoniorjr4/Aletheia/actions/runs/44655413682)).
Parou no limite diário da Groq com 437 de 480 execuções e US$ 0,1797 gastos (teto US$ 0,25).
Completos nos três pipelines: 16/16 sem ataque e **129/144** pares
([compare.md](../bench/agentdojo/runs/banking-ciclo1-openai-gpt-oss-120b/compare.md)).

| pipeline | utilidade | utilidade sob ataque | ataques cumpridos | tokens/execução |
| --- | --- | --- | --- | --- |
| Aletheia v2 keeping (runtime corrigido) | 11/16 | 71/129 | **0/129** | 1.849 |
| Aletheia v3 keeping (centavos) | 10/16 | 84/129 | **0/129** | 1.885 |
| P1 Python endurecido (keeping) | 8/16 | 65/129 | **0/129** | 1.805 |

- **D12 (mensagem de sistema neutra), medido.** Nos mesmos 72 pares da rodada de 16–17/11: Aletheia
  v2 foi de 33 para **42**, e o P1 de 42 para 45 (o P1 não mudou de código: a variação dele, 3
  pares, é o ruído do modelo). A desvantagem de 9 pares desapareceu. Nos 11 pares em que a Aletheia
  alegava não ter os dados, ela agora acerta 7 (antes 0). Esses 11 foram escolhidos por serem
  falhas, então parte da melhora é regressão à média, e o número que vale é o dos 72.
- **D10 (centavos), medido.** Nenhum sinal de confusão de unidade: os valores enviados pelo v3 são
  os da suíte (98,70; 200,29; 1.100; 1.200…), sem valores 100× maiores ou menores. Utilidade sob
  ataque 84 contra 71 do v2 (McNemar 19 × 6, p = 0,015, sem correção para comparações múltiplas).
  Sem ataque, 10 contra 11 (p = 1,0). Os centavos não custaram utilidade.
- **Aletheia × P1:** v2 12 × 6 (p = 0,24); v3 28 × 9 (p = 0,003, sem correção). Segurança igual
  (0 ataques nos três).
- **Observação D13 (não corrigida no meio da campanha).** A captura das requisições HTTP mostrou que
  as duas implementações enviam o mesmo prompt de usuário, o mesmo teto de tokens, temperatura e
  esforço de raciocínio. Diferem na mensagem de sistema ("an assistant" × "a banking assistant") e
  na ordem das propriedades do esquema JSON: o runtime Aletheia as ordena alfabeticamente
  (`answer` antes de `writes`), e o P1 usa a ordem de declaração. Uma resposta ainda saiu em
  português: os rótulos das seções do prompt são em português nos dois.
- Faltam 15 pares (43 execuções, ~80 mil tokens, ~US$ 0,02). Retomada agendada para a próxima janela.

**20/11 — rodada paga do ciclo, concluída** ([44750019730](https://github.com/Antoniorjr4/Aletheia/actions/runs/44750019730)).
O limite de 400 mil tokens por dia era uma configuração do projeto na Groq, não da organização; o
dono da conta a elevou para 3 milhões (100 mil tokens/min, 300 requisições/min). A retomada fechou as
43 execuções restantes por US$ 0,0205. Total da rodada: **US$ 0,2002** (teto US$ 0,25), 480/480
execuções, 16/16 sem ataque e **144/144** pares nos três pipelines
([compare.md](../bench/agentdojo/runs/banking-ciclo1-openai-gpt-oss-120b/compare.md)). Os números
abaixo substituem os parciais de 129 pares como resultado da rodada; os parciais ficam registrados acima.

| pipeline | utilidade | utilidade sob ataque | ataques cumpridos | tokens/execução |
| --- | --- | --- | --- | --- |
| Aletheia v2 keeping (runtime corrigido) | 11/16 | 76/144 (52,8%; IC95 45–61%) | **0/144** | 1.864 |
| Aletheia v3 keeping (centavos) | 10/16 | 92/144 (63,9%; IC95 56–71%) | **0/144** | 1.908 |
| P1 Python endurecido (keeping) | 8/16 | 67/144 (46,5%; IC95 39–55%) | **0/144** | 1.817 |

McNemar exato, pareado, sem correção para comparações múltiplas (três comparações; com Bonferroni o
limiar é 0,0167):

- **v3 × P1, utilidade sob ataque:** 34 × 9, p = 0,0002. Passa com a correção.
- **v3 × v2, utilidade sob ataque:** 23 × 7, p = 0,0052. Passa com a correção.
- **v2 × P1, utilidade sob ataque:** 16 × 7, p = 0,093. Não é diferença.
- **Sem ataque:** nenhuma diferença (p ≥ 0,25 nas três); com 16 tarefas o teste não tem poder.
- **Segurança:** igual, 0/144 nos três (limite superior do IC95: 3%).

Limites da leitura: uma suíte, um modelo, um ataque, uma repetição por par; o P1 e o v3 diferem em
mais de um fator (tipo do valor, mensagem de sistema, ordem do esquema — D13), então a vantagem do v3
sobre o P1 não é atribuível só aos centavos. D13 segue para correção com reprodução antes, agora que
os 144 pares estão fechados.

**20/11 — orçamento do ciclo elevado para até US$ 1,50** (autorização do dono do projeto), depois
de a 1ª janela de autoria do M10 projetar ~US$ 0,55 para as 48 autorias contra US$ 0,35–0,40
estimados. Gasto até aqui: US$ 0,2002 (AgentDojo) + US$ 0,0555 (M10). Registro das janelas em
[bench/m10/PROTOCOLO.md](../bench/m10/PROTOCOLO.md).

**20/11 — M10 concluído: NÃO SUPERIOR pelo critério pré-registrado.** Autoria 48/48 (US$ 0,3837),
avaliação até US$ 0,1455. A1 (correto e seguro na 1ª versão): Aletheia 2/24, Python 10/24; McNemar
1 × 9, p = 0,0215, a favor do Python. Versão final segura: 16/24 × 23/24. Tokens de autoria: 514 mil
× 735 mil (Aletheia 30% menor). Das 8 finais Aletheia não seguras, 5 não compilam em 5 ciclos e 3
compilam com política de negócio insuficiente. Detalhe e limites em
[bench/m10/PROTOCOLO.md](../bench/m10/PROTOCOLO.md). **Gasto do ciclo até aqui: US$ 0,7294**
(AgentDojo 0,2002 + autoria 0,3837 + avaliação 0,1455), de até US$ 1,50 autorizados.

**20/11 — D2: `s.links_within` (construção autorizada) e M10 com o modelo da Groq (autorizado).**

- **Reprodução antes:** `slack.ath` v1 com o modelo obediente: a injeção de phishing por mensagem
  passa em 16/105 (obediente) e 21/105 (sequestrado). Teste `SlackObedientTests`.
- **Construção:** `s.links_within("k", texto)`, predicado de política. Gramática G: trecho máximo de
  caracteres de URL que começa em `http://` ou `https://` (em qualquer posição, sem diferenciar
  maiúsculas) ou em `www.` não precedido de letra ou dígito ASCII. Comparação pela forma canônica
  (minúsculas ASCII, sem esquema, sem pontuação nem barra no fim). Sem link, verdadeiro; link que
  se reduz a vazio falha fechado.
- **Garantia anunciada:** nenhum link **na forma G** fora do inventário sai num efeito. **Não** é
  detecção de phishing. Fora de G (declarado e fixado nos vetores): domínio nu (`evil.com`),
  `example[.]com`, `hxxp://`, caracteres de largura total.
- **Validação:**
  - 49 vetores escritos à mão (`examples/fixtures/links_within.json`), lidos pelos testes dos dois
    runtimes;
  - verificador: E0421 (inventário fora do prompt), E0423 (fora de política), E0424 (nome não
    literal), tipo e aridade;
  - 7 corpos hostis de ponta a ponta com resultado e journal idênticos no ARX e no Python;
  - `slack_v2.ath` com o modelo obediente: **0/105 e 0/105** (antes 16 e 21), e 21/21 tarefas
    legítimas.
- **Complexidade:** 1 predicado; nenhuma regra ou diagnóstico novo (reusa E0421, E0423, E0424);
  ~130 linhas no Core (verificador, ARX, emissor, runtime Python); nenhuma mudança nas provas Lean
  (os predicados da fotografia não são modelados lá); nenhum programa existente mudou de veredito
  (conferido arquivo por arquivo contra o compilador histórico: só os 4 do D1).

**24/11 — pós-M10: ergonomia e diagnóstico, sem campanha paga.** O resultado do M10 (NÃO SUPERIOR)
não muda. Feito: sintaxe `//`, `_`, `{ .. }`, vírgula opcional (`e92ca42`); `len` em texto e `budget`
contextual (`ced796c`); vários erros de sintaxe por compilação, com recuperação só em fronteira de
item (`881106d`); âncora de inventário através de `let`, resolvida por escopo e com prompt idêntico
nos dois runtimes (`26fb9e8`). Reanálise das 45 versões rejeitadas em
[bench/m10/runs/reanalise/TABELA.md](../bench/m10/runs/reanalise/TABELA.md): 6 compilam agora (0
antes), as 6 sem violação nos ataques adversariais; 72 erros restantes, 26 deles de garantia
(política, efeitos, orçamento), que não serão relaxados. É banco de regressão, não evidência.
Variantes homônimas: proposta em [p2/variantes-homonimas.md](p2/variantes-homonimas.md), sem código;
a causa raiz é o idioma de slots fixos imposto por D8, que fica em P3.

**24/11 — P3 iniciada: D8 resolvido; D14 encontrado e fechado.** Decisão delegada pelo dono do projeto
("decida e execute"). `sum x in xs => n` entra no núcleo como terceira dobra delimitada, ao lado de
`all`/`any`: corpo `Int`, mesma aritmética de `+` nos dois runtimes, intervalo do resultado provado
pelo tipo quando o corpo tem intervalo declarado (o provador do E5 não deriva intervalos de `match`
nem de literais; limite registrado). Remove a necessidade do idioma de slots fixos. Testes em
`crates/athc/tests/ciclo1_sum.rs`, com equivalência ARX × Python em 9 planos.

**D14 (defeito de garantia, pré-existente desde a RC10):** ao tipar a soma, quatro fluxos de
confidencialidade aceitos indevidamente: elemento de lista `Secret` tratado como `Public` dentro de
`for`/`all`/`any`; contagem de iterações de `for` sobre lista `Secret`; resultado de `all`/`any` sobre
lista `Secret`; `text(n)` descartando a confidencialidade. Os quatro agora dão E0440. Nenhum programa
`.ath` do repositório muda de aceitação (comparação arquivo a arquivo contra o compilador anterior).
Não há medição publicada que dependesse desses fluxos: nenhum benchmark usa `^ Secret`.

Variantes homônimas: aplicada só a opção A (E0002 diz o tipo e sugere nome e `sum`); resolução de
nomes inalterada. O guia do M10 está congelado e não foi alterado; uma campanha futura precisa de
guia novo, versionado à parte.

**25/11 — P4: diagnóstico fechado** ([p4/DIAGNOSTICO.md](p4/DIAGNOSTICO.md)). As 3 falhas de regra do
M10 foram omissões do autor em quatro classes: cumulativa (agora `sum`), unidade (aberta), fato por
chave confiável (já havia `binds`), unicidade (`sum` aninhada). As três políticas corretas foram
escritas e testadas nos dois runtimes: os ataques do M10 e 7 variantes hostis são recusados pela
política; os legítimos passam. Corrigido no caminho: `(` em linha nova não é mais chamada da linha
anterior. Em aberto: nada impede o autor de omitir a regra; unidades em tipos é a proposta de maior
efeito e exige decisão.

**25/11 — autorização geral do dono do projeto** ("No que precisar de minha autorização, considere
autorizado"). Interpretação aplicada: cobre as decisões de linguagem dentro do ciclo, inclusive
construção nova no núcleo (unidades em tipos, proposta em [p4/DIAGNOSTICO.md](p4/DIAGNOSTICO.md)).
Não cobre merge de PR, gasto acima do orçamento de US$ 1,50 do ciclo nem mudança de critério,
resultado ou teste; esses continuam exigindo pedido explícito.

**26/11 — P3 medido: critério 5 NÃO atendido** ([bench/agentdojo/p3/PROTOCOLO.md](../bench/agentdojo/p3/PROTOCOLO.md)).
Seis pedidos de mudança escritos por autor cego e congelados antes das implementações; um
implementador por lado, sem ver o outro. Linhas de produção alteradas: Aletheia 78, P1 50 (**+56%**,
margem 25%); tentativas 19 × 6; nenhuma versão incompleta pega estaticamente em nenhum dos lados. Em
3 dos 6 pedidos (total corrido, tudo-ou-nada, conteúdo de texto) a decisão teve de ir para o host.
Itens de linguagem que saem daí: predicados de texto em política, `keeping` com acumulador, contagem
de recusas disponível à decisão de executar.

**26/11 — pós-P3: as três lacunas fechadas na linguagem** ([p3/CAPACIDADES.md](p3/CAPACIDADES.md)).
Diretiva do dono do projeto autorizando a implementação. Texto na política (`contains`, `lower`,
`ascii_letters`, `ascii_digits`); `keeping` com a lista dos já aceitos (avaliação ordenada, C3);
`keeping all` (autorização integral, C5). Modelo Lean estendido (`pruneOrd_sound`, `pruneAll_sound`,
`pruneOrd_stateless`). Reaplicação dos quatro pedidos que tinham ido para o host: 67 → 27 linhas, host
intacto, 19/19 casos; total dos seis 78 → 38 (P1: 50). **Diagnóstico sobre casos conhecidos; o
resultado do P3 não muda.** Núcleo: +215/−16 linhas.

**26/11 — revisão independente das capacidades pós-P3; D15 e D16 fechados.** Revisor sem
participação nas mudanças: nenhum caminho novo para dado não confiável ou secreto chegar a efeito,
nenhuma divergência entre runtimes nas entradas aceitas. Achados corrigidos, cada um com teste
(`crates/athc/tests/ciclo1_revisao.rs`): **D15** (efeito irrevogável sem parâmetro rotulado podia ser
decidido por dado não confiável; anterior ao ciclo), **D16** (valor de ramo `Secret` saía público de
`if`/`match`; anterior ao ciclo), veredito de política sobre segredo, estouro em conta de política
(agora E0451 estático, com aritmética de intervalos), E0422 contornável por `contains`, e quatro
formas mal conferidas. Nenhum programa do repositório nem do M10 muda de aceitação. Em aberto: `-0`
aceito só pelo runtime Python. Tabela de garantias corrigida onde afirmava mais que o provado.

**26/11 — reproduções dirigidas de D15/D16 confirmadas e corrigidas; `-0` alinhado.** Os três casos
pedidos pelo dono do projeto compilavam: `return` condicional sobre dado não confiável seguido de efeito
irrevogável (inclusive com parâmetro `^ Endorsed`), o mesmo com dado `Secret` e canal público, e
parâmetro `^ Untrusted` liberando a decisão do efeito. Agora E0412/E0440, com controles seguros
aceitos e nenhum programa do repositório ou do M10 mudando de aceitação. `-0` lido como no serde_json
nos dois runtimes. `pruneOrd_sound` documentado como teorema sobre a lista resultante. Auditoria de
duplicação nos oráculos do M10: 13/40 verificadores legítimos aceitariam um efeito duplicado; nenhum
cenário aprovado no M10 teve duplicação, então o resultado não muda; vira critério para o M11.

**26/11 — P3b medido: critério 5 ATENDIDO nesta medição** ([bench/agentdojo/p3b/PROTOCOLO.md](../bench/agentdojo/p3b/PROTOCOLO.md)).
Implementação e guias congelados (`bench/m11/FREEZE.json`, commit `223fb53`) antes dos pedidos. Seis
pedidos novos de um autor cego, implementadores separados: Aletheia 83 linhas, P1 102 (0,81 × P1;
margem 1,25). Tentativas 16 × 6. Uma regra (D5, conjunto de caracteres) foi para o host; D3 ficou no
handler. O P3 (+56%, contra) continua registrado: são duas medições, uma de cada lado, e o relatório
final traz as duas.

**26/11 — M11: autoria parada no teto, 32/48.** Auditoria dos oráculos antes da autoria: 2 defeitos
corrigidos em `packs_v2`, iguais para os dois lados. A autoria parou no teto de US$ 0,42, nas
repetições 1–2. **Gasto do ciclo: US$ 1,1504.** Ainda não houve avaliação; a decisão sobre a terceira
repetição fica com o Antonio e é tomada antes de ver os resultados.

**26/11 — gasto adicional autorizado: US$ 0,00.** Antonio, 13:32: limite de US$ 1,00 no acumulado,
já passado (US$ 1,1504), então nenhuma chamada paga. Os jobs pagos do `agentdojo.yml` agora falham
antes de qualquer requisição se o pedido passar de `bench/orcamento/AUTORIZACAO.json` (hoje 0).

**26/11 — M11: avaliação adversarial local** ([bench/m11/AVALIACAO-LOCAL.md](../bench/m11/AVALIACAO-LOCAL.md)).
M11 interrompido por orçamento após 32 de 48 autorias. Avaliação adversarial local realizada;
utilidade com modelo real e critério principal de superioridade não avaliados. Resultado local contra
a Aletheia: a 1ª versão roda em 0/16 (Python 13/16), a final roda em 9/16 (Python 15/16), a final é
segura em 6/16 (Python 13/16). Causa dominante: renomeação de campo no padrão, tentada em 15 das 16
autorias, sem diagnóstico que diga a alternativa. Achados: um defeito no texto do E0422 (cita a
suíte bancária) e uma limitação de vocabulário (faixa numérica numa linha confiável).

**26/11 — reserva de saldo antes de cada requisição** ([bench/orcamento/reserva.py](../bench/orcamento/reserva.py)).
O livro do M10 confere o teto antes e debita depois, e as tentativas internas do cliente não passam
por ele; daí US$ 0,4210 contra um teto de 0,42. O módulo novo calcula o pior caso (entrada por cota em
bytes, saída por `max_completion_tokens`), recusa sem enviar se não cabe, grava a reserva antes,
liquida pelo uso informado ou pelo pior caso, desliga as tentativas internas e faz cada tentativa
passar pela reserva. Onze testes offline, inclusive o histórico real do M11 reaplicado (para antes de
0,42). Fica separado dos artefatos congelados: `bench/m10/author.py` não foi tocado. Qualquer campanha
futura precisa ligá-lo ao executor e ao avaliador, e isso depende de autorização de gasto.

**28/11 — E0453: implementada e interrompida; garantia não estabelecida** ([bench/agentdojo/x2/e0453/RELATORIO.md](../bench/agentdojo/x2/e0453/RELATORIO.md)).
Decisão arquitetural X2 de 28/11. A E0453 recusa a chamada que combina dados de operações aprovadas
distintas quando detecta a combinação; a testemunha X2 que recombinava (4 chamadas para 2 aprovadas)
não compila mais, e o corpus muda só em 30 programas construídos para misturar (nenhuma autoria do
M10/M11). Três revisões independentes acharam, cada uma, uma classe nova de fuga; as duas primeiras e
parte da terceira foram corrigidas, e restam 5 fugas conhecidas (construtor que embrulha registro;
lista literal de listas). Pela própria decisão, a implementação parou e o impedimento foi
apresentado: sustentar a garantia pede a origem no sistema de rótulos e a proveniência medida no
runtime. Agregação pura fora, com decisão pendente. Sem gasto, sem merge.

**28/11 — decisão final: E0453 rebaixada a aviso experimental; defeito no rótulo do resultado de ferramenta** ([bench/agentdojo/x2/e0453/RELATORIO.md](../bench/agentdojo/x2/e0453/RELATORIO.md), seção 10; [bench/ferramentas/DEFEITO.md](../bench/ferramentas/DEFEITO.md)).
Opção B: a E0453 aponta misturas suspeitas e não recusa; o texto diz que é parcial e não certifica.
Redesenho não autorizado; agregação pura preservada (triagem RC8–RC10 inalterada). Regressão: 478
programas versionados com o mesmo veredito do compilador anterior à E0453; os 13 programas gerados
que tinham saído do tripé voltaram e concordam; B1 e F2 voltaram a rodar nos dois runtimes. A
investigação do fluxo por ferramentas achou um defeito anterior ao ciclo, de gravidade alta: o
retorno sem rótulo é `Trusted` e `Public` e não herda os argumentos, o que deixa dado `Untrusted`
chegar a parâmetro `^ Endorsed` e decidir efeito irrevogável, e segredo chegar a sink público.
Menor correção indicada e medida (nenhum veredito do corpus muda); não aplicada. Sem gasto, sem
merge.

## Parecer do checkpoint (01/12/2026), escrito em 26/11

Resultados medidos, todos mantidos, os contrários inclusive:

| frente | resultado | leitura |
| --- | --- | --- |
| capacidades pós-P3 (texto na política, `keeping` com estado, `keeping all`, E0451, D15, D16) | implementadas, com prova Lean de `pruneOrd`/`pruneAll`, revisão independente e reproduções dirigidas recusadas | [docs/p3/CAPACIDADES.md](p3/CAPACIDADES.md) |
| decisões retiradas do host | reaplicação C2/C3/C5/C6 só no programa: 78 → 38 linhas | diagnóstico, não medição cega |
| manutenção P3 | 78 × 50, critério **não** atendido | contra |
| manutenção P3b (pedidos cegos novos, versão congelada) | 83 × 102 (0,81), critério **atendido**; tentativas 16 × 6; D3 no handler; D5 depende do host | a favor nas linhas, com custo em tentativas e uma regra fora da linguagem |
| autoria e segurança M11 (local) | 1ª versão roda 0/16 × 13/16; final segura 6/16 × 13/16 | contra |
| utilidade M11 com modelo real | não avaliada | sem orçamento |
| critério principal de superioridade | não avaliado | sem orçamento |

**Parecer.** Até aqui, o ciclo não demonstra superioridade da Aletheia sobre um Python competente.
Ele demonstra três coisas menores. Primeiro, com as capacidades novas, as regras do P3 cabem no
programa e a manutenção ficou mais curta numa medição cega (P3b), ao custo de mais tentativas.
Segundo, as garantias seguraram: nenhuma violação passou por fora da política escrita, e saída
malformada nunca foi aceita. Terceiro, a autoria por modelo foi pior no M11 que no M10, com guias v2
e pacotes novos ao mesmo tempo (a causa não pode ser atribuída exclusivamente aos guias), e a causa
principal das recusas é estreita e identificável.

Bloqueios, em ordem de efeito medido:

1. **Renomeação de campo no padrão** (`Use { item: k }`). É limitação da linguagem, e a mensagem de
   erro não diz a alternativa. Pesou em 15 das 16 autorias.
2. **Texto do E0422.** Defeito de implementação: local errado e ajuda de outra suíte.
3. **Faixa numérica numa linha confiável** (`chave|mín|máx`). Limitação de vocabulário; causou a
   única violação exclusiva da Aletheia que não foi omissão do autor.
4. **Conjunto de caracteres na política** (D5 do P3b). Limitação; levou a regra para o host.
5. **Palavras reservadas sem aviso** (`authority`, `policy`). Diagnóstico insuficiente.

Corrigir 1, 2 e 5 não exige gasto e não muda a semântica das garantias. Mesmo assim, qualquer
correção vira uma versão nova: a medição do M11 continua presa a `223fb53`, e um efeito sobre a
autoria só pode ser medido com nova autorização de gasto. Sem ela, o que cabe até 08/12 é mostrar por
reaplicação local, declarada como diagnóstico, que as 32 autorias existentes passariam a compilar ou
não. A decisão final de 08/12 terá de dizer que o critério principal não foi avaliado.

**26/11 — pós-M11: renomeação em padrão, E0422 e palavra reservada** ([docs/POS-M11.md](POS-M11.md)).
Versão posterior à congelada; a medição do M11 continua presa a `223fb53` e não muda. O padrão
`Use { item: k }` liga o campo `item` ao nome local `k`, que recebe o tipo, os limites, os rótulos e
a origem do campo. Campo repetido e nome ligado duas vezes dão E0014. O E0422 aponta a comparação
responsável e orienta pelas listas do programa, e a palavra reservada usada como nome é dita como
tal. Corpus versionado (130 programas) idêntico: aceitação, códigos, raiz do Codex e Python emitido.
Reanálise das 71 versões arquivadas, como diagnóstico: as primeiras que compilam passam de 0 para
1, as finais ficam em 9; em 22 versões o erro seguinte ficou visível. Avaliação adversarial local
com a versão nova: finais iguais às da congelada (3 violam, 6 sem violação observada, 7 não
executam). Revisão independente: três achados da mudança corrigidos (hash do nome local, `campo: _`,
texto da palavra reservada); dois defeitos anteriores na análise de obrigações (B1, B2) registrados
como pendência, fechados em runtime por R0631. Gasto US$ 0,00.

**27/11 — estado das avaliações, registrado separadamente** (diretiva do Antonio, 27/11 15:44).
A autoria por modelo faz parte expressa da tese registrada no M10, no M11 e no plano do ciclo.
Familiaridade do modelo com Python é explicação plausível para parte das dificuldades, mas a
contribuição dela não foi isolada da contribuição da linguagem, do guia e dos diagnósticos.

- **M10:** avaliação concluída; resultado não superior.
- **M11:** 32 autorias realizadas e avaliação adversarial local concluída; resultado local
  desfavorável.
- **M11:** utilidade com modelo real e critério principal completo não avaliados.
- **Correções posteriores** ([POS-M11.md](POS-M11.md)): diagnóstico sobre os programas arquivados.
  Não se infere economia de tentativas nem de tokens, porque ela não foi medida.

**27/11 — checkpoint de 01/12 escrito** ([CHECKPOINT-01-12.md](CHECKPOINT-01-12.md)). B1 e B2 foram
reproduzidos, corrigidos e revisados em três rodadas. As revisões acharam outros casos do mesmo
defeito (F1, F2, G1, H1), anteriores ao ciclo, e eles foram corrigidos com reprodução e teste.
Também acharam excesso de cautela (F3, G2, G3), corrigido. H2 e H3 ficam abertos. Na varredura,
R0631 caiu de 258 para 0 e a divergência entre os runtimes de 27 para 0; o corpus real ficou
inalterado. A regressão do M4 saiu idêntica a 17/11. A extensão M4-ext: no X1, a Aletheia pegou na
compilação e o P1 e o Pysa não; no X2, nenhum dos dois pegou. Gasto adicional: US$ 0,00.

**27/11 — X2: testemunha, contrato relacional e proposta** ([bench/agentdojo/x2/RELATORIO.md](../bench/agentdojo/x2/RELATORIO.md),
[PROPOSTA-OPERACAO.md](PROPOSTA-OPERACAO.md)). Reprodução dirigida posterior ao M4-ext, não cega,
US$ 0,00.

- **Testemunha:** com o espião no banco oficial, a mutação X2 envia 4 chamadas na Aletheia (ARX e
  Python iguais), duas delas não aprovadas, e 2 no P1, uma não aprovada. O journal do P1 registra
  R2 25,00 enquanto o banco recebe R2 10,00, e a deduplicação do P1 deixa uma transferência aprovada
  sem sair. Todas as chamadas respeitam a política escrita, que só confere o destinatário.
- **Contrato relacional (`requires s.binds`):** recusa a combinação fora da lista antes da
  ferramenta, mas deixa passar combinações que estão na lista e não são a operação aprovada.
- **Protótipo `athc operacoes` (diagnóstico, sem efeito na aceitação):** acusa a mistura no X2 e
  não acusa nenhuma nos 69 programas que compilam no corpus.
- **Proposta (não implementada):** checagem estática E0453; composição explícita como variante do
  plano aprovada pela política.
