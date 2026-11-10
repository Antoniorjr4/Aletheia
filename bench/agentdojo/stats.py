#!/usr/bin/env python3
"""Estatística pareada e com dependência para uma rodada do AgentDojo, lida dos logs oficiais.

    python bench/agentdojo/stats.py bench/agentdojo/runs/banking-openai-gpt-oss-120b

O `matrix.py` dá o Wilson por pipeline, que trata cada par como independente. Mas os pares
não são independentes: cada tarefa de usuário aparece com todas as injeções, e um erro do
modelo numa tarefa tende a se repetir nos pares dela. Este script acrescenta, só sobre os pares
completos nos dois pipelines:

* **dois regimes do intervalo de confiança.** O Wilson por par supõe independência; o Wilson
  por tarefa de usuário (uma tarefa conta como sucesso se algum par dela teve sucesso) supõe
  correlação total dentro da tarefa. A incerteza real fica entre os dois, conforme a
  correlação intra-tarefa, que não é estimável com zero eventos;
* **bootstrap por agrupamento:** reamostra tarefas de usuário (10.000 vezes, semente fixa) e
  dá o intervalo percentil da taxa combinada. Com zero eventos ele degenera em [0, 0] e não
  serve, por isso entra só quando há eventos;
* **McNemar exato** nos pares discordantes, para comparar os pipelines sobre os mesmos pares.

Só biblioteca padrão. Nenhuma chamada ao modelo.
"""

from __future__ import annotations

import json
import math
import random
import sys
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from matrix import load, wilson  # noqa: E402


def mcnemar_exact(b: int, c: int) -> float:
    """Valor-p bilateral do teste exato de McNemar (binomial nos discordantes, p = 1/2)."""
    n = b + c
    if n == 0:
        return 1.0
    tail = sum(math.comb(n, i) for i in range(min(b, c) + 1)) / 2**n
    return min(1.0, 2 * tail)


def cluster_bootstrap(groups: dict, b: int = 10000, seed: int = 20261110) -> list | None:
    """IC95 percentil da taxa combinada, reamostrando grupos (tarefas de usuário)."""
    keys = sorted(groups)
    if not keys or sum(k for k, _ in groups.values()) == 0:
        return None
    rng = random.Random(seed)
    stats = []
    for _ in range(b):
        pick = [groups[rng.choice(keys)] for _ in keys]
        n = sum(x[1] for x in pick)
        stats.append(sum(x[0] for x in pick) / n if n else 0.0)
    stats.sort()
    return [round(stats[int(0.025 * b)], 4), round(stats[int(0.975 * b) - 1], 4)]


def r4(ci):
    return [round(x, 4) for x in ci] if ci else None


def analyze(logdir: Path) -> dict:
    m = json.loads((logdir / "matrix.json").read_text(encoding="utf-8"))
    suite, attack = m["suite"], m["attack"]
    pipes = list(m["pipelines"])
    from agentdojo.task_suite.load_suites import get_suite  # só para os ids da suíte

    s = get_suite(m["benchmark_version"], suite)
    order = lambda ks: sorted(ks, key=lambda k: int(k.rsplit("_", 1)[-1]))  # noqa: E731
    uts, injs = order(s.user_tasks), order(s.injection_tasks)
    data = {p: load(logdir, p, suite, attack, uts, injs) for p in pipes}
    benign = sorted(set.intersection(*(set(data[p][0]) for p in pipes)), key=lambda k: int(k.rsplit("_", 1)[-1]))
    pairs = sorted(set.intersection(*(set(data[p][1]) for p in pipes)),
                   key=lambda k: (int(k[0].rsplit("_", 1)[-1]), int(k[1].rsplit("_", 1)[-1])))
    out = {"suite": suite, "pipelines": pipes, "benign": len(benign), "pairs": len(pairs),
           "user_tasks_with_pairs": sorted({u for u, _ in pairs}, key=lambda k: int(k.rsplit("_", 1)[-1])),
           "by_pipeline": {}, "paired": {}}
    for p in pipes:
        b, a = data[p]
        res = {}
        for name, items, idx in [("utility", [b[k] for k in benign], 0), ("utility_under_attack", [a[k] for k in pairs], 0),
                                 ("asr", [a[k] for k in pairs], 1)]:
            k, n = sum(x[idx] for x in items), len(items)
            r = {"k": k, "n": n, "rate": round(k / n, 4) if n else None, "wilson_pairs": r4(wilson(k, n))}
            if name != "utility":
                groups = defaultdict(lambda: [0, 0])
                for (ut, _), x in zip(pairs, items):
                    groups[ut][0] += x[idx]
                    groups[ut][1] += 1
                g = {u: tuple(v) for u, v in groups.items()}
                kc = sum(1 for v in g.values() if v[0] > 0)
                r["wilson_user_tasks"] = r4(wilson(kc, len(g)))
                r["user_tasks_with_event"] = f"{kc}/{len(g)}"
                r["bootstrap_user_tasks"] = cluster_bootstrap(g)
                r["per_user_task"] = {u: f"{v[0]}/{v[1]}" for u, v in sorted(g.items(), key=lambda kv: int(kv[0].rsplit('_', 1)[-1]))}
            res[name] = r
        out["by_pipeline"][p] = res
    if len(pipes) == 2:
        p0, p1 = pipes
        for name, keys, bucket, idx in [("utility", benign, 0, 0), ("utility_under_attack", pairs, 1, 0), ("asr", pairs, 1, 1)]:
            x0 = [data[p0][bucket][k][idx] for k in keys]
            x1 = [data[p1][bucket][k][idx] for k in keys]
            bb = sum(1 for u, v in zip(x0, x1) if u and not v)
            cc = sum(1 for u, v in zip(x0, x1) if v and not u)
            out["paired"][name] = {f"only_{p0}": bb, f"only_{p1}": cc, "mcnemar_exact_p": round(mcnemar_exact(bb, cc), 6)}
    return out


def pct(ci):
    return "—" if ci is None else f"{100 * ci[0]:.0f}–{100 * ci[1]:.0f}%"


def markdown(r: dict) -> str:
    lines = [f"Pares completos nos dois pipelines: {r['benign']} tarefas sem ataque · {r['pairs']} pares sob ataque, "
             f"de {len(r['user_tasks_with_pairs'])} tarefas de usuário.", "",
             "| pipeline | indicador | k/n | IC95 por par (independência) | IC95 por tarefa (correlação total) | bootstrap por tarefa |",
             "| --- | --- | --- | --- | --- | --- |"]
    names = {"utility": "utilidade", "utility_under_attack": "utilidade sob ataque", "asr": "ASR"}
    for p, res in r["by_pipeline"].items():
        for name, x in res.items():
            lines.append(f"| `{p}` | {names[name]} | {x['k']}/{x['n']} | {pct(x['wilson_pairs'])} | "
                         f"{pct(x.get('wilson_user_tasks')) if 'wilson_user_tasks' in x else '—'}"
                         f"{' (' + x['user_tasks_with_event'] + ' tarefas)' if 'user_tasks_with_event' in x else ''} | "
                         f"{pct(x.get('bootstrap_user_tasks'))} |")
    if r["paired"]:
        lines += ["", "| comparação pareada | só no 1º | só no 2º | McNemar exato (p) |", "| --- | --- | --- | --- |"]
        p0, p1 = r["pipelines"]
        for name, x in r["paired"].items():
            lines.append(f"| {names[name]} (`{p0}` × `{p1}`) | {x[f'only_{p0}']} | {x[f'only_{p1}']} | {x['mcnemar_exact_p']:.4g} |")
    return "\n".join(lines) + "\n"


def main() -> int:
    logdir = Path(sys.argv[1])
    r = analyze(logdir)
    (logdir / "stats.json").write_text(json.dumps(r, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    md = markdown(r)
    (logdir / "stats.md").write_text(md, encoding="utf-8")
    print(md)
    return 0


if __name__ == "__main__":
    sys.exit(main())
