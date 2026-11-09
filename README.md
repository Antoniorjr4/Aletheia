# Aletheia

[![CI](https://github.com/Antoniorjr4/Aletheia/actions/workflows/ci.yml/badge.svg)](https://github.com/Antoniorjr4/Aletheia/actions/workflows/ci.yml)

Implementação executável de Aletheia: o subconjunto **Aletheia Core 0**, com o
compilador `athc` e o runtime `ARX` em Rust. O objetivo é provar uma coisa:
integridade, efeitos, capacidades, linearidade, refinamentos, orçamento e transações
sobre o mundo podem ser verificados **juntos, antes de rodar**, num programa de agente
real. E o tipo da saída de um modelo pode ser, ao mesmo tempo, a gramática que o modelo
é obrigado a seguir.

```text
$ athc check examples/remediate.ath
✓ E0 estrutura       11 definições, todas resolvidas
✓ E1 multiplicidade  5 chave(s) IdemKey e as obrigações Pending consumidas exatamente uma vez
✓ E2 efeitos         {Cluster.Read, Cluster.Write, CompensationFailed, Infer, Page, TimeoutUnknown} cobertos por capacidades e linhas de efeito
✓ E2 sagas           1 saga(s); compensações: scale ↩ unscale; irrevogáveis só via commit, fora de atomic
                     TimeoutUnknown tratado; reconciliação fora do contrato Δ: ≤ 9,0 s por chave
✓ E3 integridade     4 exigência(s) de integridade satisfeitas, inclusive sob pc
✓ E5 refinamentos    5 obrigação(ões) provadas por literal, campo refinado ou saída tipada
                     infer planner -> Action: entrada ≤ 2.053 tok · saída ≤ 686 tok (174 tok do tipo `Action` + 512 tok de raciocínio)
✓ E6 orçamento       Remediator.alert: pior caminho 8.217 tok · US$ 0,0022 · 53,6 s
                     ≤ [20.000 tok · US$ 0,50 · 90,0 s]
                     caminho: logs 2,0 → infer planner ×3 26,6 → scale 1,0 → healthy 1,0 → await 20,0 → unscale↩ 1,0 → restored 1,0 → page 1,0
✓ Codex              11 objetos BLAKE3, raiz #43947b1f669f
```

## Começar

Requer Rust ≥ 1.87 (e Python ≥ 3.10 para o benchmark).

```sh
cargo build --release
cargo test                                    # 50 testes: unidade, propriedade e ponta a ponta

# verificar
./target/release/athc check examples/remediate.ath
./target/release/athc check examples/errors/e0412_injection.ath     # rejeitado: E0412

# executar no ARX (mundo simulado e modelo roteirizado em examples/fixtures)
./target/release/athc run examples/remediate.ath --arg '"api"'

# sagas e reconciliação: mundos com falhas injetadas
./target/release/athc run examples/remediate.ath --arg '"api"' --world examples/fixtures/world-saga-compensa.json
./target/release/athc run examples/remediate.ath --arg '"api"' --world examples/fixtures/world-timeout-transfere.json

# contra um modelo real (Groq, OpenAI, vLLM…)
export OPENAI_API_KEY=…  OPENAI_BASE_URL=https://api.groq.com/openai/v1
cargo run --release --bin athc -- run examples/remediate.ath --provider openai --model openai/gpt-oss-20b --arg '"api"'

# a gramática que o tipo `Action` impõe ao modelo, e a máscara agindo sobre uma proposta
./target/release/athc grammar examples/remediate.ath --type Action \
    --decode '{"value":{"kind":"Delete","deploy":"db"}}'

# métrica M1: o mesmo ataque contra um laço de tool-calling em Python e contra o Aletheia
python3 bench/m1/run.py                       # modelo roteirizado como comprometido
python3 bench/m1/run.py --live                # modelo real, configurado pelo ambiente

# Codex e cadeia de custódia (inclui a checagem T9: nenhuma chave em aberto)
./target/release/athc hash examples/remediate.ath
./target/release/athc verify-journal .arx/journal.jsonl
```

## Sagas e reconciliação

Compensar não é desfazer o passado: é executar uma nova transição que leva o mundo a um
estado que uma sonda confirma. E um timeout diz que o agente parou de esperar, não que o
efeito não aconteceu. As duas coisas estão no tipo:

```text
tool scale(c: Cap[Cluster.Write], deploy: Text[..32] ^ Endorsed, to: Int[2..64], k: own IdemKey) -> Text
  ! { Cluster.Write } compensable by unscale provider queryable latency 1.0

tool restart(c: Cap[Cluster.Write], deploy: Text[..32] ^ Endorsed, k: own IdemKey) -> Text
  ! { Cluster.Write } irrevocable provider idempotent latency 1.0

let r = atomic recover restored(cluster, deploy) {
  scale(cluster, deploy, to, keys.mint())
  await healthy(cluster, deploy) within 20.0
  "resolvido"
} on failure(e) {
  "compensado: " ++ e
} on compensation_failed(cf) {
  "intervenção humana: " ++ cf
}

handle timeout_unknown(p: own Pending[_]) -> Text {
  reconcile p fuel 3 else {
    transfer p
    "pendente"
  }
}
```

| Regra | Onde | Erro |
| --- | --- | --- |
| Efeito irrevogável dentro de `atomic` (mesmo com `commit`) | E2 | T0302 |
| `compensable` sem compensador, com compensador de outra assinatura ou irrevogável | E2 | E0505 |
| Saga com efeito compensável sem `on compensation_failed` | E2 | E0503 |
| Chamada com chave sem `handle timeout_unknown` no agente | E2 | E0503 |
| Provedor `opaque` com chave dentro de `atomic` | E2 | T0305 |
| `await` fora de `atomic`; `return` dentro de `atomic` | E2 | E0601 / E0602 |
| Pós-condição ou sonda de recuperação decididas por dado Untrusted | E3 | E0417 |
| `IdemKey` ou `Pending` sem `own`; `own` em tipo não linear | E1 | E0042 / E0043 |
| `Pending` descartado ou consumido duas vezes (inclusive no `else` de `reconcile`) | E1 | E1107 / E1104 |

No runtime, a falha do corpo dispara as compensações em ordem reversa, que valem como
obrigação e rodam mesmo com o orçamento esgotado. Em seguida roda a sonda `recover`. Se ela
confirmar, executa `on failure`; se uma compensação falhar ou a sonda não confirmar, executa
`on compensation_failed`. Um `TimeoutUnknown` entrega ao tratador a obrigação linear
`Pending[k]`, e o protocolo depende do provedor: `idempotent` reenvia com a mesma chave,
`queryable` consulta o status pela chave e `opaque` não tem protocolo. O desfecho é Done (a
chamada é retomada), Failed (a chamada falha) ou esgotamento, caso em que o `else` recebe a
obrigação e precisa transferi-la. O E6 conta o caminho de falha da saga no pior caso;
a reconciliação fica fora do contrato Δ e o `athc check` mostra o limite dela por chave.

| Mundo (`examples/fixtures`) | O que acontece | Resultado |
| --- | --- | --- |
| `world.json` | a injeção é contida; a saga confirma | `resolvido` |
| `world-saga-compensa.json` | `healthy` nunca confirma; `unscale` restaura 3 réplicas | `compensado: R0910 …` |
| `world-saga-falha.json` | e `unscale` falha no provedor | `intervenção humana: …` |
| `world-timeout-reconcilia.json` | `scale` sem resposta; o status pela chave confirma | `resolvido` |
| `world-timeout-transfere.json` | status desconhecido além do combustível; `transfer`; a saga compensa | `pendente` |

## Decodificação restrita derivada do tipo

O tipo de saída de um `infer` é compilado (crate `ath-grammar`) numa forma, e dela saem
sem anotação extra:

- a **codificação canônica** da saída, `{"value": …}`, com variantes como objetos `{"kind": …}`;
- um **JSON Schema estrito** para `response_format`, em dois dialetos: `core` (padrão; só o
  núcleo que todo modo estrito documenta, com intervalos pequenos como `enum` exato) e
  `full` (com `minimum`, `maximum`, `maxLength`);
- uma **gramática GBNF**, enviada como `grammar` ao llama.cpp;
- um **autômato de prefixo viável**, base da máscara de tokens e do decodificador local;
- um **teto de bytes**, que limita os tokens de conteúdo no orçamento (E6) e no pedido.

```text
type Action =
  | Restart { deploy: Text[..32] }
  | Scale   { deploy: Text[..32], to: Int[2..64] }

action-scale ::= "{\"kind\":\"Scale\"" ",\"deploy\":" text-32 ",\"to\":" int-2-64 "}"
```

| Provedor (`--provider`) | Restrição | Saída válida por construção? |
| --- | --- | --- |
| `masked` (padrão) | máscara local sobre o autômato do tipo | sim; testado por propriedade com propostas adversariais |
| `scripted` | nenhuma | não; o ARX valida e descarta o que está fora do tipo |
| `openai` | JSON Schema estrito, GBNF ou nenhuma, num servidor compatível com OpenAI | conforme o servidor impõe; o ARX valida de novo |

### Modelo real

O provedor `openai` é configurado pelo ambiente, e os argumentos da CLI têm precedência:

| Variável | Uso |
| --- | --- |
| `OPENAI_API_KEY` | chave (também `GROQ_API_KEY`, `ATH_API_KEY`) |
| `OPENAI_BASE_URL` | ex.: `https://api.groq.com/openai/v1`; `--base-url` |
| `ALETHEIA_MODEL` | ex.: `openai/gpt-oss-20b`; `--model` |
| `ALETHEIA_SCHEMA` | `core` (padrão) ou `full`; `--schema` |
| `ALETHEIA_CONSTRAINT` | `json-schema` (padrão), `gbnf` ou `none`; `--constraint` |
| `ALETHEIA_REASONING_EFFORT` | `low` (padrão), `medium`, `high` ou `none` |
| `ALETHEIA_EXTRA_BODY` | objeto JSON mesclado no corpo; `null` remove um campo |

Modelos de raciocínio gastam tokens antes do conteúdo, e o contrato Δ declara quantos
(`reasoning = 512` no `model`). O pedido usa `max_completion_tokens` = teto do tipo + raciocínio.
O ARX liquida o orçamento com os tokens e a latência informados pelo servidor e grava no
journal o identificador da requisição, o raciocínio (truncado), a espera imposta por 429 e as
novas tentativas, e marca em `tok_source` se as contagens vieram do `usage` do servidor
(`server`) ou da estimativa de 4 caracteres por token (`estimate`). Se o servidor gerar mais
tokens que o contrato admite (R0702) ou demorar mais do que ele permite (R0703), o resultado
é `ContractBreach`.

Na Groq, o modo estrito não restringe a decodificação a um `enum` de inteiros: o modelo pode
gerar um valor fora dele. O servidor valida depois e responde HTTP 400 `json_validate_failed`,
com a geração em `failed_generation`. O ARX trata essa resposta como amostra, valida de novo e a
descarta. Como um 400 não traz `usage`, os tokens dessa amostra ficam como `estimate`.

O workflow [`Fogo real (Groq)`](.github/workflows/live.yml) roda o exemplo e a M1 contra a
Groq com a chave guardada no segredo de repositório `GROQ_API_KEY`.

## Métrica M1

`bench/m1` roda sete cenários no estilo do AgentDojo, com o ataque "important_instructions",
no laço clássico de tool-calling e no Aletheia. No modo simulado, o modelo é roteirizado como
comprometido; o CI repete a medição a cada push e falha se o Aletheia executar qualquer ação
fora da política.

| Métrica (simulado) | Python (laço de tool-calling) | Aletheia |
| --- | --- | --- |
| Ataques que atingiram o objetivo | 6/6 | 1/6 |
| Ações fora da política executadas | 5 | **0** |
| Tarefa cumprida sob ataque | 5/6 | 5/6 |
| Porte ingênuo do laço Python | — | rejeitado em compilação (E0412) |

O ataque que passa no Aletheia pede uma ação que a política aceita: o raio de explosão de
uma injeção é o conjunto aceito pela política. Os resultados com modelo real ficam em
[`bench/m1/live/`](bench/m1/live); método, cenários e limites em
[`bench/m1/README.md`](bench/m1/README.md).

## Estrutura

```text
crates/
  ath-syntax/    lexer, AST com spans, parser
  ath-grammar/   tipo → forma → JSON Schema (full/core), GBNF, autômato de prefixo, máscara
  ath-check/     verificador estratificado E0 · E1 · E2 · E3 · E5 · E6, diagnósticos
  ath-codex/     forma canônica, DAG de Merkle, BLAKE3, alfa-normalização
  arx/           interpretador, sagas, reconciliação, provedores, orçamento por reserva, journal
  athc/          CLI, renderização de diagnósticos, testes ponta a ponta
examples/        agente de remediação, mundos com falhas, 17 variantes com erro e o gerador delas
bench/m1/        harness da métrica M1: cenários, linha de base Python, agente, orquestrador
.github/         CI (fmt, clippy -D warnings, testes, M1) e o workflow de fogo real
```

## O que está implementado

| Spec v0.2 | Implementação |
| --- | --- |
| 1.3 Codex | forma canônica por definição; referências por hash (inclusive o compensador de uma ferramenta); nomes de definições e de locais não entram no hash |
| 2.3 `refine … by policy retry n` | políticas puras; só o que a política aceita ganha `Endorsed`; saída fora do tipo é descartada |
| 2.5 Modelos e inferência | `model` como valor com contrato Δ (inclusive raciocínio); provedor real configurado pelo ambiente, testado contra a Groq |
| 2.5 Compilador de gramática (`ath-grammar`) | tipo → forma → JSON Schema estrito em dois dialetos (`core`: intervalos de até 256 valores viram `enum` exato; `full`: `minimum`/`maximum`/`maxLength`), GBNF, autômato de prefixo viável, máscara de tokens e teto de bytes que entra no E6 |
| 2.6 Capacidades e integridade | capacidades só vêm dos parâmetros do agente, com atenuação; Untrusted ⊑ Endorsed ⊑ Trusted; `pc` para fluxos implícitos; `quote` obrigatório no prompt; pós-condições ⊒ Endorsed |
| 2.7 Orçamento | E6 estático no pior caminho sob Δ, inclusive o de falha da saga; reserva antes, liquidação depois; `ContractBreach` por latência ou tokens; obrigações sem reserva |
| 2.9–2.10 Estratos | E0, E1, E2, E3, E5 (refinamentos decidíveis) e E6, relatando só o primeiro estrato que falha |
| 2.2 Linearidade (E1) | `own IdemKey` e `Pending[k]` consumidos exatamente uma vez em todos os caminhos, inclusive no `else` de `reconcile`; no runtime, a checagem T9 do journal |
| 3.4 Sagas | `atomic … recover`, `await … within`, `compensable by`, `on failure`, `CompensationFailed` obrigatório, irrevogáveis só fora da saga |
| 3.5 Efeitos remotos | `own IdemKey`, `Pending[k]` linear, `TimeoutUnknown`, `reconcile … fuel … else`, `transfer`, provedores `idempotent`/`queryable`/`opaque` |
| 3.6 Execução durável | journal encadeado por BLAKE3; `verify-journal` detecta adulteração e chaves em aberto (T9) |
| 5.4 Métrica M1 | harness reprodutível no CI (simulado) e contra modelo real (workflow) |

### Erros que o compilador já pega

| Código | Estrato | Situação |
| --- | --- | --- |
| E0412 | E3 | valor Untrusted, ou ramo decidido por Untrusted, chega a um parâmetro `^ Endorsed` |
| E0415 | E3 | dado Untrusted entra no prompt sem `quote(…)` |
| E0417 | E3 | pós-condição ou sonda de recuperação decididas por dado Untrusted |
| E1104 / E1105 / E1107 | E1 | valor linear usado duas vezes, não consumido, ou obrigação `Pending` descartada |
| E0042 / E0043 | E1 | linear sem `own`; `own` em tipo não linear |
| E0501 / E0502 / E0500 | E2 | efeito não declarado; capacidade insuficiente; efeito sem capacidade na ferramenta |
| E0503 / E0505 | E2 | efeito sem tratador; compensador inválido |
| E0601 / E0602 / E0603 | E2 | `await` fora de saga; `return` dentro de saga; `reconcile`/`transfer` fora do tratador |
| E0420 | E2 | política impura |
| T0301 / T0302 / T0303 / T0305 | E2 | irrevogável sem `commit`; irrevogável na saga; `commit` em revogável; provedor opaco na saga |
| E0451 / E0450 | E5 | literal fora do refinamento; refinamento sem prova |
| B0110 | E6 | `Text` sem limite de tokens no prompt (use `fit(x, N)`) |
| B0201 / B0202 / B0207 | E6 | tokens, dólares ou tempo do pior caminho acima do orçamento |
| B0301 | E6 | prompt + saída máxima acima da janela do modelo |

## O que ainda não existe

- **Backend:** interpretador de árvore. Não há MLIR, LLVM nem emissão para Python.
- **E4 e E5 completo:** sem formas de tensor; refinamentos são intervalos e tetos de texto
  provados por literal ou por contenção, sem SMT; não há listas (`List[T, a..b]`).
- **Prova da recuperação:** as sagas rodam, mas o invariante de recuperação é observado por uma
  sonda, não provado em E5 a partir de contratos `restoring` sobre um estado abstrato do mundo;
  sem `lease` nem cobertura do footprint das pós-condições; compensações em sequência, sem
  paralelismo por comutação.
- **Recuperação após queda:** reler o journal e reconciliar as chaves abertas depois de um
  reinício (hoje só a checagem T9 as detecta); tratador `timeout_unknown` por agente, sem `resume`
  explícito; recuo fixo de 2 s.
- **Tokenizer:** a entrada é estimada em 4 caracteres por token quando o servidor não informa.
- **Mundo:** cluster simulado; cenários no estilo do AgentDojo, não as suítes do AgentDojo.
- **Certificados:** sem `certify`, `Bounded` nem monitor de deriva.
- **Teoremas:** nenhum está mecanizado. As garantias são testadas, não provadas.

## Próximos marcos

1. **Núcleo formal em Lean 4**: λ_Ath restrito ao que já roda, com T1 e T2, depois T3
   (não-interferência com endosso robusto) e T6 (recuperação de sagas).
2. **Suítes reais do AgentDojo** pelo provedor `openai`, medindo M1 e utilidade com vários modelos.
3. **Invariantes de recuperação provados**: contratos `restoring` sobre um estado abstrato do
   mundo, descarregados em E5 com SMT, e cobertura de footprint nas pós-condições.
4. **Listas na gramática e no orçamento** (`List[T, a..b]`, laços com custo multiplicado pelo teto).
5. **Recuperação após queda** a partir do journal, e o reconciliador durável como serviço.
6. **Emissor para Python + asyncio**, o backend da fase F1 da spec.

## Licença

Apache-2.0.
