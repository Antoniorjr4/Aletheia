# P3 — esforço de mudança com pedidos cegos (critério 5 do Ciclo 1)

Pré-registro, escrito em 26/11/2026 **antes** de os pedidos existirem.

## Pergunta

Mudar o agente bancário custa, em Aletheia, no máximo 25% mais linhas alteradas que no Python
endurecido (P1)? Busca-se menos. A margem é a de `docs/CICLO-1.md`, fixada em 17/11.

## Procedimento

1. **Autor cego.** Um agente que recebe apenas [`FUNCIONAL.md`](FUNCIONAL.md) escreve 6 pedidos de
   mudança realistas de manutenção, cada um com 3 a 5 casos de aceitação no formato do documento e
   com uma "omissão plausível" (o passo que um implementador apressado esqueceria), descrita em
   termos funcionais. Não vê programas, linguagem, correções do ciclo nem resultados.
2. **Congelamento.** `pedidos.json` recebe hash SHA-256 e é versionado antes de qualquer implementação.
   Um pedido só é descartado se for impossível nas ferramentas do banco; o descarte é declarado.
3. **Implementadores separados.** Um agente implementa em Aletheia (`banking_v3_keeping.ath` +
   `aletheia_pipeline.py`), outro em Python (`hardened/banking_hardened.py` +
   `hardened/agentdojo_adapter.py`). Cada um trabalha numa cópia sem os arquivos do outro lado. Cada
   pedido parte da mesma base (`cbfe4d1`), não acumula.
4. **Pronto** = casos de aceitação do pedido passam em [`aceite.py`](aceite.py) **e** os testes
   existentes do lado continuam passando (ajuste de teste existente só quando o pedido muda o
   comportamento testado; contado à parte).
5. **Versão incompleta.** Cada implementador entrega também a versão com a omissão do pedido. Eu
   verifico, aplicando o diff, em que nível ela é detectada: `estático` (Aletheia: `athc check`;
   Python: `py_compile` + `pyflakes`), `testes existentes`, `casos de aceitação` ou `não detectado`.

## Métricas

- **Primária:** linhas alteradas (adicionadas + removidas, `git diff --numstat`) nos arquivos de
  produção de cada lado, somadas nos 6 pedidos. Critério: Aletheia ≤ 1,25 × P1.
- Secundárias: linhas sem comentário nem branco; arquivos tocados; execuções de teste até passar
  (relato do implementador); defeitos introduzidos (casos de aceitação ou testes que falham na entrega);
  nível de detecção da versão incompleta.

## Limites conhecidos antes de medir

Uma suíte, 6 pedidos, um implementador por lado, ambos o mesmo modelo de linguagem, que conhece Python
do treino e a Aletheia só pelo código da base e por `bench/m10/guides/aletheia.md` mais uma nota das
construções pós-M10. O isolamento dos agentes é por instrução e por cópia de trabalho, não por
sandbox. O P1 guarda valores como texto decimal; a Aletheia v3, como centavos inteiros (diferença
pré-existente, resultado do D10).

O resultado é registrado como sair. O critério não é reescrito depois.
