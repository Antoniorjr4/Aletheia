#!/usr/bin/env python3
"""Sonda de permissões: quais modelos esta chave pode usar neste projeto?

Faz uma chamada mínima (16 tokens) a cada candidato e classifica a resposta. Com
`--pick auto`, escolhe o primeiro modelo liberado e o grava em $GITHUB_ENV como
ALETHEIA_MODEL (e, para Qwen, o `reasoning_format` exigido em modo JSON e tool-calling).
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request

# O Cloudflare na frente da API da Groq recusa o User-Agent padrão do urllib (erro 1010).
USER_AGENT = "aletheia-bench/0.3 (+https://github.com/Antoniorjr4/Aletheia)"
CANDIDATES = ["openai/gpt-oss-120b", "openai/gpt-oss-20b", "qwen/qwen3.8-27b"]


def probe(base: str, key: str, model: str) -> tuple[str, str]:
    body = {
        "model": model,
        "messages": [{"role": "user", "content": "Responda apenas: ok"}],
        "max_completion_tokens": 16,
    }
    req = urllib.request.Request(
        f"{base}/chat/completions",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}", "User-Agent": USER_AGENT},
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            json.loads(r.read())
        return "liberado", ""
    except urllib.error.HTTPError as e:
        text = e.read().decode(errors="replace")
        try:
            err = json.loads(text)["error"]
            code, msg = err.get("code", ""), err.get("message", "")
        except (ValueError, KeyError, TypeError):
            code, msg = "", text[:200]
        if "blocked" in code or "permission" in code:
            return "bloqueado no projeto", msg
        if e.code == 429:
            return "liberado (limite de taxa)", msg
        return f"HTTP {e.code}", msg


def extra_body_for(model: str) -> str | None:
    # Qwen na Groq exige reasoning_format parsed ou hidden com JSON e tool-calling.
    return json.dumps({"reasoning_format": "parsed"}) if model.startswith("qwen/") else None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pick", default=None, help="'auto' ou um modelo específico")
    args = ap.parse_args()
    base = os.environ.get("OPENAI_BASE_URL", "https://api.openai.com/v1").rstrip("/")
    key = os.environ.get("OPENAI_API_KEY", "")
    want = args.pick or "auto"
    candidates = CANDIDATES if want == "auto" else [want] + [m for m in CANDIDATES if m != want]
    status = {m: probe(base, key, m) for m in candidates}
    lines = ["| Modelo | Permissão |", "| --- | --- |"] + [f"| `{m}` | {s} |" for m, (s, _) in status.items()]
    print("\n".join(lines))
    for m, (s, msg) in status.items():
        if msg and not s.startswith("liberado"):
            print(f"  {m}: {msg}")
    allowed = [m for m, (s, _) in status.items() if s.startswith("liberado")]
    chosen = (want if want in allowed else None) if want != "auto" else (allowed[0] if allowed else None)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a") as f:
            f.write("### Permissões de modelo na Groq\n\n" + "\n".join(lines) + "\n\n")
    if not chosen:
        print(
            "::error::nenhum modelo utilizável para esta chave. Libere um em "
            "https://console.groq.com/settings/project/limits (Select Models)."
            if want == "auto"
            else f"::error::o modelo {want} não está liberado para esta chave neste projeto da Groq."
        )
        return 1
    print(f"modelo escolhido: {chosen}")
    env_file = os.environ.get("GITHUB_ENV")
    if env_file:
        with open(env_file, "a") as f:
            f.write(f"ALETHEIA_MODEL={chosen}\n")
            extra = extra_body_for(chosen)
            if extra and not os.environ.get("ALETHEIA_EXTRA_BODY"):
                f.write(f"ALETHEIA_EXTRA_BODY={extra}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
