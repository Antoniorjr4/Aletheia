"""Provedores de modelo (réplicas de `arx::model`): roteirizado, máscara local e o cliente HTTP
compatível com OpenAI, com transporte real (urllib), de gravação e de replay."""

from __future__ import annotations

import json
import time

from .canon import dumps
from .mask import DecodeError, decode_following
from .shape import proposal_from_json


class ProviderError(Exception):
    pass


class Request:
    """`arx::model::Request`: o rascunho, o teto de tokens do conteúdo e o de raciocínio."""

    def __init__(self, draft, max_tokens: int, reasoning: int):
        self.model = draft.model
        self.prompt = draft.prompt
        self.shape = draft.shape
        self.max_bytes = draft.max_bytes
        self.schema = draft.schema
        self.gbnf = draft.gbnf
        self.max_tokens = max_tokens
        self.reasoning = reasoning


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

    def complete(self, req: Request) -> Completion:
        lst = self.responses.get(req.model)
        if lst is None:
            raise ProviderError(f"roteiro sem respostas para `{req.model}`")
        if not lst:
            raise ProviderError(f"roteiro vazio para `{req.model}`")
        i = self.idx.get(req.model, 0)
        r = lst[min(i, len(lst) - 1)]
        self.idx[req.model] = i + 1
        return Completion(r if isinstance(r, str) else proposal_from_json(req.shape, r))


class Masked:
    """Decodificação restrita local: a proposta do provedor interno passa pela máscara do tipo."""

    def __init__(self, inner):
        self.inner = inner

    def name(self) -> str:
        return f"masked({self.inner.name()})"

    def complete(self, req: Request) -> Completion:
        prop = self.inner.complete(req)
        limit = 16384 if req.max_bytes is None else req.max_bytes
        try:
            text, dev = decode_following(req.shape, prop.text, limit)
        except DecodeError as e:
            raise ProviderError(str(e)) from None
        note = None
        if dev is not None:
            pos, wanted, allowed, _ = dev
            w = f"`{wanted}`" if wanted is not None else "encerrar"
            note = f"máscara: na posição {pos}, o modelo queria {w}; o tipo só admitia {' | '.join(allowed)}"
        return Completion(text, tok_in=prop.tok_in, note=note)


# ------------------------------------------------------------------ HTTP compatível com OpenAI

SYSTEM = ("Você é um componente de um programa Aletheia. Responda somente com o documento JSON pedido, "
          "no formato {\"value\": …}, sem texto adicional.")
MAX_WAIT = 60.0
USER_AGENT = "aletheia-runtime/0.5 (+https://github.com/Antoniorjr4/Aletheia)"


class HttpReply:
    def __init__(self, status: int, retry_after, request_id, body: str, latency: float):
        self.status, self.retry_after, self.request_id, self.body, self.latency = (
            status, retry_after, request_id, body, latency)


class UrllibTransport:
    """Rede real. A latência é medida até a chegada dos cabeçalhos, como no `ureq` do ARX."""

    def __init__(self, timeout: float = 180.0):
        self.timeout = timeout

    def live(self) -> bool:
        return True

    def post(self, url: str, api_key, body: str) -> HttpReply:
        import urllib.error
        import urllib.request

        headers = {"Content-Type": "application/json", "User-Agent": USER_AGENT}
        if api_key:
            headers["Authorization"] = f"Bearer {api_key}"
        req = urllib.request.Request(url, data=body.encode("utf-8"), headers=headers, method="POST")
        t0 = time.monotonic()
        try:
            resp = urllib.request.urlopen(req, timeout=self.timeout)
        except urllib.error.HTTPError as e:
            resp = e
        except OSError as e:
            raise ProviderError(f"{url}: {e}") from None
        latency = time.monotonic() - t0
        text = resp.read().decode("utf-8", errors="replace")
        return HttpReply(resp.status if hasattr(resp, "status") else resp.code, resp.headers.get("retry-after"),
                         resp.headers.get("x-request-id"), text, latency)


class Exchange:
    def __init__(self, url: str, request: str, reply: HttpReply):
        self.url, self.request, self.reply = url, request, reply

    def to_json(self) -> dict:
        r = self.reply
        return {"url": self.url, "request": self.request, "status": r.status, "retry_after": r.retry_after,
                "x_request_id": r.request_id, "response": r.body, "latency_s": r.latency}

    @staticmethod
    def from_json(j: dict) -> "Exchange":
        return Exchange(j["url"], j["request"], HttpReply(j["status"], j.get("retry_after"), j.get("x_request_id"),
                                                          j["response"], float(j["latency_s"])))


class Recorder:
    def __init__(self, inner):
        self.inner = inner
        self.tape: list = []

    def live(self) -> bool:
        return self.inner.live()

    def post(self, url: str, api_key, body: str) -> HttpReply:
        reply = self.inner.post(url, api_key, body)
        self.tape.append(Exchange(url, body, reply))
        return reply


class Replay:
    """Respostas gravadas, em ordem; cada requisição precisa ser idêntica à gravada."""

    def __init__(self, tape: list):
        self.tape = tape
        self.next = 0

    def live(self) -> bool:
        return False

    def post(self, url: str, api_key, body: str) -> HttpReply:
        n = self.next + 1
        if self.next >= len(self.tape):
            raise ProviderError(f"replay esgotado: a requisição {n} não foi gravada")
        ex = self.tape[self.next]
        if ex.url != url:
            raise ProviderError(f"replay diverge na requisição {n}: URL `{url}`, gravada `{ex.url}`")
        if ex.request != body:
            raise ProviderError(f"replay diverge na requisição {n}: o corpo difere do gravado")
        self.next += 1
        return ex.reply


def _retry_wait(header, body: str, attempt: int) -> float:
    if header is not None:
        try:
            return max(float(header.strip()), 0.5)
        except ValueError:
            pass
    i = body.find("try again in ")
    if i >= 0:
        rest = body[i + 13:]
        num = ""
        for c in rest:
            if c.isdigit() and c.isascii() or c == ".":
                num += c
            else:
                break
        try:
            v = float(num)
        except ValueError:
            v = None
        if v is not None and num:
            return max(v / 1000.0, 0.5) if rest[len(num):].startswith("ms") else v + 0.5
    return min(2.0 ** attempt, MAX_WAIT)


def _get(j, *path):
    for k in path:
        if isinstance(k, int):
            j = j[k] if isinstance(j, list) and len(j) > k else None
        else:
            j = j.get(k) if isinstance(j, dict) else None
    return j


def _u64(v):
    return v if type(v) is int and v >= 0 else None


def _str(v):
    return v if isinstance(v, str) else None


class OpenAiCompat:
    """`arx::model::OpenAiCompat`: mesmo corpo de requisição, mesmas novas tentativas, mesma leitura
    da resposta. A configuração vem do ambiente ou de um cassete."""

    def __init__(self, config: dict, transport, api_key=None):
        self.base_url = config["base_url"].rstrip("/")
        self.model_id = config["model_id"]
        self.constraint = config["constraint"]
        self.dialect = config["dialect"]
        self.reasoning_effort = config.get("reasoning_effort")
        self.extra = config.get("extra")
        self.max_retries = int(config.get("max_retries", 6))
        self.transport = transport
        self.api_key = api_key

    @staticmethod
    def from_env(model=None, base_url=None, transport=None) -> "OpenAiCompat":
        import os

        env = lambda k: (os.environ.get(k) or "").strip() or None
        model = model or env("ALETHEIA_MODEL")
        if not model:
            raise ProviderError("informe o modelo remoto com --model ou ALETHEIA_MODEL")
        effort = env("ALETHEIA_REASONING_EFFORT") or "low"
        extra = env("ALETHEIA_EXTRA_BODY")
        cfg = {
            "base_url": base_url or env("OPENAI_BASE_URL") or "https://api.openai.com/v1",
            "model_id": model,
            "constraint": env("ALETHEIA_CONSTRAINT") or "json-schema",
            "dialect": env("ALETHEIA_SCHEMA") or "core",
            "reasoning_effort": None if effort == "none" else effort,
            "extra": json.loads(extra) if extra else None,
            "max_retries": 6,
        }
        key = env("OPENAI_API_KEY") or env("ATH_API_KEY") or env("GROQ_API_KEY")
        return OpenAiCompat(cfg, transport or UrllibTransport(), key)

    def config_json(self) -> dict:
        return {"base_url": self.base_url, "model_id": self.model_id, "constraint": self.constraint,
                "dialect": self.dialect, "reasoning_effort": self.reasoning_effort, "extra": self.extra,
                "max_retries": self.max_retries}

    def name(self) -> str:
        return f"openai-compat({self.model_id}, {self.constraint}/{self.dialect})"

    def body(self, req: Request) -> dict:
        total = req.max_tokens + req.reasoning
        body = {
            "model": self.model_id,
            "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": req.prompt}],
            "temperature": 0,
        }
        if self.constraint == "json-schema":
            body["max_completion_tokens"] = total
            body["response_format"] = {
                "type": "json_schema",
                "json_schema": {"name": "aletheia_output", "strict": True, "schema": req.schema[self.dialect]},
            }
        elif self.constraint == "gbnf":
            body["max_tokens"] = total
            body["grammar"] = req.gbnf
        else:
            body["max_completion_tokens"] = total
        if req.reasoning > 0 and self.reasoning_effort is not None:
            body["reasoning_effort"] = self.reasoning_effort
        if isinstance(self.extra, dict):
            for k, v in self.extra.items():
                if v is None:
                    body.pop(k, None)
                else:
                    body[k] = v
        return body

    def complete(self, req: Request) -> Completion:
        url = f"{self.base_url}/chat/completions"
        body = dumps(self.body(req))
        queued = 0.0
        attempt = 0
        while True:
            r = self.transport.post(url, self.api_key, body)
            status, text, latency = r.status, r.body, r.latency
            if (status == 429 or status >= 500) and attempt < self.max_retries:
                wait = _retry_wait(r.retry_after, text, attempt)
                if wait > MAX_WAIT:
                    raise ProviderError(f"{url}: HTTP {status}: o provedor pediu {wait:.0f} s de espera")
                if self.transport.live():
                    time.sleep(wait)
                queued += wait
                attempt += 1
                continue
            if status == 400 and "json_validate_failed" in text:
                try:
                    j = json.loads(text)
                except ValueError:
                    j = None
                c = Completion(_str(_get(j, "error", "failed_generation")) or "", note="servidor: json_validate_failed")
                c.latency, c.queued, c.retries, c.request_id = latency, queued, attempt, r.request_id
                return c
            if status != 200:
                raise ProviderError(f"{url}: HTTP {status}: {text[:400]}")
            try:
                j = json.loads(text)
            except ValueError as e:
                raise ProviderError(f"{url}: JSON inválido: {e}") from None
            msg = _get(j, "choices", 0, "message")
            reasoning = _str(_get(msg, "reasoning"))
            c = Completion(_str(_get(msg, "content")) or "",
                           note=f"restrição no servidor: {self.constraint}/{self.dialect}")
            c.tok_in = _u64(_get(j, "usage", "prompt_tokens"))
            c.tok_out = _u64(_get(j, "usage", "completion_tokens"))
            c.tok_reasoning = _u64(_get(j, "usage", "completion_tokens_details", "reasoning_tokens"))
            c.reasoning = reasoning if reasoning else None
            c.latency, c.queued, c.retries = latency, queued, attempt
            c.request_id = _str(_get(j, "id")) or r.request_id
            c.finish_reason = _str(_get(j, "choices", 0, "finish_reason"))
            c.served_model = _str(_get(j, "model"))
            return c
