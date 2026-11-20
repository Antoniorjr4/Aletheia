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
| P2 | Expressividade de políticas: D10 (valor monetário: positivo, limite, formato) e D2 (conteúdo de mensagem: links autorizados) | **desenho pronto** ([P2-DESENHO.md](P2-DESENHO.md)); implementação aguarda autorização |
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
