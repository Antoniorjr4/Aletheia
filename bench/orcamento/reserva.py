"""Reserva de saldo ANTES de cada requisição paga.

O livro-caixa do M10/M11 (`bench/m10/author.py`, congelado) confere o teto antes de cada chamada,
mas só debita o custo depois dela. Uma chamada que começa com saldo de US$ 0,0033 pode custar
US$ 0,0045. Foi o que aconteceu: US$ 0,4210 gastos contra um teto de 0,42. Além disso, o cliente da
OpenAI faz até 4 tentativas internas (`max_retries=4`) que o livro não vê.

Aqui a ordem se inverte:

1. antes de enviar, calcula o **pior caso** da requisição: tokens de entrada por cota superior
   (bytes UTF-8 do conteúdo + folga por mensagem) e tokens de saída = `max_completion_tokens`;
2. se o saldo livre (teto − liquidado − reservas abertas) não cobre o pior caso, recusa **sem enviar**;
3. senão grava a reserva em disco (fsync) e só então envia;
4. depois liquida pelo uso informado. Sem uso informado (erro, queda, tempo esgotado), a reserva
   inteira fica como gasta, porque o provedor pode ter cobrado;
5. cada tentativa é uma requisição: as tentativas automáticas do cliente ficam desligadas
   (`max_retries=0`) e as repetições passam de novo pelo passo 1.

Uma reserva sem liquidação encontrada ao reabrir o livro (o processo morreu no meio) conta como
gasta pelo pior caso.

Hipótese declarada: o provedor não cobra saída acima de `max_completion_tokens` (o raciocínio está
incluído nesse limite). Se a liquidação mostrar uso maior que a reserva, o livro registra o excesso,
marca a hipótese como violada e recusa todas as requisições seguintes.
"""

from __future__ import annotations

import fcntl
import json
import os
import time
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path

# folga por mensagem (papel, delimitadores do modelo de chat) e por requisição
TOK_POR_MENSAGEM = 16
TOK_POR_REQUISICAO = 64


class SemSaldo(RuntimeError):
    """O saldo livre não cobre o pior caso da requisição; nada foi enviado."""


class HipoteseViolada(RuntimeError):
    """O uso informado passou da reserva; o livro recusa as requisições seguintes."""


def cota_entrada(messages: list[dict]) -> int:
    """Cota superior de tokens de entrada: um token cobre pelo menos um byte do texto."""
    n = TOK_POR_REQUISICAO
    for m in messages:
        c = m.get("content") or ""
        if not isinstance(c, str):
            c = json.dumps(c, ensure_ascii=False)
        n += len(c.encode("utf-8")) + len(str(m.get("role", "")).encode("utf-8")) + TOK_POR_MENSAGEM
    return n


@dataclass(frozen=True)
class Precos:
    entrada: float  # US$ por milhão de tokens
    saida: float

    def usd(self, tin: int, tout: int) -> float:
        return (tin * self.entrada + tout * self.saida) / 1e6


class Orcamento:
    """Livro-caixa com reservas, persistido em JSON lines e protegido por trava de arquivo."""

    def __init__(self, caminho: Path, teto_usd: float, precos: Precos):
        if teto_usd < 0:
            raise ValueError("teto negativo")
        self.caminho, self.teto, self.precos = Path(caminho), float(teto_usd), precos
        self.caminho.parent.mkdir(parents=True, exist_ok=True)
        self.caminho.touch(exist_ok=True)

    # ---- leitura do estado, sempre a partir do disco
    def _estado(self) -> dict:
        reservas, liquidado, violada = {}, 0.0, False
        for linha in self.caminho.read_text(encoding="utf-8").splitlines():
            if not linha.strip():
                continue
            e = json.loads(linha)
            if e["tipo"] == "reserva":
                reservas[e["id"]] = e["usd"]
            elif e["tipo"] == "liquidacao":
                reservas.pop(e["id"], None)
                liquidado += e["usd"]
                violada = violada or e.get("excedeu", False)
        return {"abertas": reservas, "liquidado": liquidado, "violada": violada}

    def gasto(self) -> float:
        """Liquidado + reservas abertas (uma reserva aberta pode ter sido cobrada)."""
        s = self._estado()
        return s["liquidado"] + sum(s["abertas"].values())

    def saldo(self) -> float:
        return self.teto - self.gasto()

    @contextmanager
    def _trava(self):
        with self.caminho.open("a", encoding="utf-8") as f:
            fcntl.flock(f, fcntl.LOCK_EX)
            try:
                yield f
            finally:
                fcntl.flock(f, fcntl.LOCK_UN)

    @staticmethod
    def _grava(f, e: dict):
        f.write(json.dumps(e, ensure_ascii=False) + "\n")
        f.flush()
        os.fsync(f.fileno())

    # ---- operações
    def reservar(self, tin_max: int, tout_max: int, ctx: dict | None = None) -> str:
        pior = self.precos.usd(tin_max, tout_max)
        with self._trava() as f:
            s = self._estado()
            if s["violada"]:
                raise HipoteseViolada("uso acima da reserva registrado antes; nenhuma requisição é enviada")
            livre = self.teto - s["liquidado"] - sum(s["abertas"].values())
            if pior > livre:
                raise SemSaldo(f"pior caso US$ {pior:.6f} > saldo livre US$ {max(livre, 0):.6f} (teto {self.teto})")
            rid = f"{time.time_ns()}-{os.getpid()}-{len(s['abertas'])}"
            self._grava(f, {"tipo": "reserva", "id": rid, "usd": round(pior, 10), "tin_max": tin_max,
                            "tout_max": tout_max, **(ctx or {})})
        return rid

    def liquidar(self, rid: str, tin: int | None, tout: int | None) -> float:
        with self._trava() as f:
            s = self._estado()
            if rid not in s["abertas"]:
                raise KeyError(f"reserva {rid} inexistente ou já liquidada")
            reservado = s["abertas"][rid]
            if tin is None or tout is None:
                usd, excedeu = reservado, False  # sem uso informado: conta o pior caso
            else:
                usd = self.precos.usd(tin, tout)
                excedeu = usd > reservado + 1e-12
            self._grava(f, {"tipo": "liquidacao", "id": rid, "usd": round(usd, 10), "tin": tin, "tout": tout,
                            "reservado": reservado, "excedeu": excedeu})
        if excedeu:
            raise HipoteseViolada(f"uso US$ {usd:.6f} acima da reserva US$ {reservado:.6f}")
        return usd


TRANSITORIOS = ("RateLimitError", "APITimeoutError", "APIConnectionError", "InternalServerError")


class ChamadaProtegida:
    """Envolve `client.chat.completions.create`: reserva, envia, liquida; tentativas pelo mesmo caminho."""

    def __init__(self, client, orcamento: Orcamento, tentativas: int = 3, espera_s: float = 2.0, dormir=time.sleep):
        # as tentativas internas do cliente não passam pelo livro: desligadas
        self.client = client.with_options(max_retries=0) if hasattr(client, "with_options") else client
        self.orc, self.tentativas, self.espera_s, self.dormir = orcamento, tentativas, espera_s, dormir

    def criar(self, ctx: dict | None = None, **kw):
        if "max_completion_tokens" not in kw:
            raise ValueError("max_completion_tokens é obrigatório: sem ele não há pior caso")
        tin_max, tout_max = cota_entrada(kw["messages"]), int(kw["max_completion_tokens"])
        for n in range(1, self.tentativas + 1):
            rid = self.orc.reservar(tin_max, tout_max, {**(ctx or {}), "tentativa": n})  # SemSaldo sai daqui
            try:
                r = self.client.chat.completions.create(**kw)
            except Exception as e:  # noqa: BLE001 — qualquer falha liquida pelo pior caso
                self.orc.liquidar(rid, None, None)
                if type(e).__name__ in TRANSITORIOS and n < self.tentativas:
                    self.dormir(self.espera_s * n)
                    continue
                raise
            u = getattr(r, "usage", None)
            self.orc.liquidar(rid, getattr(u, "prompt_tokens", None), getattr(u, "completion_tokens", None))
            return r
        raise AssertionError("inalcançável")
