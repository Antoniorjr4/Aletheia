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


def core_view(entries: list, compensable=()) -> Journal:
    """A vista do núcleo λ_Ath de um journal: a mesma projeção de `arx::journal::Journal::core_view_with`.

    `tool` com capacidade → `act {args, cap, tool}` (sem capacidade é uma sonda do mundo e fica
    de fora); `endorse` → `endorse {accepted: true, policy, value}`; `refine_exhausted` →
    `endorse {accepted: false, policy, value: null}`; `await_timeout` → `fail {probe}`;
    `compensate` → `undo {args, ok, tool}` (o topo do registro da saga); `saga_recovered` e
    `compensation_failed` → `recover {ok, probe}`; `end` → `end {value}`. O registro da saga é
    reconstruído como no ARX: `compensable` empilha, `saga_commit` descarta as
    `compensable_ops` do topo, `compensate` desempilha. O avaliador do Lean produz esta mesma
    vista, byte a byte.

    O protocolo de chaves (Rev11; `mint` → `mint {key}` desde a Rev12): `tool` com chave → `act` (sem a chave nos argumentos) e
    `settle {key, status: "done"}`; `tool_failed` → `settle {key, status: "failed"}`;
    `timeout_unknown` → `pending {key, tool}` (e, se a ferramenta está em `compensable`, a
    operação entra no registro); `reconcile` → `query {attempt, key, status}`; `resolved` →
    `settle {key, status}`; `transfer` → `transfer {key}` e `settle {key, status:
    "transferred"}`; `saga_abort` por `R0600`/`R0611` → `fail {cause: "refused", key}` e pela
    obrigação transferida → `fail {cause: "abort"}`; `compensate` com chave → `undo {ok, tool}`;
    `abort` por `R0600`/`R0611` → `abort {cause: "refused", key}`; `end` com
    `aborted_by_handler` → `end {aborted: true, value}`.
    """
    compensable = set(compensable)
    j = Journal()
    comp: list = []
    probe = (None, None)
    refused = None
    for e in entries:
        d, kind = e.get("detail") or {}, e.get("kind")
        if kind == "tool":
            cap, args = None, []
            key = d.get("key")
            key_seen = key is None
            for a in d.get("args") or []:
                if cap is None and _is_cap(a):
                    cap = a["cap"]
                elif not key_seen and a == key:
                    key_seen = True
                else:
                    args.append(a)
            if cap is None:
                probe = (d.get("tool"), d.get("result"))
                continue
            if d.get("class") == "compensable":
                comp.append(list(args))
            j.push_untimed("act", {"args": args, "cap": cap, "tool": d.get("tool")})
            if key is not None:
                j.push_untimed("settle", {"key": key, "status": "done"})
        elif kind == "tool_failed":
            refused = d.get("key")
            j.push_untimed("settle", {"key": d.get("key"), "status": "failed"})
        elif kind == "mint":
            j.push_untimed("mint", {"key": d.get("key")})
        elif kind == "timeout_unknown":
            if d.get("tool") in compensable:
                comp.append([])
            j.push_untimed("pending", {"key": d.get("key"), "tool": d.get("tool")})
        elif kind == "reconcile":
            j.push_untimed("query", {"attempt": d.get("attempt"), "key": d.get("key"), "status": d.get("status")})
        elif kind == "resolved":
            if d.get("status") == "failed":
                refused = d.get("key")
                # `R0611`: a operação refutada sai da pilha de compensações
                if d.get("tool") in compensable and comp:
                    comp.pop()
            j.push_untimed("settle", {"key": d.get("key"), "status": d.get("status")})
        elif kind == "transfer":
            j.push_untimed("transfer", {"key": d.get("key")})
            j.push_untimed("settle", {"key": d.get("key"), "status": "transferred"})
        elif kind == "saga_abort":
            reason = d.get("reason") or ""
            if reason.startswith("R0600 ") or reason.startswith("R0611 "):
                j.push_untimed("fail", {"cause": "refused", "key": refused})
            elif not reason.startswith("R"):
                j.push_untimed("fail", {"cause": "abort"})
        elif kind == "abort" and d.get("code") in ("R0600", "R0611"):
            j.push_untimed("abort", {"cause": "refused", "key": refused})
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
            if d.get("key") is not None:
                j.push_untimed("undo", {"ok": d.get("ok"), "tool": d.get("of")})
            else:
                j.push_untimed("undo", {"args": args, "ok": d.get("ok"), "tool": d.get("of")})
        elif kind in ("saga_recovered", "compensation_failed"):
            j.push_untimed("recover", {"ok": probe[1], "probe": probe[0]})
        elif kind == "end":
            if d.get("aborted_by_handler") is True:
                j.push_untimed("end", {"aborted": True, "value": d.get("result")})
            else:
                j.push_untimed("end", {"value": d.get("result")})
    return j
