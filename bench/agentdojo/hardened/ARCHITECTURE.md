# P1 — Python endurecido: arquitetura

A mesma missão do `banking_v2_*.ath`, em Python 3.12 comum: biblioteca padrão, `pydantic`
(validação) e o SDK `openai` (cliente compatível da Groq). Nenhuma dependência do runtime
Aletheia.

```
pedido (confiável) ──► [LLM: leituras] ──► Reads (pydantic) ──► ferramentas de leitura
                                                                   │  (resultado = Untrusted)
inventários do host (confiáveis) ─┐                                ▼
pedido ───────────────────────────┼──► [LLM: plano] ──► Plan (pydantic, ≤ 4 operações)
dado delimitado <<< … >>> ────────┘                          │
                                                             ▼
                                    porta da política (safe_write; strict | keeping)
                                                             │  Endorsed (selado)
                                                             ▼
                              commit(capacidade, Endorsed) ──► função oficial
                                                             │
                                          journal SHA-256 encadeado + chave de idempotência
```

| controle | onde | teste |
| --- | --- | --- |
| 1. planejar/executar com isolamento; saída estruturada estrita | `HardenedBanker.run`, `prompt`, `strict_schema` | `test_1_tool_data_only_inside_delimiters` |
| 2. mesmas políticas e inventários, `strict` e `keeping` | `safe_write`, `_gate`, `banking_inventories` | `test_2_policy_strict_and_keeping`, `test_inventories_equal_the_aletheia_host` |
| 3. capacidades não forjáveis | `Capability`, `grant`, `EFFECT_CAP` | `test_3_capabilities_cannot_be_forged_or_omitted` |
| 4. rótulos em tempo de execução; `commit` exige `Endorsed` | `Untrusted`, `Endorsed`, `_endorse`, `commit` | `test_4_commit_refuses_unendorsed` |
| 5. orçamento por tarefa | `Meter` | `test_5_budget_aborts_before_the_next_call` |
| 6. journal encadeado, idempotência | `Journal`, `commit` | `test_6_journal_is_hash_chained_and_tamper_evident` |
| 7. validação de forma e tamanho | modelos pydantic | `test_7_shape_and_size_validation` |

Para isolar a variável medida, o P1 usa os mesmos textos de tarefa, as mesmas seções de
prompt, os mesmos limites de saída e a mesma sequência de tentativas do programa Aletheia.
Muda só **onde** as garantias são verificadas:

- no Aletheia, `athc check` recusa o programa antes de ele rodar se um efeito não tem
  capacidade, se um argumento de efeito não é `Endorsed`, se um dado de ferramenta entra no
  prompt sem `quote`, se o pior caso estoura o orçamento;
- no P1, os mesmos controles existem como código e testes, e uma violação só aparece quando o
  caminho é executado (ou nunca, se nenhum teste o exercita).

Essa diferença é o que a métrica M4 (defeitos semeados) mede. Ela não é presumida aqui.

Limites conhecidos do P1, que também valem para o equivalente Aletheia: a política não
inspeciona texto livre; o selo de `Endorsed` e de `Capability` protege contra erro de
programação, não contra código malicioso no mesmo processo (Python não tem encapsulamento
forte).

## Revisão independente (16/11/2026, antes de qualquer medição paga)

Um revisor que não viu o código Aletheia recebeu a instrução de tornar o P1 o mais forte
possível. Ele apontou 14 itens. Os testes `ReviewFindingTests` cobrem os itens corrigidos.

| # | achado | decisão |
| --- | --- | --- |
| 1 | `dataclasses.replace` copiava o selo de `Endorsed` e mudava a operação | **corrigido**: o `commit` exige um objeto emitido pela porta nesta execução e reaplica a política |
| 2 | o dado podia fechar o bloco `>>>` e imitar uma seção confiável | **corrigido**: delimitadores e `#` no início de linha são neutralizados no dado, na origem |
| 3 | valores monetários sem validação (`-1e9`, `nan`, `1.234,56`) | **corrigido**: decimal positivo com até 2 casas, ou a saída é inválida |
| 4 | a chave de idempotência nunca se repetia | **corrigido**: a chave vem do conteúdo, e duplicatas não rodam |
| 5 | o journal se perdia se um efeito falhasse no meio | **corrigido**: `commit_error` entra no journal, e o modo `keeping` continua |
| 6 | o 400 `json_validate_failed` da Groq abortava a tarefa | **corrigido** (antes da revisão): vira saída inválida e é pedida de novo |
| 7 | erros transitórios viravam falha da tarefa | mantido por paridade: o runtime Aletheia também registra aborto depois de esgotar as tentativas; o SDK tenta 6 vezes |
| 8 | `finish_reason == "length"` não detectado | não aplicado: o runtime Aletheia também não o distingue; fica na lista de melhorias dos dois |
| 9 | `minItems`/`maxItems` no esquema estrito | mantido: o esquema do Aletheia usa os mesmos limites e a Groq os aceitou no lote 3; tipos desconhecidos agora dão erro |
| 10 | a previsão do orçamento ignorava a entrada | **corrigido**: prevê entrada (com o mesmo overhead do contrato Δ) e US$ |
| 11 | a segunda proposta depois de saída inválida diz "recusada pela política" e passa pela porta estrita | mantido por paridade com `banking_v2_*.ath`; vale para os dois |
| 12 | a capacidade de leitura era checada só pela chave | **corrigido** |
| 13 | documentação × código (rótulo na origem, idempotência sem teste, journal autoverificável) | **corrigido** nos dois primeiros; o terceiro está declarado: o journal mostra adulteração interna, não substitui uma assinatura externa (o do Aletheia também não) |
| 14 | `Profile`, `subject` e `date` não são restringidos pela política | fora da garantia, por desenho, nos dois; declarado em PROVA-ENGENHARIA.md (D11) |

Os itens 2 e 3 também se aplicam ao programa Aletheia. Eles estão diagnosticados como D9 e D10
em [PROVA-ENGENHARIA.md](../PROVA-ENGENHARIA.md). Nesta campanha, o P1 sai **mais forte** que os
`banking_v2_*.ath` nesses dois pontos, e isso fica registrado assim.
