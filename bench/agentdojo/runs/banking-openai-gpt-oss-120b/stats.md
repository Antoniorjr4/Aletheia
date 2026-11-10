Pares completos nos dois pipelines: 16 tarefas sem ataque · 20 pares sob ataque, de 3 tarefas de usuário.

| pipeline | indicador | k/n | IC95 por par (independência) | IC95 por tarefa (correlação total) | bootstrap por tarefa |
| --- | --- | --- | --- | --- | --- |
| `baseline-gpt-oss-120b` | utilidade | 11/16 | 44–86% | — | — |
| `baseline-gpt-oss-120b` | utilidade sob ataque | 3/20 | 5–36% | 21–94% (2/3 tarefas) | 0–50% |
| `baseline-gpt-oss-120b` | ASR | 16/20 | 58–92% | 44–100% (3/3 tarefas) | 67–100% |
| `aletheia-gpt-oss-120b` | utilidade | 12/16 | 50–90% | — | — |
| `aletheia-gpt-oss-120b` | utilidade sob ataque | 6/20 | 15–52% | 6–79% (1/3 tarefas) | 0–67% |
| `aletheia-gpt-oss-120b` | ASR | 0/20 | 0–16% | 0–56% (0/3 tarefas) | — |

| comparação pareada | só no 1º | só no 2º | McNemar exato (p) |
| --- | --- | --- | --- |
| utilidade (`baseline-gpt-oss-120b` × `aletheia-gpt-oss-120b`) | 4 | 5 | 1 |
| utilidade sob ataque (`baseline-gpt-oss-120b` × `aletheia-gpt-oss-120b`) | 1 | 4 | 0.375 |
| ASR (`baseline-gpt-oss-120b` × `aletheia-gpt-oss-120b`) | 16 | 0 | 3.1e-05 |
