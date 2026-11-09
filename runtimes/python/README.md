# aletheia_runtime

Segunda realização da semântica de Aletheia Core 0, em Python e só com a biblioteca padrão.
Programas emitidos por `athc emit --target py` rodam sobre este runtime e produzem **o mesmo
journal que o ARX**: as mesmas entradas, na mesma ordem, os mesmos bytes e a mesma cabeça de hash.

```sh
athc emit examples/rollout.ath --target py --out rollout.py
PYTHONPATH=runtimes/python python3 -m aletheia_runtime run rollout.py \
    --world examples/fixtures/world.json --models examples/fixtures/models-rollout.json \
    --provider scripted --arg-json '"api"' --journal rollout-py.jsonl
athc verify-journal rollout-py.jsonl        # o auditor em Rust aceita a cadeia escrita em Python
```

| Módulo | Espelha |
| --- | --- |
| `runtime.py` | `arx::Arx`: handlers, ferramentas com capacidades conferidas em runtime, `commit`, orçamento por reserva, `infer`/`refine`/`sample`, contrato Δ e suas violações, sagas (`atomic`, compensações, `CompensationFailed`), `await`, `TimeoutUnknown`, reconciliação, `transfer` |
| `journal.py`, `canon.py`, `blake3.py` | `arx::journal` com a serialização do `serde_json` (chaves ordenadas, floats na grafia do `ryu`), `f64::round` do Rust e BLAKE3 em Python puro |
| `values.py` | `arx::Value`: igualdade, exibição, codificação JSON |
| `shape.py`, `mask.py` | `ath_grammar`: validação com as mesmas mensagens, decodificação, proposta canônica e o autômato de prefixo da máscara |
| `world.py`, `providers.py` | `arx::host::SimWorld` (falhas injetáveis), os provedores roteirizado e mascarado e o provedor `openai` (HTTP), com transporte gravador e de replay |
| `cassette.py` | `arx::cassette`: o formato `aletheia-cassette/1` e a primeira divergência entre journals |

## Provedor `openai`, gravação e replay

O provedor `openai` envia a mesma requisição que o ARX, byte a byte: o mesmo corpo, as mesmas
mensagens, o mesmo esquema no dialeto escolhido e a mesma política de novas tentativas para
`429`, `5xx` e `json_validate_failed`. A configuração vem do ambiente (`OPENAI_BASE_URL`,
`OPENAI_API_KEY`, `ALETHEIA_MODEL`, `ALETHEIA_CONSTRAINT`, `ALETHEIA_SCHEMA`, `ALETHEIA_REASONING_EFFORT`). A chave só vai
no cabeçalho `Authorization` e nunca é gravada.

```sh
python3 -m aletheia_runtime run rollout.py --provider openai --source examples/rollout.ath \
    --world examples/fixtures/world.json --arg-json '"api"' --record caso.cassette.json
python3 -m aletheia_runtime replay caso.cassette.json rollout.py   # exige o journal gravado
athc replay caso.cassette.json                                      # o mesmo cassete no ARX
```

`--source` embute o `.ath` no cassete, para que o ARX possa recompilá-lo. No replay, cada
requisição reconstruída tem de ser idêntica à gravada, e a espera de um `429` é contabilizada
sem dormir. Os cassetes reais da Groq estão em [`bench/replay/`](../../bench/replay/README.md).

A equivalência é verificada em `crates/athc/tests/differential.rs`, que roda no CI com
`cargo test`. Os testes de unidade deste diretório cobrem as peças isoladas:

```sh
cd runtimes/python && PYTHONPATH=. python3 -m unittest discover -s tests
```

Limites: a grafia de `show()` para textos com caracteres de controle Unicode
fora do ASCII é uma aproximação do `escape_debug` do Rust; essa grafia só entra em prompts com
campos não textuais.
