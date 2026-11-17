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
| P2 | Expressividade de políticas: D10 (valor monetário: positivo, limite, formato) e D2 (conteúdo de mensagem: links autorizados) | a desenhar |
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
