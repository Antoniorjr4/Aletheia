# M4: regressão de 27/11/2026 (não é medição nova)

Reexecução, sem custo e sem rede, dos executores congelados do M4:

- `run_m4.py`, com o catálogo do autor e as correções pós-auditoria;
- `run_blind.py`, com o catálogo cego;
- `run_pysa.py`, nos dois catálogos.

O compilador foi o do reparo de B1 e B2 (`f267d44`). Os reparos seguintes (F1–F3, G1–G3, H1) não
mudam as afirmações de obrigação nem o Python emitido de nenhum programa real
([bench/obrigacoes/regressao_corpus.json](../../../obrigacoes/regressao_corpus.json)), e a base do M4
não declara `requires`. A execução foi feita numa cópia do repositório, para não sobrescrever
[runs/m4](../m4/).

**Resultado: idêntico ao registro de 17/11** em estágio de detecção, códigos e efeito com o modelo
obediente, nos 21 + 4 casos do autor e nos 15 cegos. O Pysa também saiu idêntico: 1/21 no catálogo
do autor e 0/15 no cego. As únicas diferenças nos JSON são caminhos temporários nas mensagens de erro
de casos que já "não executavam" no registro original.

O banco de contratos da Rev12 (`bench/adversarial/harness.py --contracts`) não pôde ser
reexecutado. `banking_contracts.ath` é recusado com E0421 desde que essa regra entrou, também pelo
compilador congelado `223fb53`, e não foi alterado, por ser artefato histórico.
