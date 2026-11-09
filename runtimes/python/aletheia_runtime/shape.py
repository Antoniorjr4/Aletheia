"""Formas de saída (emitidas pelo `athc` a partir dos tipos): validação estrita com as mesmas
mensagens do `ath_grammar::check`, decodificação para valores e a proposta canônica.

Uma forma é um dicionário: `{"t": "int", "min": a, "max": b}`, `{"t": "bool"}`,
`{"t": "text", "max": n}`, `{"t": "list", "elem": …, "min": a, "max": b}` ou
`{"t": "adt", "name": T, "variants": [{"name": V, "fields": [[campo, forma], …]}, …]}`.
"""

from __future__ import annotations

from .canon import I64_MAX, I64_MIN, dump_str, dumps, is_i64
from .values import Adt, is_int


def _show(b) -> str:
    return "" if b is None else str(b)


def check(shape: dict, doc) -> str | None:
    """`None` se o documento `{"value": …}` pertence à forma; senão a mensagem do ARX."""
    if not isinstance(doc, dict):
        return "a saída deve ser um objeto `{\"value\": …}`"
    if len(doc) != 1:
        return "a saída deve ter exatamente a chave `value`"
    if "value" not in doc:
        return "chave `value` ausente"
    return _check(shape, doc["value"], "value")


def _check(s: dict, v, path: str) -> str | None:
    t = s["t"]
    if t == "int":
        if not is_i64(v):
            return f"`{path}` deve ser inteiro"
        lo, hi = s.get("min"), s.get("max")
        if (lo is not None and v < lo) or (hi is not None and v > hi):
            return f"`{path}` = {v} fora do refinamento [{_show(lo)}..{_show(hi)}]"
        return None
    if t == "bool":
        return None if type(v) is bool else f"`{path}` deve ser booleano"
    if t == "text":
        if not isinstance(v, str):
            return f"`{path}` deve ser texto"
        m = s.get("max")
        if m is not None and len(v) > m:
            return f"`{path}` tem {len(v)} caracteres; o refinamento admite até {m}"
        return None
    if t == "list":
        if not isinstance(v, list):
            return f"`{path}` deve ser uma lista"
        if not (s["min"] <= len(v) <= s["max"]):
            return f"`{path}` tem {len(v)} elemento(s); o tipo admite de {s['min']} a {s['max']}"
        for i, x in enumerate(v):
            e = _check(s["elem"], x, f"{path}[{i}]")
            if e is not None:
                return e
        return None
    if t == "adt":
        name = s["name"]
        if not isinstance(v, dict):
            return f"`{path}` deve ser um objeto `{name}`"
        kind = v.get("kind")
        if not isinstance(kind, str):
            return f"`{path}.kind` ausente"
        var = next((x for x in s["variants"] if x["name"] == kind), None)
        if var is None:
            return f"`{kind}` não é variante de `{name}`"
        if len(v) != len(var["fields"]) + 1:
            return f"`{path}` deve ter exatamente os campos de `{kind}`"
        for f, fs in var["fields"]:
            if f not in v:
                return f"campo `{path}.{f}` ausente"
            e = _check(fs, v[f], f"{path}.{f}")
            if e is not None:
                return e
        return None
    raise ValueError(f"forma desconhecida {t}")


def from_json(s: dict, j):
    """Valor de um documento já validado (`arx::from_json`)."""
    t = s["t"]
    if t == "int":
        return j if is_i64(j) else 0
    if t == "bool":
        return j if type(j) is bool else False
    if t == "text":
        return j if isinstance(j, str) else ""
    if t == "list":
        return [from_json(s["elem"], x) for x in j] if isinstance(j, list) else []
    kind = j.get("kind", "") if isinstance(j, dict) else ""
    kind = kind if isinstance(kind, str) else ""
    var = next((x for x in s["variants"] if x["name"] == kind), None)
    fields = [(f, from_json(fs, j.get(f))) for f, fs in var["fields"]] if var else []
    return Adt(s["name"], kind, fields)


def fits(s: dict, v) -> bool:
    """O valor pertence à forma, inclusive aos refinamentos (`arx::fits`)?"""
    t = s["t"]
    if t == "int":
        lo, hi = s.get("min"), s.get("max")
        return is_int(v) and (lo is None or v >= lo) and (hi is None or v <= hi)
    if t == "bool":
        return type(v) is bool
    if t == "text":
        m = s.get("max")
        return isinstance(v, str) and (m is None or len(v) <= m)
    if t == "list":
        return isinstance(v, list) and s["min"] <= len(v) <= s["max"] and all(fits(s["elem"], x) for x in v)
    if t == "adt" and isinstance(v, Adt):
        var = next((x for x in s["variants"] if x["name"] == v.variant), None)
        return (
            var is not None
            and len(var["fields"]) == len(v.fields)
            and all(any(g == f and fits(fs, gv) for g, gv in v.fields) for f, fs in var["fields"])
        )
    return False


def proposal_from_json(shape: dict, j) -> str:
    """Codificação na ordem canônica da forma (`ath_grammar::proposal_from_json`)."""
    return '{"value":' + _loose(shape, j) + "}"


def _loose(shape, j) -> str:
    if isinstance(j, list):
        elem = shape["elem"] if shape is not None and shape["t"] == "list" else None
        return "[" + ",".join(_loose(elem, x) for x in j) + "]"
    if isinstance(j, dict):
        kind = j.get("kind") if isinstance(j.get("kind"), str) else None
        variant = None
        if shape is not None and shape["t"] == "adt" and kind is not None:
            variant = next((x for x in shape["variants"] if x["name"] == kind), None)
        parts = []
        if kind is not None:
            parts.append('"kind":' + dump_str(kind))
        seen = ["kind"]
        if variant is not None:
            for f, fs in variant["fields"]:
                if f in j:
                    parts.append(dump_str(f) + ":" + _loose(fs, j[f]))
                    seen.append(f)
        for k in sorted(j):
            if k not in seen:
                parts.append(dump_str(k) + ":" + _loose(None, j[k]))
        return "{" + ",".join(parts) + "}"
    return dumps(j)


def from_arg(s: dict, j):
    """Argumento de evento em JSON (formato `Value::to_json`) → valor."""
    t = s["t"]
    if t == "list":
        return [from_arg(s["elem"], x) for x in j]
    if t == "adt":
        (variant, fields), = j.items()
        var = next(x for x in s["variants"] if x["name"] == variant)
        return Adt(s["name"], variant, [(f, from_arg(fs, fields[f])) for f, fs in var["fields"]])
    return j


__all__ = ["check", "from_json", "fits", "proposal_from_json", "from_arg", "I64_MIN", "I64_MAX"]
