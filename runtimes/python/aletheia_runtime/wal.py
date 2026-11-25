"""Write-ahead log das operações remotas com chave (Rev13, M-III): a mesma realização do
`arx::wal`. Antes do envio, a intenção (chave, ferramenta, argumentos, provedor, obrigação de
reconciliação); depois, o desfecho com evidência (`close`) ou a escalada (`escalate`). As sagas
abertas também ficam no log. Cada registro é uma linha encadeada por BLAKE3, sincronizada no
disco antes do passo seguinte; uma última linha rasgada é descartada na leitura."""

import os

from . import blake3
from .canon import dumps, loads
from .journal import GENESIS, Journal

CRASH = "aletheia: queda simulada do processo"


class Crash(BaseException):
    """A queda simulada: atravessa todo tratador do runtime (não é `Exception`)."""


class WalError(Exception):
    """O WAL existe e não pôde ser lido (`W0001`) ou tem uma linha inteira inválida, fora da
    cadeia ou adulterada (`W0002`). Nenhuma das duas falhas vira um log vazio."""

    def __init__(self, code: str, msg: str):
        super().__init__(f"{code}: {msg}")
        self.code = code


class Wal:
    def __init__(self, path: str | None = None):
        self.path = path
        self.log = Journal()
        self.crash = None  # (ponto, n)
        self.nth = 0
        self.ord: dict = {}
        self.fresh = True
        self.torn: bytes | None = None
        if path:
            try:
                with open(path, "rb") as f:
                    data = f.read()
            except FileNotFoundError:
                return
            except OSError as e:
                raise WalError("W0001", f"o WAL `{path}` existe e não pôde ser lido: {e}") from e
            self.fresh = False
            self.log, self.torn = Wal.read(data, path)
            if self.torn is not None:
                # a escrita que a queda cortou: guardada à parte, e o arquivo volta ao último
                # registro íntegro para que o próximo continue a cadeia
                with open(path + ".torn", "ab") as side:
                    side.write(self.torn + b"\n")
                    side.flush()
                    os.fsync(side.fileno())
                with open(path, "r+b") as f:
                    f.truncate(len(data) - len(self.torn))
                    f.flush()
                    os.fsync(f.fileno())

    @staticmethod
    def read(data: bytes, path: str = "<memória>"):
        """Cada linha terminada por `\\n` precisa ser um registro da cadeia; a primeira que não
        for é `W0002`. Só o resto sem `\\n` no fim é uma escrita interrompida, devolvida à parte."""
        j = Journal()
        prev = GENESIS
        *lines, rest = data.split(b"\n")
        for n, raw in enumerate(lines, 1):
            def bad(reason, n=n):
                return WalError(
                    "W0002",
                    f"o WAL `{path}` está corrompido na linha {n} ({reason}); "
                    f"{len(j.entries)} registro(s) íntegro(s) antes dela",
                )
            try:
                e = loads(raw.decode("utf-8"))
            except UnicodeDecodeError:
                raise bad("não é UTF-8") from None
            except ValueError:
                raise bad("não é JSON") from None
            if not isinstance(e, dict):
                raise bad("não é um registro")
            if "hash" not in e:
                raise bad("registro sem `hash`")
            body = {k: v for k, v in e.items() if k != "hash"}
            if body.get("prev") != prev:
                raise bad("`prev` não continua a cadeia")
            if blake3.hexdigest(dumps(body).encode("utf-8")) != e["hash"]:
                raise bad("o hash não confere com o conteúdo")
            j.entries.append(e)
            prev = e["hash"]
        j.head = prev
        return j, (rest if rest else None)

    def append(self, t: float, kind: str, detail: dict) -> None:
        self.log.push(t, kind, detail)
        if self.path:
            with open(self.path, "a", encoding="utf-8", newline="\n") as f:
                f.write(dumps(self.log.entries[-1]) + "\n")
                f.flush()
                os.fsync(f.fileno())

    def maybe_crash(self, at: str, key: int) -> None:
        if self.crash is not None and self.ord.get(key) == self.crash[1] and at == self.crash[0]:
            raise Crash(f"{CRASH} ({at} da operação {self.crash[1]})")

    def to_jsonl(self) -> str:
        return self.log.to_jsonl()


class WalState:
    def __init__(self):
        self.intents: list = []
        self.outcome: dict = {}
        self.sagas: dict = {}
        self.hashes: dict = {}

    def _saga(self, sid):
        return self.sagas.setdefault(sid, {"ended": False, "ops": [], "unlogged": [], "compensated": 0})

    @staticmethod
    def of(entries: list) -> "WalState":
        s = WalState()
        for e in entries:
            d = e.get("detail") or {}
            kind = e.get("kind")
            if kind == "intent":
                s.intents.append((d.get("key"), d))
                s.hashes[d.get("key")] = e.get("hash")
                if d.get("saga") is not None:
                    s._saga(d["saga"])["ops"].append(d)
            elif kind == "close":
                s.outcome[d.get("key")] = d.get("status")
            elif kind == "escalate":
                s.outcome[d.get("key")] = "escalated"
            elif kind == "saga_begin":
                s._saga(d.get("saga"))
            elif kind == "saga_op":
                sg = s._saga(d.get("saga"))
                if d.get("key") is None or not any(o.get("key") == d.get("key") for o in sg["ops"]):
                    sg["ops"].append(d)
            elif kind == "saga_unlog":
                s._saga(d.get("saga"))["unlogged"].append(d.get("key"))
            elif kind == "compensated":
                s._saga(d.get("saga"))["compensated"] += 1
            elif kind == "saga_end":
                s._saga(d.get("saga"))["ended"] = True
        return s

    def open(self) -> list:
        return [(k, d) for k, d in self.intents if k not in self.outcome]
