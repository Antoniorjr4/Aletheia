Pares completos nos dois pipelines: 16 tarefas sem ataque · 94 pares sob ataque, de 11 tarefas de usuário.

| pipeline | indicador | k/n | IC95 por par (independência) | IC95 por tarefa (correlação total) | bootstrap por tarefa |
| --- | --- | --- | --- | --- | --- |
| `baseline-gpt-oss-120b` | utilidade | 11/16 | 44–86% | — | — |
| `baseline-gpt-oss-120b` | utilidade sob ataque | 50/94 | 43–63% | 43–90% (8/11 tarefas) | 29–76% |
| `baseline-gpt-oss-120b` | ASR | 68/94 | 63–80% | 52–95% (9/11 tarefas) | 48–92% |
| `aletheia-gpt-oss-120b` | utilidade | 12/16 | 50–90% | — | — |
| `aletheia-gpt-oss-120b` | utilidade sob ataque | 57/94 | 51–70% | 43–90% (8/11 tarefas) | 35–85% |
| `aletheia-gpt-oss-120b` | ASR | 0/94 | 0–4% | 0–26% (0/11 tarefas) | — |

| comparação pareada | só no 1º | só no 2º | McNemar exato (p) |
| --- | --- | --- | --- |
| utilidade (`baseline-gpt-oss-120b` × `aletheia-gpt-oss-120b`) | 4 | 5 | 1 |
| utilidade sob ataque (`baseline-gpt-oss-120b` × `aletheia-gpt-oss-120b`) | 9 | 16 | 0.2295 |
| ASR (`baseline-gpt-oss-120b` × `aletheia-gpt-oss-120b`) | 68 | 0 | 0 |

Classificação (todos os logs completos de cada pipeline):

- `baseline-gpt-oss-120b`: infra: log parcial sem veredito: 1; sem ataque: não cumprida · erro do modelo ou da tarefa: 5; sob ataque: ataque bem-sucedido: 68; sob ataque: não cumprida · erro do modelo ou da tarefa: 44
- `aletheia-gpt-oss-120b`: sem ataque: não cumprida · erro do modelo ou da tarefa: 4; sem ataque: rejeição pelo tipo (decodificador/refine): 1; sob ataque: não cumprida · erro do modelo ou da tarefa: 37; sob ataque: rejeição pelo tipo (decodificador/refine): 5
