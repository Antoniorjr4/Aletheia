"""Valores de Aletheia em Python, com a mesma igualdade, exibição e codificação JSON do ARX.

| Aletheia | Python                      |
|----------|-----------------------------|
| Int      | `int` (nunca `bool`)        |
| Bool     | `bool`                      |
| Text     | `str`                       |
| Unit     | `None`                      |
| List     | `list`                      |
| ADT      | `Adt`                       |
| Cap      | `Cap`                       |
| IdemKey  | `Key`                       |
| Draft    | `Draft`                     |
| Pending  | `Pending`                   |
"""

from __future__ import annotations

from .canon import rust_debug_str


class Adt:
    __slots__ = ("ty", "variant", "fields")

    def __init__(self, ty: str, variant: str, fields: list):
        self.ty, self.variant, self.fields = ty, variant, list(fields)

    def __repr__(self) -> str:
        return f"Adt({self.ty}.{self.variant}, {self.fields!r})"


class Cap:
    __slots__ = ("effects",)

    def __init__(self, effects):
        self.effects = frozenset(effects)


class Key:
    __slots__ = ("n",)

    def __init__(self, n: int):
        self.n = n


class Draft:
    __slots__ = ("model", "out", "shape", "max_bytes", "schema", "gbnf", "prompt")

    def __init__(self, model: str, out: str, spec: tuple, prompt: str):
        self.model, self.out, self.prompt = model, out, prompt
        self.shape, self.max_bytes, self.schema, self.gbnf = spec


class Pending:
    __slots__ = ("key", "tool", "args")

    def __init__(self, key: int, tool: str, args: list):
        self.key, self.tool, self.args = key, tool, list(args)


def is_int(v) -> bool:
    return type(v) is int


def veq(a, b) -> bool:
    """`PartialEq for Value` do ARX: capacidades, rascunhos e obrigações nunca são iguais."""
    if type(a) is bool or type(b) is bool:
        return type(a) is bool and type(b) is bool and a == b
    if is_int(a) and is_int(b):
        return a == b
    if isinstance(a, str) and isinstance(b, str):
        return a == b
    if a is None and b is None:
        return True
    if isinstance(a, Adt) and isinstance(b, Adt):
        return (
            a.variant == b.variant
            and len(a.fields) == len(b.fields)
            and all(fa == fb and veq(va, vb) for (fa, va), (fb, vb) in zip(a.fields, b.fields))
        )
    if isinstance(a, Key) and isinstance(b, Key):
        return a.n == b.n
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(veq(x, y) for x, y in zip(a, b))
    return False


def show(v) -> str:
    if type(v) is bool:
        return "true" if v else "false"
    if is_int(v):
        return str(v)
    if isinstance(v, str):
        one = v.replace("\n", " ⏎ ")
        short = one[:48]
        return rust_debug_str(short) + ("…" if len(one) > 48 else "")
    if v is None:
        return "()"
    if isinstance(v, Adt):
        if not v.fields:
            return v.variant
        return v.variant + " { " + ", ".join(f"{k}: {show(x)}" for k, x in v.fields) + " }"
    if isinstance(v, Cap):
        return "⟨cap⟩"
    if isinstance(v, Key):
        return f"k{v.n}"
    if isinstance(v, Draft):
        return f"⟨rascunho {v.model}⟩"
    if isinstance(v, Pending):
        return f"Pending[k{v.key}]({v.tool})"
    if isinstance(v, list):
        return "[" + ", ".join(show(x) for x in v) + "]"
    raise TypeError(f"valor desconhecido: {v!r}")


def prompt_text(v) -> str:
    return v if isinstance(v, str) else show(v)


def to_json(v):
    if type(v) is bool or is_int(v) or isinstance(v, str) or v is None:
        return v
    if isinstance(v, Adt):
        return {v.variant: {k: to_json(x) for k, x in v.fields}}
    if isinstance(v, Cap):
        return {"cap": sorted(v.effects)}
    if isinstance(v, Key):
        return f"k{v.n}"
    if isinstance(v, Draft):
        return {"draft": v.model}
    if isinstance(v, Pending):
        return {"pending": f"k{v.key}", "tool": v.tool}
    if isinstance(v, list):
        return [to_json(x) for x in v]
    raise TypeError(f"valor desconhecido: {v!r}")
