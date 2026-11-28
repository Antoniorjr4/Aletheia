# B1 e B2: reproduções da análise de obrigações

Os dois defeitos foram achados pela revisão independente do lote pós-M11
([bench/m11/diagnostico/REVISAO-INDEPENDENTE.md](../m11/diagnostico/REVISAO-INDEPENDENTE.md)).
Os dois vêm de antes: o compilador congelado `223fb53` também os tem.

- **B1:** a análise trata argumentos de elementos diferentes da mesma lista endossada como se fossem
  do mesmo elemento.
- **B2:** em `let q = refine … keeping … as fora`, a análise não liga `fora`, e uma ligação externa
  com o mesmo nome empresta sua origem.

`casos/` tem as reproduções, os controles (situações em que a origem comum é comprovável), os
mundos e os roteiros. `relatorio.py` roda cada caso no compilador dado e nos dois runtimes, sem rede
e sem modelo real. `antes.json` foi gerado **antes do reparo**, com o compilador de `e79c04b`.
