"""Journal append-only encadeado por BLAKE3, com a mesma serialização do ARX."""

from __future__ import annotations

from . import blake3
from .canon import dumps, f64_round

GENESIS = "0" * 64


class Journal:
    def __init__(self):
        self.entries: list = []
        self.head = GENESIS

    def push(self, t: float, kind: str, detail) -> None:
        body = {"seq": len(self.entries), "t": f64_round(t * 1000.0) / 1000.0, "kind": kind, "detail": detail,
                "prev": self.head}
        h = blake3.hexdigest(dumps(body).encode("utf-8"))
        entry = dict(body)
        entry["hash"] = h
        self.head = h
        self.entries.append(entry)

    def push_untimed(self, kind: str, detail) -> None:
        """Entrada sem carimbo de tempo: o formato do journal do núcleo."""
        body = {"seq": len(self.entries), "kind": kind, "detail": detail, "prev": self.head}
        h = blake3.hexdigest(dumps(body).encode("utf-8"))
        entry = dict(body)
        entry["hash"] = h
        self.head = h
        self.entries.append(entry)

    def to_jsonl(self) -> str:
        return "".join(dumps(e) + "\n" for e in self.entries)

    @staticmethod
    def verify(entries: list) -> int | None:
        """Índice da primeira entrada adulterada, ou `None` se a cadeia está íntegra."""
        prev = GENESIS
        for i, e in enumerate(entries):
            body = dict(e)
            h = body.pop("hash", None)
            if body.get("prev") != prev or h != blake3.hexdigest(dumps(body).encode("utf-8")):
                return i
            prev = h
        return None


def _is_cap(a) -> bool:
    return isinstance(a, dict) and len(a) == 1 and isinstance(a.get("cap"), list)


def core_view(entries: list) -> Journal:
    """A vista do núcleo λ_Ath de um journal: a mesma projeção de `arx::journal::Journal::core_view`.

    `tool` com capacidade → `act {args, cap, tool}` (sem capacidade é uma sonda do mundo e fica
    de fora); `endorse` → `endorse {accepted: true, policy, value}`; `refine_exhausted` →
    `endorse {accepted: false, policy, value: null}`; `await_timeout` → `fail {probe}`;
    `compensate` → `undo {args, ok, tool}` (o topo do registro da saga); `saga_recovered` e
    `compensation_failed` → `recover {ok, probe}`; `end` → `end {value}`. O registro da saga é
    reconstruído como no ARX: `compensable` empilha, `saga_commit` descarta as
    `compensable_ops` do topo, `compensate` desempilha. O avaliador do Lean produz esta mesma
    vista, byte a byte.
    """
    j = Journal()
    comp: list = []
    probe = (None, None)
    for e in entries:
        d, kind = e.get("detail") or {}, e.get("kind")
        if kind == "tool":
            cap, args = None, []
            for a in d.get("args") or []:
                if cap is None and _is_cap(a):
                    cap = a["cap"]
                else:
                    args.append(a)
            if cap is None:
                probe = (d.get("tool"), d.get("result"))
                continue
            if d.get("class") == "compensable":
                comp.append(list(args))
            j.push_untimed("act", {"args": args, "cap": cap, "tool": d.get("tool")})
        elif kind == "endorse":
            j.push_untimed("endorse", {"accepted": True, "policy": d.get("policy"), "value": d.get("value")})
        elif kind == "refine_exhausted":
            j.push_untimed("endorse", {"accepted": False, "policy": d.get("policy"), "value": None})
        elif kind == "await_timeout":
            j.push_untimed("fail", {"probe": probe[0]})
        elif kind == "saga_commit":
            n = int(d.get("compensable_ops") or 0)
            if n:
                del comp[max(0, len(comp) - n):]
        elif kind == "compensate":
            args = comp.pop() if comp else []
            j.push_untimed("undo", {"args": args, "ok": d.get("ok"), "tool": d.get("of")})
        elif kind in ("saga_recovered", "compensation_failed"):
            j.push_untimed("recover", {"ok": probe[1], "probe": probe[0]})
        elif kind == "end":
            j.push_untimed("end", {"value": d.get("result")})
    return j
