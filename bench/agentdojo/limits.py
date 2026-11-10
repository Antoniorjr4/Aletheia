#!/usr/bin/env python3
"""Os limites de taxa que a Groq aplica a esta chave e a este modelo, lidos dos cabeçalhos.

Uma chamada mínima (8 tokens). `x-ratelimit-limit-requests` é o teto de requisições por dia;
`x-ratelimit-limit-tokens`, o de tokens por minuto. O teto diário de tokens não vem nos
cabeçalhos: aparece na mensagem do primeiro 429 que o atinge.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request

USER_AGENT = "aletheia-bench/0.7 (+https://github.com/Antoniorjr4/Aletheia)"


def main() -> int:
    model = sys.argv[1] if len(sys.argv) > 1 else "openai/gpt-oss-120b"
    base = os.environ.get("OPENAI_BASE_URL", "https://api.groq.com/openai/v1").rstrip("/")
    body = {"model": model, "messages": [{"role": "user", "content": "ok"}], "max_completion_tokens": 8}
    req = urllib.request.Request(
        f"{base}/chat/completions",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {os.environ['OPENAI_API_KEY']}",
                 "User-Agent": USER_AGENT},
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            headers, status = dict(r.headers), r.status
    except urllib.error.HTTPError as e:
        headers, status = dict(e.headers), e.code
        print(e.read().decode(errors="replace")[:400])
    keys = sorted(k for k in headers if k.lower().startswith("x-ratelimit"))
    lines = [f"| `{k.lower()}` | {headers[k]} |" for k in keys]
    table = "\n".join(["| cabeçalho | valor |", "| --- | --- |", *lines])
    print(f"HTTP {status} · {model}\n{table}")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a") as f:
            f.write(f"### Limites da Groq para `{model}`\n\nHTTP {status}\n\n{table}\n\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
