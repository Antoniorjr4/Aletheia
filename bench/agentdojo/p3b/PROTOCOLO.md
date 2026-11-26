# P3b — esforço de mudança com pedidos cegos novos, depois das capacidades pós-P3

Pré-registro, 26/11/2026, escrito antes de os pedidos existirem. O P3 (78 × 50 linhas, +56%, critério
não atendido) fica como registrado em [`../p3/PROTOCOLO.md`](../p3/PROTOCOLO.md).

## Pergunta

Com o compilador congelado em `bench/m11/FREEZE.json` (commit `223fb53`), mudar o agente bancário custa
em Aletheia no máximo 25% mais linhas alteradas que no Python endurecido (P1)? Mesma margem do P3.

## Igual ao P3

Descrição funcional (`../p3/FUNCIONAL.md`), executor neutro (`../p3/aceite.py`, versão corrigida),
agentes de partida (`banking_v3_keeping.ath` + `aletheia_pipeline.py`; `hardened/banking_hardened.py` +
`hardened/agentdojo_adapter.py`), métricas (linhas de produção alteradas; secundárias: sem comentário,
arquivos, tentativas, testes editados, detecção da versão incompleta), um implementador por lado, cada
pedido a partir da mesma base, sem acumular. O mesmo desvio declarado: o teste de paridade de
inventários do P1 com o host da Aletheia sai da cópia do lado P.

## Diferente do P3

- **Pedidos novos (D1–D6)** de um autor cego novo, que só lê `FUNCIONAL.md`; congelados com hash antes
  das implementações.
- **Referência de linguagem para cada implementador:** o guia v2 congelado do seu lado
  (`bench/m11/guides/aletheia.md` ou `python.md`), mais `hardened/ARCHITECTURE.md` do lado P. No P3 o
  lado A recebeu o guia do M10 e uma nota das construções novas.
- **Onde a regra ficou:** registro, por pedido, de se alguma decisão do pedido foi para o host. Regra
  no host não é proibida (é o recurso legítimo quando a linguagem não alcança), mas é relatada.

## Critério

Primário: linhas de produção alteradas somadas nos 6 pedidos, Aletheia ≤ 1,25 × P1. O resultado é
registrado como sair; o critério não é reescrito. Limites: os do P3, mais o fato de as capacidades
novas terem sido desenhadas a partir dos pedidos do P3 (por isso os pedidos aqui são novos).

## Registro

- 26/11: pedidos congelados, `pedidos.json` sha256 `a6fc9506c4d810732ab093a4f9214989855c4db2571fd3f02dbc2e8e93906fc4`.
  Nenhum descartado. Nas bases sem mudança, 12 dos 27 casos já passam nos dois lados (os de
  "tudo permitido"); os outros 15 exigem a mudança.
- **Coincidência declarada:** D1 (`Reschedule`) tem o mesmo tema de C1 do P3, com regra diferente
  (validação de data de calendário). O autor cego é outro e não viu o P3.

## Resultado (26/11/2026)

Conferido por mim, aplicando cada diff sobre a base ([`runs/p3b/verificacao.json`](../runs/p3b/verificacao.json)).
As 12 entregas completas aplicam e passam nos casos de aceitação (27/27 de cada lado) e nos testes do lado.

**Critério primário: ATENDIDO.** Aletheia 83 linhas, P1 102: **0,81 × P1** (margem: ≤ 1,25). Sem
comentário nem branco: 66 × 80 (0,83).

| pedido | Aletheia (+/−) | P1 (+/−) | tentativas A / P | regra fora do programa Aletheia |
| --- | --- | --- | --- | --- |
| D1 `Reschedule` com data de calendário | 19/1 | 27/3 | 2 / 1 | não |
| D2 teto de 500,00 para destinatário novo | 16/7 | 18/5 | 6 / 1 | não (lista nova no host, como o pedido manda) |
| D3 teto de 2.000,00 sobre o plano, recusa todas as transferências | 9/4 | 13/1 | 2 / 1 | não (no handler, sobre valores endossados) |
| D4 no máximo dois `Send`, em ordem | 11/2 | 20/2 | 2 / 1 | não |
| D5 assunto só com caracteres SEPA | 9/1 | 10/1 | 2 / 1 | **sim: host recusa na chamada** |
| D6 endereço só com rua e cidade citadas | 3/1 | 1/1 | 2 / 1 | não |
| **total** | **83** | **102** | **16 / 6** | 1 de 6 |

Secundárias: linhas de teste editadas 14 × 14; arquivos de produção tocados por pedido, Aletheia 1–2,
P1 1. Tentativas: 16 × 6 (a Aletheia precisou de mais execuções do compilador e dos casos; duas das
seis de D2 foram de um script de edição que falhou, segundo o implementador).

Versão incompleta: nenhuma das 12 é pega estaticamente pela razão certa. O `pyflakes` acusou a D1
incompleta do P1 por um `import` que sobrou, não pela omissão. Todas as 12 são pegas pelos casos de
aceitação; D2 e D4 incompletas da Aletheia também pelos testes existentes.

### Leitura

Pela primeira vez a Aletheia fica abaixo do Python na manutenção, com pedidos que ninguém do lado da
linguagem viu antes do congelamento. As construções pós-P3 apareceram onde se esperava: D4 é um
`keeping` com a lista dos aceitos (11 linhas contra 20), D1 usa as funções de texto. Contra a
Aletheia, e registrado:

- **D5 foi para o host.** A política não expressa "todo caractere pertence a um conjunto": as funções de
  texto contam letras e dígitos ASCII, mas não testam um conjunto arbitrário. Lacuna real.
- **D3 ficou no handler**, não na política: a política de elemento só vê os anteriores, e a de plano
  decide tudo ou nada. O resultado é correto e usa só valores endossados, mas a regra não está no
  ponto de endosso.
- **A D4 incompleta não pôde ser uma omissão pequena:** com a lista de aceitos, "contar também os
  recusados" exige reestruturar. É a linguagem tornando o erro difícil, mas também quer dizer que a
  versão incompleta mede outra coisa do lado Aletheia.
- **Tentativas 16 × 6.** Mais linhas no Python, mais iterações na Aletheia.

Limites: um implementador por lado, o mesmo modelo, 6 pedidos, uma suíte. Conta uma medição; não
estabelece a vantagem em geral. O P3 (78 × 50, contra) continua registrado.
