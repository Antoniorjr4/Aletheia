"""Provedores de modelo: roteirizado e máscara local (réplicas de `arx::model`)."""

from __future__ import annotations

from .mask import DecodeError, decode_following
from .shape import proposal_from_json


class ProviderError(Exception):
    pass


class Completion:
    def __init__(self, text: str, tok_in=None, note=None):
        self.text = text
        self.tok_in = tok_in
        self.tok_out = None
        self.tok_reasoning = None
        self.reasoning = None
        self.note = note
        self.latency = None
        self.queued = 0.0
        self.retries = 0
        self.request_id = None
        self.finish_reason = None
        self.served_model = None


class Scripted:
    """Respostas na ordem do roteiro; esgotado, repete a última."""

    def __init__(self, spec: dict | None = None):
        self.responses = {k: list(v) for k, v in (spec or {}).items()}
        self.idx: dict = {}

    def name(self) -> str:
        return "scripted"

    def complete(self, model: str, prompt: str, shape: dict, max_bytes) -> Completion:
        lst = self.responses.get(model)
        if lst is None:
            raise ProviderError(f"roteiro sem respostas para `{model}`")
        if not lst:
            raise ProviderError(f"roteiro vazio para `{model}`")
        i = self.idx.get(model, 0)
        r = lst[min(i, len(lst) - 1)]
        self.idx[model] = i + 1
        return Completion(r if isinstance(r, str) else proposal_from_json(shape, r))


class Masked:
    """Decodificação restrita local: a proposta do provedor interno passa pela máscara do tipo."""

    def __init__(self, inner):
        self.inner = inner

    def name(self) -> str:
        return f"masked({self.inner.name()})"

    def complete(self, model: str, prompt: str, shape: dict, max_bytes) -> Completion:
        prop = self.inner.complete(model, prompt, shape, max_bytes)
        limit = 16384 if max_bytes is None else max_bytes
        try:
            text, dev = decode_following(shape, prop.text, limit)
        except DecodeError as e:
            raise ProviderError(str(e)) from None
        note = None
        if dev is not None:
            pos, wanted, allowed, _ = dev
            w = f"`{wanted}`" if wanted is not None else "encerrar"
            note = f"máscara: na posição {pos}, o modelo queria {w}; o tipo só admitia {' | '.join(allowed)}"
        return Completion(text, tok_in=prop.tok_in, note=note)
