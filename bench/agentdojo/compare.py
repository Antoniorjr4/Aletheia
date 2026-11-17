#!/usr/bin/env python3
"""Strict × Keeping × P1, e o baseline histórico: a comparação da campanha de 16/11/2026.

    python3 bench/agentdojo/compare.py bench/agentdojo/runs/banking-v2-p1-openai-gpt-oss-120b \
        --baseline bench/agentdojo/runs/banking-openai-gpt-oss-120b

Só sobre os pares completos em TODOS os pipelines comparados. O baseline vem de outra rodada
(mesmo modelo, ataque, pacote e verificadores, em outro dia): a comparação com ele é pareada
por tarefa, mas não por execução simultânea, e a Groq não garante a mesma saída a temperatura
0. Entre os três pipelines da campanha, cada tarefa rodou nos três antes da próxima.
Escreve `compare.json` e `compare.md` no logdir. Só biblioteca padrão.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from itertools import combinations
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from matrix import load, wilson  # noqa: E402
from stats import mcnemar_exact  # noqa: E402

SUITE, ATTACK = "banking", "important_instructions"
LABELS = {
    "baseline-gpt-oss-120b": "P0 baseline (AgentDojo, sem defesa) · histórico",
    "aletheia-v2-strict-gpt-oss-120b": "Aletheia v2 strict",
    "aletheia-v2-keeping-gpt-oss-120b": "Aletheia v2 keeping",
    "hardened-keeping-gpt-oss-120b": "P1 Python endurecido (keeping)",
}


def ids(n):
    return [f"{p}_{i}" for p, i in n]


def journals(logdir: Path, pipe: str) -> dict:
    tag = pipe.removesuffix("-gpt-oss-120b")
    d = logdir / "journals" / tag / SUITE
    kinds, executed, errors, n = Counter(), Counter(), Counter(), 0
    for f in sorted(d.glob("*/*.summary.json")) if d.is_dir() else []:
        s = json.loads(f.read_text(encoding="utf-8"))
        n += 1
        kinds.update(s.get("kinds", []))
        for c in s.get("calls", []):
            if not c["function"].startswith(("get_", "read_")):
                executed[c["function"]] += 1
        if s.get("error"):
            errors[(s["error"].get("code") or s["error"].get("type"))] += 1
    keep = ("policy_reject", "prune", "decode_reject", "refine_exhausted", "endorse", "commit_error",
            "duplicate_skipped")
    return {"runs": n, "journal": {k: kinds[k] for k in keep if kinds[k]}, "executed": dict(executed),
            "aborts": dict(errors)}


def usage(logdir: Path, pipe: str) -> dict:
    f = logdir / "usage.jsonl"
    rows = [json.loads(x) for x in f.read_text().splitlines() if x.strip()] if f.exists() else []
    rows = [r for r in rows if r["pipeline"] == pipe]
    if not rows:
        return {}
    tok = sum(r["tok_in"] + r["tok_out"] for r in rows)
    return {"runs": len(rows), "tok_per_run": round(tok / len(rows)), "calls_per_run": round(
        sum(r["calls"] for r in rows) / len(rows), 2), "usd": round(sum(r.get("usd", 0) for r in rows), 6),
        "seconds_per_run": round(sum(r["seconds"] for r in rows) / len(rows), 2)}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("logdir")
    ap.add_argument("--baseline", default=None)
    a = ap.parse_args()
    logdir = Path(a.logdir)
    from agentdojo.task_suite.load_suites import get_suite

    suite = get_suite("v1.2.2", SUITE)
    uts = sorted(suite.user_tasks, key=lambda k: int(k.rsplit("_", 1)[1]))
    injs = sorted(suite.injection_tasks, key=lambda k: int(k.rsplit("_", 1)[1]))
    pipes = ["aletheia-v2-strict-gpt-oss-120b", "aletheia-v2-keeping-gpt-oss-120b", "hardened-keeping-gpt-oss-120b"]
    data = {p: load(logdir, p, SUITE, ATTACK, uts, injs) for p in pipes}
    if a.baseline:
        data["baseline-gpt-oss-120b"] = load(Path(a.baseline), "baseline-gpt-oss-120b", SUITE, ATTACK, uts, injs)
    order = (["baseline-gpt-oss-120b"] if a.baseline else []) + pipes
    benign_keys = set.intersection(*(set(data[p][0]) for p in order))
    attack_keys = set.intersection(*(set(data[p][1]) for p in order))

    def rate(k, n):
        lo, hi = wilson(k, n) if n else (0, 0)
        return {"k": k, "n": n, "lo": round(lo, 4), "hi": round(hi, 4)}

    out = {"coverage": {"benign": len(benign_keys), "attacked": len(attack_keys)}, "pipelines": {}, "paired": []}
    for p in order:
        b, at = data[p]
        out["pipelines"][p] = {
            "utility": rate(sum(b[k][0] for k in benign_keys), len(benign_keys)),
            "utility_under_attack": rate(sum(at[k][0] for k in attack_keys), len(attack_keys)),
            "asr": rate(sum(at[k][1] for k in attack_keys), len(attack_keys)),
            "breaches": sorted("×".join(k) for k in attack_keys if at[k][1]),
        }
        if p != "baseline-gpt-oss-120b":
            out["pipelines"][p]["journals"] = journals(logdir, p)
            out["pipelines"][p]["usage"] = usage(logdir, p)
    for x, y in combinations(order, 2):
        for name, keys, idx, src in (("utilidade", benign_keys, 0, 0), ("utilidade sob ataque", attack_keys, 0, 1),
                                     ("ataque cumprido", attack_keys, 1, 1)):
            only_x = sum(data[x][src][k][idx] and not data[y][src][k][idx] for k in keys)
            only_y = sum(data[y][src][k][idx] and not data[x][src][k][idx] for k in keys)
            out["paired"].append({"a": x, "b": y, "metric": name, "only_a": only_x, "only_b": only_y,
                                  "p": round(mcnemar_exact(only_x, only_y), 4)})
    (logdir / "compare.json").write_text(json.dumps(out, ensure_ascii=False, indent=1))
    md = render(out, order)
    (logdir / "compare.md").write_text(md)
    print(md)
    return 0


def pct(r):
    return f"{100 * r['k'] / r['n']:.1f}% ({r['k']}/{r['n']}; IC95 {100 * r['lo']:.0f}–{100 * r['hi']:.0f}%)" if r["n"] else "—"


def render(o, order):
    c = o["coverage"]
    lines = ["# banking · Strict × Keeping × P1 (16/11/2026)", "",
             f"Pares completos em todos os pipelines: {c['benign']}/16 sem ataque · {c['attacked']}/144 sob ataque.", "",
             "| pipeline | utilidade | utilidade sob ataque | ASR | tokens/execução | US$/execução |",
             "| --- | --- | --- | --- | --- | --- |"]
    for p in order:
        x = o["pipelines"][p]
        u = x.get("usage", {})
        lines.append(f"| {LABELS.get(p, p)} | {pct(x['utility'])} | {pct(x['utility_under_attack'])} | {pct(x['asr'])} "
                     f"| {u.get('tok_per_run', '—')} | {round(u['usd'] / u['runs'], 6) if u else '—'} |")
    lines += ["", "| comparação pareada (McNemar exato) | indicador | só A | só B | p |", "| --- | --- | --- | --- | --- |"]
    for r in o["paired"]:
        lines.append(f"| {LABELS.get(r['a'], r['a'])} (A) × {LABELS.get(r['b'], r['b'])} (B) | {r['metric']} "
                     f"| {r['only_a']} | {r['only_b']} | {r['p']} |")
    lines += ["", "| pipeline | journal | operações executadas | abortos | US$ |", "| --- | --- | --- | --- | --- |"]
    for p in order:
        x = o["pipelines"][p]
        if "journals" in x:
            lines.append(f"| {LABELS.get(p, p)} | {x['journals']['journal']} | {x['journals']['executed']} "
                         f"| {x['journals']['aborts'] or 'nenhum'} | {x['usage'].get('usd', '—')} |")
    lines += ["", "Chamadas ao modelo por execução não entram na tabela: o runtime Aletheia conta cada 429 de"
              " tokens por minuto como chamada, e o SDK `openai` do P1 os repete sem contar.", "", "Ataques cumpridos:"]
    for p in order:
        lines.append(f"- {LABELS.get(p, p)}: {', '.join(o['pipelines'][p]['breaches']) or 'nenhum'}")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    sys.exit(main())
