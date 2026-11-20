#!/usr/bin/env python3
"""M10 — autoria por modelo: o modelo escreve o agente de um pacote numa das duas linguagens.

    python3 bench/m10/author.py --packs payments,refunds --langs ath,py --reps 3 \
        --out bench/m10/runs/authoring --max-usd-total 0.45

Para cada (pacote, linguagem, repetição), até 5 ciclos de "escrever → ferramentas da linguagem →
corrigir". O autor recebe o guia da linguagem (mensagem de sistema) e a missão (sem o mundo e sem
os cenários do oráculo). As ferramentas devolvem só diagnósticos:

* Aletheia: `athc check`; se passa, um ensaio (`__dry__`: modelo neutro e modelo hostil genérico);
* Python: `ruff check` e `mypy`; se passam, o mesmo ensaio.

Cada versão é guardada (`v1.ath` … `v5.ath`). A rodada é retomável: o que tem `result.json` não
roda de novo. Cada chamada vai para `spend.jsonl`, e a rodada para no teto em dólar. Cota
esgotada (429) encerra com código 75, sem gravar resultado parcial.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import harness  # noqa: E402

MODEL = os.environ.get("M10_MODEL", "openai/gpt-oss-120b")
PRICE_IN, PRICE_OUT = 0.15, 0.60
MAX_CYCLES = 5
EXT = {"ath": "ath", "py": "py"}
ANSI = re.compile(r"\x1b\[[0-9;]*m")
FENCE = re.compile(r"```[a-zA-Z0-9_+-]*\n(.*?)```", re.S)


def brief(spec: dict, lang: str) -> str:
    """A missão como o autor a recebe: sem o mundo e sem cenários."""
    lines = [f"# Mission: {spec['title']}", "", spec["mission"], "", "## Security rules"]
    lines += [f"{i}. {r}" for i, r in enumerate(spec["rules"], 1)]
    lines += ["", "## Tools (name, parameters in order, kind)"]
    for t in spec["tools"]:
        params = ", ".join(f"{p['name']}: {p['type']}" for p in t["params"]) or "no parameters"
        flags = [t["kind"]] + (["untrusted result"] if t.get("untrusted") else []) + \
                (["irreversible"] if t.get("irreversible") else [])
        lines.append(f"- `{t['name']}({params})` — {', '.join(flags)}. {t.get('doc', '')}")
    lines += ["", "## Trusted lists provided by the host"]
    lines += [f"- `{t['name']}` — {t['doc']}" for t in spec["trusted"]]
    what = "the complete Aletheia source file" if lang == "ath" else "the complete Python source file"
    lines += ["", f"Write {what} for this agent. Reply with the whole file in ONE fenced code block and nothing else."]
    return "\n".join(lines)


def extract(text: str) -> str | None:
    blocks = FENCE.findall(text or "")
    return max(blocks, key=len) if blocks else None


def run(cmd: list, timeout=120) -> tuple[int, str]:
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return r.returncode, ANSI.sub("", r.stdout + r.stderr)
    except subprocess.TimeoutExpired:
        return 124, "timeout"


def feedback(lang: str, src: Path, pack_dir: Path) -> str:
    """Os diagnósticos das ferramentas da linguagem; vazio se está tudo limpo."""
    if lang == "ath":
        code, out = run([harness.ATHC, "check", str(src)])
        if code != 0:
            return "athc check:\n" + out[-3500:]
    else:
        code, out = run(["ruff", "check", "--no-cache", "--output-format", "concise", str(src)])
        msgs = out.strip() if code != 0 else ""
        code, out = run(["mypy", "--ignore-missing-imports", "--no-error-summary", "--no-incremental", str(src)])
        if code != 0:
            msgs += "\nmypy:\n" + out.strip()
        if msgs.strip():
            return "static tools:\n" + msgs.strip().replace(str(src.parent) + "/", "")[-3500:]
    errors = []
    for mode, seed in (("stub", 0), ("adversarial", 0), ("adversarial", 1)):
        r = harness.exec_subprocess(lang, src, pack_dir, "__dry__", mode, seed)
        if r["error"]:
            errors.append(f"dry run ({mode} model): {r['error']}")
    return "\n".join(dict.fromkeys(errors))[:3500]


class Ledger:
    def __init__(self, path: Path, cap: float):
        self.path, self.cap = path, cap
        self.total = 0.0
        if path.exists():
            self.total = sum(json.loads(x)["usd"] for x in path.read_text().splitlines() if x.strip())

    def guard(self):
        if self.total >= self.cap:
            raise SystemExit(f"teto de gasto atingido: US$ {self.total:.4f} de {self.cap}")

    def charge(self, ctx: dict, tin: int, tout: int):
        usd = (tin * PRICE_IN + tout * PRICE_OUT) / 1e6
        self.total += usd
        with self.path.open("a", encoding="utf-8") as f:
            f.write(json.dumps({**ctx, "tok_in": tin, "tok_out": tout, "usd": round(usd, 8)}) + "\n")


def author_one(client, pack_dir: Path, lang: str, rep: int, out: Path, ledger: Ledger) -> dict:
    spec = json.loads((pack_dir / "spec.json").read_text(encoding="utf-8"))
    guide = (HERE / "guides" / ("aletheia.md" if lang == "ath" else "python.md")).read_text(encoding="utf-8")
    messages = [{"role": "system", "content": guide}, {"role": "user", "content": brief(spec, lang)}]
    out.mkdir(parents=True, exist_ok=True)
    cycles, tin_total, tout_total, t0 = [], 0, 0, time.monotonic()
    clean = False
    for n in range(1, MAX_CYCLES + 1):
        ledger.guard()
        r = client.chat.completions.create(model=MODEL, messages=messages, temperature=0.6, seed=1000 * rep + n,
                                           max_completion_tokens=7000, reasoning_effort="medium")
        tin, tout = (r.usage.prompt_tokens, r.usage.completion_tokens) if r.usage else (0, 0)
        ledger.charge({"pack": spec["id"], "lang": lang, "rep": rep, "cycle": n}, tin, tout)
        tin_total += tin
        tout_total += tout
        text = r.choices[0].message.content or ""
        code = extract(text)
        src = out / f"v{n}.{EXT[lang]}"
        src.write_text(code if code is not None else "", encoding="utf-8")
        fb = "no fenced code block found in the reply" if code is None else feedback(lang, src, pack_dir)
        cycles.append({"cycle": n, "file": src.name, "tok_in": tin, "tok_out": tout, "feedback": fb})
        if not fb:
            clean = True
            break
        messages += [{"role": "assistant", "content": text},
                     {"role": "user", "content": "The tools reported:\n\n" + fb +
                      "\n\nFix the problems and reply with the whole corrected file in ONE fenced code block."}]
    result = {"pack": spec["id"], "lang": lang, "rep": rep, "model": MODEL, "clean": clean, "cycles": cycles,
              "tok_in": tin_total, "tok_out": tout_total, "seconds": round(time.monotonic() - t0, 1),
              "usd": round((tin_total * PRICE_IN + tout_total * PRICE_OUT) / 1e6, 6)}
    (out / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=1), encoding="utf-8")
    return result


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--packs", required=True)
    ap.add_argument("--langs", default="ath,py")
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--out", default=str(HERE / "runs" / "authoring"))
    ap.add_argument("--max-usd-total", type=float, default=0.45)
    a = ap.parse_args()
    import openai

    client = openai.OpenAI(max_retries=4)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    ledger = Ledger(out / "spend.jsonl", a.max_usd_total)
    # pareado: para cada pacote e repetição, as duas linguagens em seguida
    for rep in range(1, a.reps + 1):
        for pack in a.packs.split(","):
            for lang in a.langs.split(","):
                d = out / pack / lang / f"rep{rep}"
                if (d / "result.json").exists():
                    continue
                try:
                    r = author_one(client, HERE / "packs" / pack, lang, rep, d, ledger)
                except openai.RateLimitError as e:
                    print(f"cota: {str(e)[:300]}", flush=True)
                    print(f"gasto acumulado: US$ {ledger.total:.4f}", flush=True)
                    return 75
                print(f"{pack} {lang} rep{rep}: {'limpo' if r['clean'] else 'NÃO limpo'} em {len(r['cycles'])} ciclo(s), "
                      f"{r['tok_in'] + r['tok_out']} tok, US$ {r['usd']} (acumulado {ledger.total:.4f})", flush=True)
    print(f"gasto acumulado: US$ {ledger.total:.4f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
