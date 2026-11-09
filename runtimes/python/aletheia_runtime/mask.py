"""Autômato de prefixo viável e decodificação mascarada (`ath_grammar::mask`), com o mesmo
comportamento caractere a caractere. Otimização sem efeito no resultado: o conjunto completo de
continuações só é calculado onde o Rust o usa de fato (desvio e fallback sem candidato fixo).
"""

from __future__ import annotations

DEAD, PARTIAL, COMPLETE = "dead", "partial", "complete"
_OK, _EOF, _BAD = 0, 1, 2
ROOT_OPEN, ROOT_CLOSE = '{"value":', "}"


def _lit(s: str, i: int, w: str):
    j = i
    for wc in w:
        if j == len(s):
            return _EOF, j
        if s[j] != wc:
            return _BAD, j
        j += 1
    return _OK, j


def viable(shape: dict, prefix: str) -> str:
    st, i = _lit(prefix, 0, ROOT_OPEN)
    if st == _OK:
        st, i = _value(shape, prefix, i)
        if st == _OK:
            st, i = _lit(prefix, i, ROOT_CLOSE)
    if st == _OK:
        return COMPLETE if i == len(prefix) else DEAD
    return DEAD if st == _BAD else PARTIAL


def _value(sh: dict, s: str, i: int):
    t = sh["t"]
    if t == "bool":
        a = _lit(s, i, "true")
        if a[0] == _OK:
            return a
        b = _lit(s, i, "false")
        if b[0] == _OK:
            return b
        if a[0] == _EOF or b[0] == _EOF:
            return _EOF, i
        return _BAD, i
    if t == "int":
        return _int(s, i, sh.get("min"), sh.get("max"))
    if t == "text":
        return _text(s, i, sh.get("max"))
    if t == "list":
        return _list(sh["elem"], sh["min"], sh["max"], s, i)
    return _adt(sh["variants"], s, i)


def _text(s: str, i: int, mx):
    st, j = _lit(s, i, '"')
    if st != _OK:
        return st, j
    n = 0
    while True:
        if j == len(s):
            return _EOF, j
        c = s[j]
        if c == '"':
            return _OK, j + 1
        if c == "\\" or ord(c) < 0x20:
            return _BAD, j
        n += 1
        if mx is not None and n > mx:
            return _BAD, j
        j += 1


_I64_MIN, _I64_MAX = -(2**63), 2**63 - 1


def _canonical(neg: bool, d: str) -> bool:
    return (
        d != ""
        and len(d) <= 19
        and all("0" <= c <= "9" for c in d)
        and not (len(d) > 1 and d.startswith("0"))
        and not (neg and d == "0")
    )


def _split(run: str):
    return (True, run[1:]) if run.startswith("-") else (False, run)


def _int_complete(run: str, lo, hi) -> bool:
    neg, d = _split(run)
    if not _canonical(neg, d):
        return False
    v = -int(d) if neg else int(d)
    return (_I64_MIN if lo is None else lo) <= v <= (_I64_MAX if hi is None else hi)


def _int_prefix_viable(run: str, lo, hi) -> bool:
    lo = _I64_MIN if lo is None else lo
    hi = _I64_MAX if hi is None else hi
    if run == "":
        return lo <= hi
    neg, d = _split(run)
    if d == "":
        return lo <= -1
    if not _canonical(neg, d):
        return False
    v = int(d)
    if d == "0":
        return lo <= 0 <= hi
    for k in range(0, 19 - len(d) + 1):
        p = 10**k
        a, b = v * p, v * p + p - 1
        if neg:
            a, b = -b, -a
        if a <= hi and b >= lo:
            return True
    return False


def _int(s: str, i: int, lo, hi):
    j = i
    while j < len(s) and ("0" <= s[j] <= "9" or (j == i and s[j] == "-")):
        j += 1
    run = s[i:j]
    if j == len(s):
        return (_EOF, j) if _int_prefix_viable(run, lo, hi) else (_BAD, j)
    return (_OK, j) if _int_complete(run, lo, hi) else (_BAD, j)


def _adt(variants: list, s: str, i: int):
    st, j = _lit(s, i, '{"kind":"')
    if st != _OK:
        return st, j
    start = j
    while j < len(s) and s[j] != '"':
        j += 1
    name = s[start:j]
    if j == len(s):
        return (_EOF, j) if any(v["name"].startswith(name) for v in variants) else (_BAD, j)
    v = next((x for x in variants if x["name"] == name), None)
    if v is None:
        return _BAD, j
    j += 1
    for f, fs in v["fields"]:
        st, j = _lit(s, j, f',"{f}":')
        if st != _OK:
            return st, j
        st, j = _value(fs, s, j)
        if st != _OK:
            return st, j
    return _lit(s, j, "}")


def _list(elem: dict, mn: int, mx: int, s: str, i: int):
    st, j = _lit(s, i, "[")
    if st != _OK:
        return st, j
    if j == len(s):
        return _EOF, j
    if s[j] == "]":
        return (_OK, j + 1) if mn == 0 else (_BAD, j)
    if mx == 0:
        return _BAD, j
    n = 0
    while True:
        st, j = _value(elem, s, j)
        if st != _OK:
            return st, j
        n += 1
        if j == len(s):
            return _EOF, j
        c = s[j]
        if c == "]" and n >= mn:
            return _OK, j + 1
        if c == "," and n < mx:
            j += 1
            continue
        return _BAD, j


ALPHABET = [chr(b) for b in range(0x20, 0x7F)] + list("áàâãéêíóôõúçÁÀÂÃÉÊÍÓÔÕÚÇ")
_FALLBACK = '"}],:0123456789'


def allowed(shape: dict, prefix: str) -> list:
    return [c for c in ALPHABET if viable(shape, prefix + c) != DEAD]


class DecodeError(Exception):
    pass


def decode_following(shape: dict, proposal: str, limit: int):
    """Devolve (texto, desvio | None); desvio = (pos, querido | None, permitidos[:12], prefixo)."""
    out = ""
    k = 0
    deviation = None
    for pos in range(limit):
        if viable(shape, out) == COMPLETE:
            return out, deviation
        c = None
        if deviation is None and k < len(proposal) and viable(shape, out + proposal[k]) != DEAD:
            c = proposal[k]
            k += 1
        else:
            if deviation is None:
                ok = allowed(shape, out)
                if not ok:
                    raise DecodeError(f"gramática sem continuação após {out!r}")
                deviation = (pos, proposal[k] if k < len(proposal) else None, ok[:12], out)
            for p in _FALLBACK:
                if viable(shape, out + p) != DEAD:
                    c = p
                    break
            if c is None:
                ok = allowed(shape, out)
                if not ok:
                    raise DecodeError(f"gramática sem continuação após {out!r}")
                c = min(ok)
        out += c
    raise DecodeError(f"limite de {limit} caracteres atingido sem completar a saída")
