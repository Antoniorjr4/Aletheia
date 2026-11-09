"""BLAKE3 (modo hash, saída de 32 bytes) em Python puro, seguindo a implementação de referência.

O journal do ARX encadeia entradas por BLAKE3; o runtime Python precisa do mesmo hash sem
dependências nativas. Desempenho não é o objetivo: um journal típico tem dezenas de entradas.
"""

from __future__ import annotations

_M = 0xFFFFFFFF
_IV = (0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19)
_PERM = (2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8)
_CHUNK_START, _CHUNK_END, _PARENT, _ROOT = 1, 2, 4, 8
_BLOCK_LEN, _CHUNK_LEN = 64, 1024


def _g(s: list, a: int, b: int, c: int, d: int, x: int, y: int) -> None:
    s[a] = (s[a] + s[b] + x) & _M
    v = s[d] ^ s[a]
    s[d] = ((v >> 16) | (v << 16)) & _M
    s[c] = (s[c] + s[d]) & _M
    v = s[b] ^ s[c]
    s[b] = ((v >> 12) | (v << 20)) & _M
    s[a] = (s[a] + s[b] + y) & _M
    v = s[d] ^ s[a]
    s[d] = ((v >> 8) | (v << 24)) & _M
    s[c] = (s[c] + s[d]) & _M
    v = s[b] ^ s[c]
    s[b] = ((v >> 7) | (v << 25)) & _M


def _compress(cv, words, counter: int, block_len: int, flags: int) -> list:
    s = [cv[0], cv[1], cv[2], cv[3], cv[4], cv[5], cv[6], cv[7],
         _IV[0], _IV[1], _IV[2], _IV[3], counter & _M, (counter >> 32) & _M, block_len, flags]
    m = list(words)
    for r in range(7):
        _g(s, 0, 4, 8, 12, m[0], m[1])
        _g(s, 1, 5, 9, 13, m[2], m[3])
        _g(s, 2, 6, 10, 14, m[4], m[5])
        _g(s, 3, 7, 11, 15, m[6], m[7])
        _g(s, 0, 5, 10, 15, m[8], m[9])
        _g(s, 1, 6, 11, 12, m[10], m[11])
        _g(s, 2, 7, 8, 13, m[12], m[13])
        _g(s, 3, 4, 9, 14, m[14], m[15])
        if r < 6:
            m = [m[i] for i in _PERM]
    for i in range(8):
        s[i] ^= s[i + 8]
        s[i + 8] ^= cv[i]
    return s


def _words(block: bytes) -> list:
    return [int.from_bytes(block[i:i + 4], "little") for i in range(0, 64, 4)]


class _Output:
    def __init__(self, cv, words, counter, block_len, flags):
        self.cv, self.words, self.counter, self.block_len, self.flags = cv, words, counter, block_len, flags

    def chaining_value(self) -> list:
        return _compress(self.cv, self.words, self.counter, self.block_len, self.flags)[:8]

    def root_bytes(self, n: int) -> bytes:
        out = bytearray()
        counter = 0
        while len(out) < n:
            w = _compress(self.cv, self.words, counter, self.block_len, self.flags | _ROOT)
            for x in w:
                out += x.to_bytes(4, "little")
            counter += 1
        return bytes(out[:n])


class _Chunk:
    def __init__(self, key, counter: int, flags: int):
        self.cv = list(key)
        self.counter = counter
        self.block = bytearray(64)
        self.block_len = 0
        self.blocks_compressed = 0
        self.flags = flags

    def __len__(self) -> int:
        return _BLOCK_LEN * self.blocks_compressed + self.block_len

    def _start(self) -> int:
        return _CHUNK_START if self.blocks_compressed == 0 else 0

    def update(self, data: bytes) -> None:
        i = 0
        while i < len(data):
            if self.block_len == _BLOCK_LEN:
                self.cv = _compress(self.cv, _words(bytes(self.block)), self.counter, _BLOCK_LEN,
                                    self.flags | self._start())[:8]
                self.blocks_compressed += 1
                self.block = bytearray(64)
                self.block_len = 0
            take = min(_BLOCK_LEN - self.block_len, len(data) - i)
            self.block[self.block_len:self.block_len + take] = data[i:i + take]
            self.block_len += take
            i += take

    def output(self) -> _Output:
        return _Output(self.cv, _words(bytes(self.block)), self.counter, self.block_len,
                       self.flags | self._start() | _CHUNK_END)


def _parent(left, right, key, flags) -> _Output:
    return _Output(key, list(left) + list(right), 0, _BLOCK_LEN, _PARENT | flags)


def digest(data: bytes, n: int = 32) -> bytes:
    key, flags = _IV, 0
    chunk = _Chunk(key, 0, flags)
    stack: list = []
    i = 0
    while i < len(data):
        if len(chunk) == _CHUNK_LEN:
            cv = chunk.output().chaining_value()
            total = chunk.counter + 1
            while total & 1 == 0:
                cv = _parent(stack.pop(), cv, key, flags).chaining_value()
                total >>= 1
            stack.append(cv)
            chunk = _Chunk(key, chunk.counter + 1, flags)
        take = min(_CHUNK_LEN - len(chunk), len(data) - i)
        chunk.update(data[i:i + take])
        i += take
    out = chunk.output()
    for cv in reversed(stack):
        out = _parent(cv, out.chaining_value(), key, flags)
    return out.root_bytes(n)


def hexdigest(data: bytes) -> str:
    return digest(data).hex()
