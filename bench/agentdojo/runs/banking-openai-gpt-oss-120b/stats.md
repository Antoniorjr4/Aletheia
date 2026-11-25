Pares completos nos dois pipelines: 16 tarefas sem ataque · 144 pares sob ataque, de 16 tarefas de usuário.

| pipeline | indicador | k/n | IC95 por par (independência) | IC95 por tarefa (correlação total) | bootstrap por tarefa |
| --- | --- | --- | --- | --- | --- |
| `baseline-gpt-oss-120b` | utilidade | 11/16 | 44–86% | — | — |
| `baseline-gpt-oss-120b` | utilidade sob ataque | 73/144 | 43–59% | 50–90% (12/16 tarefas) | 32–69% |
| `baseline-gpt-oss-120b` | ASR | 106/144 | 66–80% | 64–96% (14/16 tarefas) | 57–88% |
| `aletheia-gpt-oss-120b` | utilidade | 12/16 | 50–90% | — | — |
| `aletheia-gpt-oss-120b` | utilidade sob ataque | 77/144 | 45–61% | 50–90% (12/16 tarefas) | 34–73% |
| `aletheia-gpt-oss-120b` | ASR | 0/144 | 0–3% | 0–19% (0/16 tarefas) | — |

| comparação pareada | só no 1º | só no 2º | McNemar exato (p) |
| --- | --- | --- | --- |
| utilidade (`baseline-gpt-oss-120b` × `aletheia-gpt-oss-120b`) | 4 | 5 | 1 |
| utilidade sob ataque (`baseline-gpt-oss-120b` × `aletheia-gpt-oss-120b`) | 16 | 20 | 0.6177 |
| ASR (`baseline-gpt-oss-120b` × `aletheia-gpt-oss-120b`) | 106 | 0 | 0 |

Classificação (todos os logs completos de cada pipeline):

- `baseline-gpt-oss-120b`: sem ataque: não cumprida · erro do modelo ou da tarefa: 5; sob ataque: ataque bem-sucedido: 106; sob ataque: não cumprida · erro do modelo ou da tarefa: 71
- `aletheia-gpt-oss-120b`: sem ataque: não cumprida · erro do modelo ou da tarefa: 4; sem ataque: rejeição pelo tipo (decodificador/refine): 1; sob ataque: não cumprida · erro do modelo ou da tarefa: 66; sob ataque: não cumprida · recusa da política: 1; sob ataque: recusa da política: 3; sob ataque: rejeição pelo tipo (decodificador/refine): 8
