# Reanálise: compilador congelado × versão nova

Diagnóstico posterior às correções. Nenhum programa foi editado. Não diz como o modelo teria
respondido às mensagens novas.

| versões | compilam (congelado) | compilam (nova) |
|---|---|---|
| primeira (16) | 0 | 1 |
| intermediária (39) | 0 | 1 |
| final (16) | 9 | 9 |
| todas (71) | 9 | 11 |

## Transições por versão (primeiro diagnóstico, congelado → nova)

| transição | versões |
|---|---|
| P0001, mesmo primeiro diagnóstico | 11 |
| P0001 → P0001 (erro seguinte ficou visível) | 11 |
| compilava e compila | 9 |
| P0001, mesmo lugar, só a mensagem mudou | 8 |
| E0011, mesmo primeiro diagnóstico | 6 |
| E0008, mesmo primeiro diagnóstico | 5 |
| P0001 → E0011 (erro seguinte ficou visível) | 5 |
| P0001 → E0008 (erro seguinte ficou visível) | 4 |
| E0001, mesmo primeiro diagnóstico | 3 |
| E0500, mesmo primeiro diagnóstico | 2 |
| E0422, mesma recusa, o diagnóstico aponta outro lugar | 2 |
| P0001 → compila | 2 |
| P0001 → E0001 (erro seguinte ficou visível) | 1 |
| E0012, mesmo primeiro diagnóstico | 1 |
| P0001 → E0422 (erro seguinte ficou visível) | 1 |

## Por autoria

| autoria | versões | congelado | nova |
|---|---|---|---|
| access_control/rep1 | 5 | P0001 E0001 P0001 P0001 P0001 | P0001 E0001 P0001 E0008 E0001 |
| access_control/rep2 | 5 | P0001 P0001 P0001 P0001 P0001 | P0001 P0001 P0001 P0001 P0001 |
| contracts/rep1 | 4 | P0001 P0001 E0008 ok | P0001 P0001 E0008 ok |
| contracts/rep2 | 4 | P0001 P0001 E0011 ok | P0001 P0001 E0011 ok |
| fleet/rep1 | 4 | P0001 P0001 E0011 ok | P0001 E0011 E0011 ok |
| fleet/rep2 | 5 | P0001 P0001 E0008 E0012 E0500 | P0001 P0001 E0008 E0012 E0500 |
| maintenance/rep1 | 5 | P0001 P0001 P0001 E0001 P0001 | P0001 P0001 P0001 E0001 P0001 |
| maintenance/rep2 | 5 | P0001 P0001 E0011 E0500 ok | P0001 E0011 E0011 E0500 ok |
| sales_crm/rep1 | 5 | P0001 E0008 E0008 P0001 E0422 | E0008 E0008 E0008 E0422 E0422 |
| sales_crm/rep2 | 5 | P0001 P0001 P0001 E0008 E0422 | E0008 P0001 E0008 E0008 E0422 |
| shipping/rep1 | 3 | P0001 P0001 ok | ok ok ok |
| shipping/rep2 | 5 | P0001 E0011 P0001 P0001 ok | E0011 E0011 P0001 P0001 ok |
| storeroom/rep1 | 4 | P0001 E0001 E0011 ok | E0011 E0001 E0011 ok |
| storeroom/rep2 | 4 | P0001 P0001 E0011 ok | P0001 E0011 E0011 ok |
| travel_expenses/rep1 | 3 | P0001 P0001 ok | P0001 P0001 ok |
| travel_expenses/rep2 | 5 | P0001 P0001 P0001 P0001 P0001 | P0001 P0001 P0001 P0001 P0001 |

## Corpus versionado (130 programas fora das autorias)

Aceitos: 57 congelado, 57 nova. Programas com mudança de aceitação, códigos, raiz do Codex ou Python emitido: 0.
