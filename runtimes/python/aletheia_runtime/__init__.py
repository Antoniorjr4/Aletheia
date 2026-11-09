"""aletheia_runtime — segunda realização da semântica de Aletheia Core 0, em Python.

Programas emitidos por `athc emit --target py` rodam sobre este runtime e produzem o mesmo
journal que o ARX em Rust: as mesmas entradas, na mesma ordem, serializadas byte a byte como o
`serde_json` e encadeadas pelo mesmo BLAKE3. A suíte diferencial do repositório compara os dois
backends traço a traço (mesma cabeça de hash, mesmo resultado, mesmo mundo, mesmo orçamento).

Só a biblioteca padrão: BLAKE3 e a serialização canônica são implementados aqui.
"""

__version__ = "0.4.0"
