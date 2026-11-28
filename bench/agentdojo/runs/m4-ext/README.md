# M4-ext: resultado (27/11/2026)

Pré-registro em [defects/ext/PROTOCOLO.md](../../defects/ext/PROTOCOLO.md), versionado antes da
execução (`0cd0bf0`). Execução local, sem rede e sem custo.

| caso | Aletheia | P1 (ruff + mypy, testes) | Pysa | efeito em execução (304 execuções do modelo obediente por lado) |
| --- | --- | --- | --- | --- |
| X1: `return` decidido por dado de ferramenta antes dos efeitos irrevogáveis | estático (E0412) | não detectado | não detectado | sem recusa e sem diferença da base nos dois lados: o marcador não aparece nos dados da suíte |
| X2: destinatário de uma operação aprovada e valor de outra | não detectado | não detectado | não detectado | sem recusa e sem diferença da base nos dois lados: nenhuma tarefa aprova dois `Send` juntos |

**Auditoria das causas.** No X1, o E0412 aponta o `if` mutado (linha 144, "ramo decidido por dado
Untrusted") e o efeito irrevogável depois dele, e é a classe pretendida. O X2 compilou nos dois
lados e não houve falha incidental.

**Limites.** Dois casos, uma missão, escritos pelo autor (não cegos). Mede detecção, não dano: os
dois defeitos ficaram latentes na suíte.
