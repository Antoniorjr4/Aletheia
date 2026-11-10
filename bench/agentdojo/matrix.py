#!/usr/bin/env python3
"""A matriz canônica de uma rodada do AgentDojo, lida dos logs oficiais.

    python bench/agentdojo/matrix.py bench/agentdojo/runs/banking-gpt-oss-120b

Para cada pipeline:

* **utilidade** sem ataque: fração das tarefas do usuário resolvidas (verificador oficial);
* **utilidade sob ataque**: a mesma fração em cada par (tarefa do usuário, tarefa de injeção);
* **ASR** (attack success rate): fração dos pares em que o verificador de segurança oficial
  diz que o objetivo do atacante foi cumprido. A taxa de defesa é 1 − ASR.

Os intervalos são de Wilson a 95%. Só entram logs completos (com `utility` e `security`). A
comparação pareada usa só os pares que todos os pipelines completaram.
"""

from __future__ import annotations

import json
import math
import sys
from collections import Counter
from pathlib import Path


def wilson(k: int, n: int, z: float = 1.959964) -> tuple[float, float] | None:
    if n == 0:
        return None
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, c - h), min(1.0, c + h))


def rate(k: int, n: int) -> dict:
    ci = wilson(k, n)
    return {"k": k, "n": n, "rate": (k / n) if n else None, "ci95": [round(x, 4) for x in ci] if ci else None}


def load(logdir: Path, pipeline: str, suite: str, attack: str, user_tasks, injection_tasks):
    base = logdir / pipeline.replace("/", "_") / suite
    benign, attacked = {}, {}
    for ut in user_tasks:
        for inj, bucket, att in [("none", benign, "none")] + [(i, attacked, attack) for i in injection_tasks]:
            f = base / ut / att / f"{inj}.json"
            if not f.is_file():
                continue
            try:
                r = json.loads(f.read_text(encoding="utf-8"))
            except ValueError:
                continue
            if not isinstance(r.get("utility"), bool) or not isinstance(r.get("security"), bool):
                continue
            key = ut if inj == "none" else (ut, inj)
            bucket[key] = (r["utility"], r["security"])
    return benign, attacked


def aletheia_notes(logdir: Path, suite: str) -> dict:
    d = logdir / "aletheia-journals" / suite
    errors, kinds, writes = Counter(), Counter(), Counter()
    for f in sorted(d.glob("*/*.summary.json")) if d.is_dir() else []:
        s = json.loads(f.read_text(encoding="utf-8"))
        if s.get("error"):
            errors[s["error"]["code"]] += 1
        kinds.update(k for k in s.get("kinds", []) if k in ("policy_reject", "refine_exhausted", "decode_reject", "endorse"))
        writes.update(c["function"] for c in s.get("calls", []) if not c["function"].startswith(("get_", "read_")))
    return {"aborts": dict(errors), "journal": dict(kinds), "writes_executed": dict(writes)}


def usage(logdir: Path) -> dict:
    f = logdir / "usage.jsonl"
    out: dict = {}
    if not f.is_file():
        return out
    for line in f.read_text(encoding="utf-8").splitlines():
        u = json.loads(line)
        a = out.setdefault(u["pipeline"], {"runs": 0, "calls": 0, "tok_in": 0, "tok_out": 0, "seconds": 0.0,
                                           "api_errors": 0})
        a["runs"] += 1
        a["api_errors"] += len(u.get("api_errors") or [])
        for k in ("calls", "tok_in", "tok_out", "seconds"):
            a[k] += u[k]
    for a in out.values():
        a["tok_per_run"] = round((a["tok_in"] + a["tok_out"]) / a["runs"]) if a["runs"] else None
        a["seconds"] = round(a["seconds"], 1)
    return out


def br(x: float, nd: int = 1) -> str:
    """Grafia brasileira: vírgula decimal."""
    return f"{x:.{nd}f}".replace(".", ",")


def thousands(n: int) -> str:
    return f"{n:,}".replace(",", ".")


def pct(r: dict) -> str:
    if r["rate"] is None:
        return "—"
    lo, hi = r["ci95"]
    return f"{br(100 * r['rate'])}% ({r['k']}/{r['n']}; IC95 {100 * lo:.0f}–{100 * hi:.0f}%)"


def write_matrix(logdir: Path, suite, attack: str, pipelines: list, user_tasks, injection_tasks) -> dict:
    logdir = Path(logdir)
    suite_name = suite if isinstance(suite, str) else suite.name
    loaded = {p: load(logdir, p, suite_name, attack, user_tasks, injection_tasks) for p in pipelines}
    common_b = set.intersection(*(set(b) for b, _ in loaded.values())) if loaded else set()
    common_a = set.intersection(*(set(a) for _, a in loaded.values())) if loaded else set()
    rows = {}
    for p, (b, a) in loaded.items():
        rows[p] = {
            "utility": rate(sum(u for u, _ in b.values()), len(b)),
            "utility_under_attack": rate(sum(u for u, _ in a.values()), len(a)),
            "asr": rate(sum(s for _, s in a.values()), len(a)),
            "paired": {
                "utility": rate(sum(b[k][0] for k in common_b), len(common_b)),
                "utility_under_attack": rate(sum(a[k][0] for k in common_a), len(common_a)),
                "asr": rate(sum(a[k][1] for k in common_a), len(common_a)),
            },
            "breaches": sorted(f"{ut}×{inj}" for (ut, inj), (_, s) in a.items() if s),
            "benign_failures": sorted(ut for ut, (u, _) in b.items() if not u),
        }
    total_b, total_a = len(user_tasks), len(user_tasks) * len(injection_tasks)
    m = {
        "benchmark": "AgentDojo",
        "benchmark_version": "v1.2.2",
        "suite": suite_name,
        "attack": attack,
        "coverage": {"benign": [len(common_b), total_b], "attacked": [len(common_a), total_a]},
        "pipelines": rows,
        "aletheia": aletheia_notes(logdir, suite_name),
        "usage": usage(logdir),
    }
    m["summary"] = {
        p: {
            "utilidade": pct(r["paired"]["utility"]),
            "utilidade sob ataque": pct(r["paired"]["utility_under_attack"]),
            "ASR": pct(r["paired"]["asr"]),
        }
        for p, r in rows.items()
    }
    (logdir / "matrix.json").write_text(json.dumps(m, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    md = [
        f"# AgentDojo {m['benchmark_version']} · `{suite_name}` · ataque `{attack}`",
        "",
        f"Cobertura pareada: {len(common_b)}/{total_b} tarefas sem ataque · {len(common_a)}/{total_a} pares sob ataque.",
        "",
        "| pipeline | utilidade | utilidade sob ataque | ASR | taxa de defesa |",
        "| --- | --- | --- | --- | --- |",
    ]
    for p, r in rows.items():
        q = r["paired"]
        defense = "—" if q["asr"]["rate"] is None else f"{br(100 * (1 - q['asr']['rate']))}%"
        md.append(f"| `{p}` | {pct(q['utility'])} | {pct(q['utility_under_attack'])} | {pct(q['asr'])} | {defense} |")
    md += ["", "Ataques bem-sucedidos (pares):", ""]
    for p, r in rows.items():
        md.append(f"- `{p}`: {', '.join(r['breaches']) or 'nenhum'}")
    md += ["", "Tarefas sem ataque não resolvidas:", ""]
    for p, r in rows.items():
        md.append(f"- `{p}`: {', '.join(r['benign_failures']) or 'nenhuma'}")
    if m["usage"]:
        md += ["", "| pipeline | execuções | chamadas ao modelo | tokens (entrada + saída) | tokens por execução |",
               "| --- | --- | --- | --- | --- |"]
        for p, u in m["usage"].items():
            md.append(
                f"| `{p}` | {u['runs']} | {u['calls']} | {thousands(u['tok_in'])} + {thousands(u['tok_out'])} | "
                f"{thousands(u['tok_per_run'])} |"
            )
    a = m["aletheia"]
    if a["journal"] or a["aborts"]:
        md += ["", f"Aletheia (journals): {a['journal']} · abortos {a['aborts'] or 'nenhum'} · "
               f"escritas executadas {a['writes_executed'] or 'nenhuma'}"]
    (logdir / "matrix.md").write_text("\n".join(md) + "\n", encoding="utf-8")
    return m


def main() -> int:
    logdir = Path(sys.argv[1])
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from agentdojo.task_suite.load_suites import get_suite

    meta = json.loads((logdir / "matrix.json").read_text())
    suite = get_suite(meta["benchmark_version"], meta["suite"])
    users = sorted(suite.user_tasks, key=lambda k: int(k.rsplit("_", 1)[-1]))
    injs = sorted(suite.injection_tasks, key=lambda k: int(k.rsplit("_", 1)[-1]))
    m = write_matrix(logdir, suite, meta["attack"], list(meta["pipelines"]), users, injs)
    print(json.dumps(m["summary"], indent=1, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
