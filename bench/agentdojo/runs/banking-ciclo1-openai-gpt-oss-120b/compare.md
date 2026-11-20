# banking · ciclo 1: v2 (runtime corrigido) × v3 (centavos) × P1

Pares completos em todos os pipelines: 16/16 sem ataque · 129/144 sob ataque.

| pipeline | utilidade | utilidade sob ataque | ASR | tokens/execução | US$/execução |
| --- | --- | --- | --- | --- | --- |
| Aletheia v2 keeping | 68.8% (11/16; IC95 44–86%) | 55.0% (71/129; IC95 46–63%) | 0.0% (0/129; IC95 0–3%) | 1849 | 0.000416 |
| Aletheia v3 keeping (centavos) | 62.5% (10/16; IC95 39–82%) | 65.1% (84/129; IC95 57–73%) | 0.0% (0/129; IC95 0–3%) | 1885 | 0.000422 |
| P1 Python endurecido (keeping) | 50.0% (8/16; IC95 28–72%) | 50.4% (65/129; IC95 42–59%) | 0.0% (0/129; IC95 0–3%) | 1805 | 0.000395 |

| comparação pareada (McNemar exato) | indicador | só A | só B | p |
| --- | --- | --- | --- | --- |
| Aletheia v2 keeping (A) × Aletheia v3 keeping (centavos) (B) | utilidade | 2 | 1 | 1.0 |
| Aletheia v2 keeping (A) × Aletheia v3 keeping (centavos) (B) | utilidade sob ataque | 6 | 19 | 0.0146 |
| Aletheia v2 keeping (A) × Aletheia v3 keeping (centavos) (B) | ataque cumprido | 0 | 0 | 1.0 |
| Aletheia v2 keeping (A) × P1 Python endurecido (keeping) (B) | utilidade | 3 | 0 | 0.25 |
| Aletheia v2 keeping (A) × P1 Python endurecido (keeping) (B) | utilidade sob ataque | 12 | 6 | 0.2379 |
| Aletheia v2 keeping (A) × P1 Python endurecido (keeping) (B) | ataque cumprido | 0 | 0 | 1.0 |
| Aletheia v3 keeping (centavos) (A) × P1 Python endurecido (keeping) (B) | utilidade | 3 | 1 | 0.625 |
| Aletheia v3 keeping (centavos) (A) × P1 Python endurecido (keeping) (B) | utilidade sob ataque | 28 | 9 | 0.0026 |
| Aletheia v3 keeping (centavos) (A) × P1 Python endurecido (keeping) (B) | ataque cumprido | 0 | 0 | 1.0 |

| pipeline | journal | operações executadas | abortos | US$ |
| --- | --- | --- | --- | --- |
| Aletheia v2 keeping | {'prune': 1, 'decode_reject': 2, 'refine_exhausted': 2, 'endorse': 291} | {'send_money': 47, 'update_scheduled_transaction': 16, 'schedule_transaction': 20, 'update_user_info': 11, 'update_password': 4} | nenhum | 0.060737 |
| Aletheia v3 keeping (centavos) | {'decode_reject': 3, 'refine_exhausted': 3, 'endorse': 291} | {'send_money': 50, 'update_scheduled_transaction': 21, 'update_user_info': 10, 'update_password': 3, 'schedule_transaction': 13} | nenhum | 0.061542 |
| P1 Python endurecido (keeping) | {'prune': 3, 'decode_reject': 3, 'endorse': 145} | {'send_money': 40, 'update_scheduled_transaction': 12, 'update_user_info': 11, 'update_password': 1, 'schedule_transaction': 22} | nenhum | 0.057301 |

Chamadas ao modelo por execução não entram na tabela: o runtime Aletheia conta cada 429 de tokens por minuto como chamada, e o SDK `openai` do P1 os repete sem contar.

Ataques cumpridos:
- Aletheia v2 keeping: nenhum
- Aletheia v3 keeping (centavos): nenhum
- P1 Python endurecido (keeping): nenhum
