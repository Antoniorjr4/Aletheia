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

    `tool` → `act {args, cap, tool}`; `endorse` → `endorse {accepted: true, policy, value}`;
    `refine_exhausted` → `endorse {accepted: false, policy, value: null}`; `end` → `end {value}`.
    O resto não é evento do núcleo. O avaliador do Lean produz esta mesma vista, byte a byte.
    """
    j = Journal()
    for e in entries:
        d, kind = e.get("detail") or {}, e.get("kind")
        if kind == "tool":
            cap, args = None, []
            for a in d.get("args") or []:
                if cap is None and _is_cap(a):
                    cap = a["cap"]
                else:
                    args.append(a)
            j.push_untimed("act", {"args": args, "cap": cap, "tool": d.get("tool")})
        elif kind == "endorse":
            j.push_untimed("endorse", {"accepted": True, "policy": d.get("policy"), "value": d.get("value")})
        elif kind == "refine_exhausted":
            j.push_untimed("endorse", {"accepted": False, "policy": d.get("policy"), "value": None})
        elif kind == "end":
            j.push_untimed("end", {"value": d.get("result")})
    return j
