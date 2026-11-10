# Rev7 — AgentDojo oficial: adaptador preliminar

Fonte upstream: https://github.com/ethz-spylab/agentdojo

A API oficial exporta `agentdojo.task_suite.load_suites.get_suite(version, name)`
para as suítes `workspace` e `banking`. Este diretório integra apenas
**descoberta de metadados** e uma fronteira de classificação de efeitos
**fail-closed**; ainda **não executa** user tasks nem injection tasks contra
um agente Aletheia. Nunca rotular estes scripts como benchmark oficial concluído.

## Preparação no ambiente isolado

```bash
python3 -m venv .venv-agentdojo
. .venv-agentdojo/bin/activate
pip install agentdojo
python3 bench/agentdojo/discover.py --suite workspace --output bench/agentdojo/out/workspace.json
python3 bench/agentdojo/discover.py --suite banking --output bench/agentdojo/out/banking.json
python3 -m unittest discover -s bench/agentdojo/tests -v
```

Versão **das suítes** fixada: `v1.2.2`. Não confundir com versão
do pacote Python; mantenha o commit upstream do pacote no manifest
antes de qualquer experimento publicado.

## Fronteira de segurança preliminar

`authorize_tool` exige: ferramenta mapeada explicitamente, efeito coberto
por `Cap[E]` e política independente aprovada. Ferramentas desconhecidas,
operação bancária sem autorização ou ausência de política são rejeitadas.
O método **não** executa ferramentas; trata-se de preparação para a ponte
de runtime. As ações de `banking` deverão requerer correspondência de
identidade/destinatário/valor autorizados no contexto legítimo, não só
uma lista de métodos permitidos.

## Próximos itens para rodar o benchmark oficial

1. Traduzir a AST dos argumentos de cada ferramenta oficial para um tipo
   Aletheia (sem alterar a semântica do benchmark), e registrar contratos.
2. Implementar `BasePipelineElement` compatível com o AgentDojo e a
   ponte síncrona de tool calls com o executor real Aletheia.
3. Preservar todo input das suítes, ataques, utility checkers e predicados
   oficiais sem reescrevê-los para beneficiar um dos agentes.
4. Adicionar instrumentação de erro, latência e custo e executar baseline
   e Aletheia sob a mesma versão, modelo, seed e orçamento.
5. Incluir controles negativos (prompt em log, e-mail, documento,
   transferências bancárias) e relatar utility, ASR e violações separadamente.

O M1 customizado anterior **não é** resultado oficial AgentDojo.
