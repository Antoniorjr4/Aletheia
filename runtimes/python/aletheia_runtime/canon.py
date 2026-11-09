"""Serialização canônica idêntica à do `serde_json` (o que o ARX hasheia) e aritmética de f64
com as mesmas regras do Rust.

- objetos com chaves em ordem (o `Map` do serde_json é um `BTreeMap`), sem espaços;
- floats como o `ryu` os formata (`3.3098843550000003`, `0.00009630000000000001`, `1e-7`,
  `1e+20`),
  sempre distintos de inteiros (`2.0`);
- strings em UTF-8 cru, escapando só `"`, `\\` e caracteres de controle.
"""

from __future__ import annotations

import math

_ESC = {'"': '\\"', "\\": "\\\\", "\n": "\\n", "\r": "\\r", "\t": "\\t", "\b": "\\b", "\f": "\\f"}


def dump_str(s: str) -> str:
    out = ['"']
    for ch in s:
        e = _ESC.get(ch)
        if e is not None:
            out.append(e)
        elif ord(ch) < 0x20:
            out.append("\\u%04x" % ord(ch))
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def _digits(x: float) -> tuple[str, int]:
    """Dígitos significativos mínimos (os mesmos do `repr` e do ryu) e o expoente k: x = d × 10^k."""
    r = repr(x)
    if "e" in r:
        mant, exp = r.split("e")
        e = int(exp)
    else:
        mant, e = r, 0
    if "." in mant:
        ip, fp = mant.split(".")
    else:
        ip, fp = mant, ""
    d = (ip + fp).lstrip("0")
    k = e - len(fp)
    while len(d) > 1 and d.endswith("0"):
        d = d[:-1]
        k += 1
    return d, k


def dump_float(x: float) -> str:
    if math.isnan(x) or math.isinf(x):
        return "null"
    if x == 0.0:
        return "-0.0" if math.copysign(1.0, x) < 0 else "0.0"
    sign = "-" if x < 0 else ""
    d, k = _digits(abs(x))
    n = len(d)
    kk = n + k
    if 0 <= k and kk <= 16:
        body = d + "0" * k + ".0"
    elif 0 < kk <= 16:
        body = d[:kk] + "." + d[kk:]
    elif -5 < kk <= 0:
        body = "0." + "0" * (-kk) + d
    else:
        e = kk - 1
        exp = ("e+" if e > 0 else "e") + str(e)
        body = (d if n == 1 else d[0] + "." + d[1:]) + exp
    return sign + body


def dumps(v) -> str:
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, float):
        return dump_float(v)
    if isinstance(v, str):
        return dump_str(v)
    if isinstance(v, (list, tuple)):
        return "[" + ",".join(dumps(x) for x in v) + "]"
    if isinstance(v, dict):
        return "{" + ",".join(dump_str(k) + ":" + dumps(v[k]) for k in sorted(v)) + "}"
    raise TypeError(f"valor não serializável: {v!r}")


def _reject_constant(name: str):
    raise ValueError(f"constante JSON inválida: {name}")


def loads(s: str):
    """`serde_json::from_str`: rejeita NaN/Infinity, como o Rust."""
    import json

    return json.loads(s, parse_constant=_reject_constant)


def f64_round(x: float) -> float:
    """`f64::round` do Rust: metade para longe de zero (o `round` do Python arredonda para o par)."""
    if x < 0:
        return -f64_round(-x)
    f = math.floor(x)
    return float(f + 1) if x - f >= 0.5 else float(f)


I64_MIN, I64_MAX = -(2**63), 2**63 - 1


def wrap_i64(n: int) -> int:
    """Aritmética `wrapping_*` de i64."""
    return ((n - I64_MIN) % 2**64) + I64_MIN


def is_i64(v) -> bool:
    return type(v) is int and I64_MIN <= v <= I64_MAX


_PRINTABLE_EXEMPT = {" "}


def rust_debug_str(s: str) -> str:
    """`format!("{s:?}")` para texto: aspas e escapes de `char::escape_debug`."""
    import unicodedata

    out = ['"']
    for ch in s:
        if ch == "\t":
            out.append("\\t")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\0":
            out.append("\\0")
        elif ch not in _PRINTABLE_EXEMPT and unicodedata.category(ch) in ("Cc", "Cf", "Co", "Cn", "Cs", "Zl", "Zp", "Zs"):
            out.append("\\u{%x}" % ord(ch))
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def est_tokens(s: str) -> int:
    """Estimativa do Marco Zero: ⌈caracteres / 4⌉."""
    return (len(s) + 3) // 4
