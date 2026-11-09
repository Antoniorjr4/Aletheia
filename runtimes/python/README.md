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
| `world.py`, `providers.py` | `arx::host::SimWorld` (falhas injetáveis) e os provedores roteirizado e mascarado |

A equivalência é verificada em `crates/athc/tests/differential.rs`, que roda no CI com
`cargo test`. Os testes de unidade deste diretório cobrem as peças isoladas:

```sh
cd runtimes/python && PYTHONPATH=. python3 -m unittest discover -s tests
```

Limites: o provedor `openai` (HTTP) ainda não foi portado, e os testes diferenciais usam
provedores deterministas. A grafia de `show()` para textos com caracteres de controle Unicode
fora do ASCII é uma aproximação do `escape_debug` do Rust; essa grafia só entra em prompts com
campos não textuais.
