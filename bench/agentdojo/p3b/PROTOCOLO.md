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
