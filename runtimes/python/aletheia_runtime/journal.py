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
