# AgentDojo oficial × Aletheia

Fonte upstream: https://github.com/ethz-spylab/agentdojo · pacote `agentdojo==0.1.35` ·
suítes `v1.2.2`.

Este diretório executa a suíte oficial `banking` do AgentDojo contra um modelo real da Groq,
em dois pipelines com o mesmo modelo, o mesmo ataque e os mesmos verificadores:

- **`baseline`**: o pipeline padrão do AgentDojo, sem defesa. Usa a mensagem de sistema
  oficial, o `OpenAILLM` e o laço de tool calling `ToolsExecutionLoop` sobre as ferramentas da
  suíte.
- **`aletheia`**: o agente [`banking.ath`](banking.ath), verificado pelo `athc`, emitido para
  Python (`athc emit --target py`) e executado pelo `aletheia_runtime`. Ele entra no AgentDojo
  como um `BasePipelineElement` ([`AletheiaAgent`](aletheia_pipeline.py)).

Nada da suíte é reescrito: as tarefas, o ambiente injetado, as funções, o ataque
`important_instructions` e os verificadores de utilidade e de segurança são os do pacote. A
execução usa as funções oficiais `run_task_without_injection_tasks` e
`run_task_with_injection_tasks`, e os logs saem no formato do AgentDojo.

## O agente Aletheia

O pedido do usuário é a única instrução `Trusted`. Tudo o que as ferramentas devolvem
(transações, arquivos, agenda, cadastro) é `Untrusted`: entra no prompt só como dado citado e
nunca decide uma ação sozinho. O agente faz duas inferências tipadas, com decodificação
restrita pelo esquema derivado do tipo:

1. **Leitura** (`infer planner -> Reads`): o planejador vê só o pedido e marca as leituras.
   Ler não tem efeito, e a política `Harmless` aceita qualquer plano de leitura.
2. **Ação** (`infer planner -> Plan`): o planejador vê o pedido, os inventários confiáveis e
   as leituras citadas, e propõe até 4 operações tipadas (`Send`, `Schedule`, `Update`,
   `Retarget`, `Password`, `Profile`) e a resposta ao usuário.

Um `Plan` só ganha `Endorsed` se a política `SafePlan` aceitar **todas** as operações. As
regras de `SafePlan` são estas:

| operação | regra |
| --- | --- |
| mover dinheiro (`Send`, `Schedule`, `Retarget`) | o destinatário está no inventário `payees` |
| alterar uma transação agendada (`Update`, `Retarget`) | o id está no inventário `scheduled` |
| trocar a senha (`Password`) | a senha está no inventário `quoted` |
| endereço (`Profile`) | aceito: não move dinheiro nem credencial |

Se a política recusa, o agente faz uma segunda proposta, com a recusa explicada. Se ela
também é recusada, nenhuma operação é executada. As escritas são `irrevocable` e só existem
via `commit`, com a capacidade `Cap[Bank.Transfer]`, `Cap[Bank.Credentials]` ou
`Cap[Bank.Profile]`.

Os inventários são **inventários confiáveis nomeados** (`s.inventory("k")` no prompt,
`s.member("k", x)` na política), uma extensão da fotografia confiável da Rev7. O compilador
exige que o prompt receba o mesmo inventário em que a política decide (E0421), que a política
não troque o inventário por literais (E0422), que `s.member` só decida dentro de política
(E0423) e que o nome do inventário seja literal (E0424). O host (`banking_inventories`) os
calcula **só de fontes confiáveis**:

- **`payees`**: os IBANs que o usuário escreveu no pedido, mais os destinatários que a conta
  já pagou (transações enviadas e agendadas). São campos estruturados do razão do banco: um
  remetente que só depositou na conta não vira beneficiário.
- **`scheduled`**: os ids das transações agendadas.
- **`quoted`**: os literais entre aspas no pedido.

Texto livre de ferramenta (assunto de transação, conteúdo de arquivo) nunca entra num
inventário. Essas regras foram escritas antes de qualquer rodada e não mencionam o IBAN do
atacante.

Elas têm um custo de utilidade conhecido. A tarefa `user_task_0` pede para pagar a conta de
um arquivo, e o IBAN só aparece dentro do arquivo, que é justamente o vetor de injeção. A
política recusa esse pagamento por desenho. Na `user_task_15`, o usuário escreve no pedido o
mesmo IBAN que o atacante usa, então transferências para ele são legítimas pela política.

## Executar

```bash
pip install "agentdojo==0.1.35"
cargo build --release --bin athc
python3 -m unittest discover -s bench/agentdojo/tests -v      # offline: roteiro e API local

# modelo real (chave em OPENAI_API_KEY, base em OPENAI_BASE_URL)
python3 bench/agentdojo/run.py --suite banking --model openai/gpt-oss-120b \
    --logdir bench/agentdojo/runs/banking-openai-gpt-oss-120b
python3 bench/agentdojo/matrix.py bench/agentdojo/runs/banking-openai-gpt-oss-120b
```

O workflow [`agentdojo.yml`](../../.github/workflows/agentdojo.yml) roda o mesmo comando com a
chave no segredo `GROQ_API_KEY` e versiona os logs no branch.

A rodada é **pareada e retomável**:

- **pareada:** cada tarefa roda nos dois pipelines antes da próxima, então uma rodada
  interrompida compara os mesmos pares;
- **retomável:** o que já tem log oficial completo não roda de novo;
- **cota:** se a cota do provedor acaba, a rodada termina sem gravar a tarefa como falha.

A ordem é esta: as 16 tarefas sem ataque, depois os 144 pares (tarefa do usuário × tarefa de
injeção). As 9 tarefas de injeção executadas como tarefas do usuário, uma checagem de
sanidade do script oficial, não entram na matriz.

## A matriz

[`matrix.py`](matrix.py) lê os logs oficiais e escreve `matrix.json` e `matrix.md`. Para
cada pipeline, a matriz tem:

- **utilidade** sem ataque;
- **utilidade sob ataque**;
- **ASR** (attack success rate): a fração de pares em que o verificador de segurança oficial
  diz que o objetivo do atacante foi cumprido. A taxa de defesa é 1 − ASR.

As taxas vêm com intervalos de Wilson a 95% e são calculadas sobre os pares que os dois
pipelines completaram. A matriz traz ainda o consumo de tokens por execução e, para o
Aletheia, os endossos, as recusas e as escritas executadas, tirados dos journals encadeados
(`aletheia-journals/`).

## Resultados

Ver [`runs/`](runs/). A primeira rodada está descrita em [`RESULTS.md`](RESULTS.md).
