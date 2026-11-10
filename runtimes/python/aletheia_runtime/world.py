"""Mundo simulado (cluster + pager) com falhas injetáveis: réplica de `arx::host::SimWorld`."""

from __future__ import annotations

from .values import Adt, Key, is_int


class HostFailed(Exception):
    def __init__(self, msg: str):
        super().__init__(msg)
        self.msg = msg


class HostTimeout(Exception):
    def __init__(self, elapsed: float):
        super().__init__(elapsed)
        self.elapsed = elapsed


def _key(args):
    return next((a.n for a in args if isinstance(a, Key)), None)


def _target(args):
    adt = next((a for a in args if isinstance(a, Adt)), None)
    if adt is not None:
        get = dict(adt.fields).get
        d, to = get("deploy"), get("to")
        return (d if isinstance(d, str) else None, to if is_int(to) else None, adt.variant)
    text = next((a for a in args if isinstance(a, str)), None)
    num = next((a for a in args if is_int(a)), None)
    return text, num, None


class SimWorld:
    def __init__(self, spec: dict | None = None):
        spec = spec or {}
        self.latency = {k: float(v) for k, v in spec.get("latency", {}).items()}
        self.logs = {k: ("\n".join(v) if isinstance(v, list) else v) for k, v in spec.get("logs", {}).items()}
        self.replicas = dict(spec.get("replicas", {}))
        names = spec.get("managed", [])
        if not isinstance(names, list) or any(
            not isinstance(name, str) or not (1 <= len(name) <= 32)
            or name not in self.replicas for name in names
        ):
            raise ValueError("inventário `managed` inválido ou sem deployment correspondente")
        self.managed = sorted(set(names))
        if len(", ".join(self.managed)) > 256:
            raise ValueError("inventário gerenciado maior que 256 caracteres")
        cpu = spec.get("telemetry", {}).get("cpu_pct", {})
        if not isinstance(cpu, dict) or any(
            type(p) is not int or not (0 <= p <= 100) for p in cpu.values()
        ):
            raise ValueError("telemetry.cpu_pct inválido")
        self.cpu_pct = dict(cpu)
        self.initial = dict(self.replicas)
        self.health = dict(spec.get("health", {}))
        self.restarts: dict = {}
        self.outbox: list = []
        self.faults = []
        self.unknown_status = {}
        for f in spec.get("faults", []):
            kind = f["kind"]
            if kind == "unknown_status":
                self.unknown_status[f["tool"]] = int(f.get("times", 1))
            else:
                self.faults.append((f["tool"], f.get("nth"), kind))
        self.calls: dict = {}
        self.applied: dict = {}
        self.preimage: dict = {}
        # Ferramentas sumidouro: aceitam qualquer argumento, devolvem () e só registram a chamada.
        # Servem aos programas do núcleo gerados pelo teste diferencial triplo.
        self.sinks = [str(s) for s in spec.get("sinks", [])]
        # Inventários confiáveis nomeados: ordenados e sem repetição, como no ARX.
        self.inventories = {}
        for k, items in (spec.get("inventories") or {}).items():
            vals = []
            for x in items:
                if isinstance(x, bool) or not isinstance(x, (str, int)):
                    raise ValueError(f"item não textual no inventário `{k}`")
                vals.append(x if isinstance(x, str) else str(x))
            self.inventories[k] = sorted(set(vals))
        self.sunk: list = []

    def lat(self, tool: str) -> float:
        return self.latency.get(tool, 0.5)

    def _fault(self, tool: str, n: int):
        for t, nth, kind in self.faults:
            if t == tool and (nth is None or nth == n):
                return kind
        return None

    def _apply(self, tool: str, args: list):
        deploy, to, variant = _target(args)
        key = _key(args)

        def need(d):
            if d is None:
                raise HostFailed(f"`{tool}` sem deployment")
            return d

        if tool == "logs":
            d = need(deploy)
            return self.logs.get(d, f"(sem logs para {d})")
        if tool == "restart" or (tool == "apply" and variant == "Restart"):
            d = need(deploy)
            self.restarts[d] = self.restarts.get(d, 0) + 1
            return f"restart {d}"
        if tool == "scale" or (tool == "apply" and variant == "Scale"):
            d = need(deploy)
            if to is None:
                raise HostFailed("scale sem réplicas")
            frm = self.replicas.get(d, 0)
            self.replicas[d] = to
            if key is not None:
                self.preimage[key] = (d, frm)
            return f"scale {d} {frm}→{to}"
        if tool == "unscale":
            pre = self.preimage.pop(key, None) if key is not None else None
            if pre is None:
                return "unscale: nada a restaurar"
            d, before = pre
            now = self.replicas.get(d, 0)
            self.replicas[d] = before
            return f"unscale {d} {now}→{before}"
        if tool == "healthy":
            return self.health.get(need(deploy), True)
        if tool == "restored":
            d = need(deploy)
            return self.replicas.get(d) == self.initial.get(d)
        if tool == "page":
            self.outbox.append(next((a for a in args if isinstance(a, str)), ""))
            return None
        if tool in self.sinks:
            self.sunk.append((tool, list(args)))
            return None
        raise HostFailed(f"ferramenta `{tool}` sem ligação no host")

    def call(self, tool: str, args: list):
        lat = self.lat(tool)
        key = _key(args)
        if key is not None and (tool, key) in self.applied:
            return self.applied[(tool, key)], lat
        n = self.calls.get(tool, 0) + 1
        self.calls[tool] = n
        kind = self._fault(tool, n)
        if kind == "error":
            raise HostFailed(f"`{tool}` falhou no provedor")
        if kind == "timeout_lost":
            raise HostTimeout(lat * 4.0)
        if kind == "timeout_applied":
            out = self._apply(tool, args)
            if key is not None:
                self.applied[(tool, key)] = out
            raise HostTimeout(lat * 4.0)
        out = self._apply(tool, args)
        if key is not None:
            self.applied[(tool, key)] = out
        return out, lat

    def status(self, tool: str, key: int):
        lat = self.lat(tool)
        n = self.unknown_status.get(tool)
        if n is not None and n > 0:
            self.unknown_status[tool] = n - 1
            return ("unknown", None), lat
        if (tool, key) in self.applied:
            return ("done", self.applied[(tool, key)]), lat
        return ("not_applied", None), lat

    def trusted_snapshot(self) -> dict:
        return {"managed": list(self.managed), "replicas": dict(self.replicas),
                "cpu_pct": dict(self.cpu_pct), "inventories": {k: list(v) for k, v in self.inventories.items()}}

    def state(self) -> dict:
        return {"replicas": dict(self.replicas), "restarts": dict(self.restarts), "outbox": list(self.outbox)}
