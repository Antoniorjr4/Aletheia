#!/usr/bin/env python3
"""Reavalia uma rodada da M1 sob os dois oráculos, a partir das ações brutas.

Não chama modelo nem altera o `results.json` original. Útil para rodadas medidas com uma
régua só: as ações executadas ficam guardadas, então a outra régua pode ser aplicada depois.

    python3 bench/m1/rescore.py bench/m1/live/<rodada>/results.json --output <rodada>/oracles.md
"""
from __future__ import annotations

import argparse
import collections
import json
import pathlib
import sys

M1 = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(M1))

import run  # noqa: E402


def scenarios() -> dict:
    return {json.loads(p.read_text())["id"]: json.loads(p.read_text()) for p in (M1 / "scenarios").glob("*.json")}


def rescore(results: dict) -> dict:
    sc = scenarios()
    out = {"python": collections.Counter(), "aletheia": collections.Counter(), "rows": []}
    for row in results["rows"]:
        s = sc[row["id"]]
        line = {"id": row["id"], "rep": row["rep"]}
        for system in ("python", "aletheia"):
            acts = row[system]["actions"]
            rev6 = [a for a in acts if not run.in_policy_rev6(a["kind"], a, s)]
            ctx = [a for a in acts if not run.in_policy_contextual(a["kind"], a, s)]
            out[system]["rev6"] += len(rev6)
            out[system]["contextual"] += len(ctx)
            line[system] = {"rev6": rev6, "contextual": ctx}
        out["rows"].append(line)
    return out


def render(path: pathlib.Path, results: dict, r: dict) -> str:
    summ = results["summary"]
    md = [
        f"# Reavaliação sob os dois oráculos · `{path.parent.name}`",
        "",
        f"Rodada de {summ['date']} · commit `{summ['commit']}` · `{summ['model']}` · "
        f"{summ['scenarios']} cenários × {summ['reps']} repetições. Reavaliação offline das ações brutas de "
        "`results.json`; nenhuma chamada nova ao modelo.",
        "",
        "| Ações fora da política | Python | Aletheia |",
        "| --- | ---: | ---: |",
        f"| oráculo canônico da Rev6 | {r['python']['rev6']} | {r['aletheia']['rev6']} |",
        f"| oráculo contextual da Rev7 | {r['python']['contextual']} | {r['aletheia']['contextual']} |",
        "",
        "O contextual usa o inventário e a telemetria declarados nos cenários atuais "
        "(`bench/m1/scenarios/*.json`).",
    ]
    for system in ("python", "aletheia"):
        flagged = [(x["id"], x["rep"], a, n) for x in r["rows"] for n in ("rev6", "contextual") for a in x[system][n]]
        if flagged:
            md += ["", f"Ações de `{system}` fora da política:", ""]
            seen = collections.OrderedDict()
            for sid, rep, a, n in flagged:
                seen.setdefault((sid, rep, json.dumps(a, ensure_ascii=False)), []).append(n)
            for (sid, rep, a), names in seen.items():
                md.append(f"- `{sid}` #{rep}: `{a}` · oráculo: {', '.join(names)}")
    return "\n".join(md) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("results", type=pathlib.Path)
    ap.add_argument("--output", type=pathlib.Path)
    args = ap.parse_args()
    results = json.loads(args.results.read_text(encoding="utf-8"))
    md = render(args.results, results, rescore(results))
    if args.output:
        args.output.write_text(md, encoding="utf-8")
    print(md)
    return 0


if __name__ == "__main__":
    sys.exit(main())
