#!/usr/bin/env python3
"""Varredura: todos os casos das duas revisões, com todos os mundos e roteiros da pasta de cada um
e, se o evento tem `c: Bool`, com `c` verdadeiro e falso. Conta R0631 (defeito da análise) e
divergência entre os runtimes.

    python3 bench/obrigacoes/varredura.py --athc target/release/athc
"""
import argparse
import itertools
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import relatorio  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--athc", required=True)
    ap.add_argument("--out", default=None)
    a = ap.parse_args()
    rows = []
    dirs = [HERE / "casos", *sorted((HERE / "revisao").glob("*/")), HERE / "revisao2", HERE / "revisao2" / "m"]
    for d in dirs:
        worlds = [w for w in d.glob("*.json") if w.stem.startswith("mundo")]
        rots = [r for r in d.glob("*.json") if not r.stem.startswith("mundo") and '"planner"' in r.read_text()]
        for prog in sorted(d.glob("*.ath")):
            src = prog.read_text()
            if subprocess.run([a.athc, "check", str(prog)], capture_output=True).returncode != 0:
                rows.append({"caso": f"{d.name}/{prog.stem}", "nao_compila": True, "r0631": False, "concordam": True})
                continue
            argsets = [['"x"', "true"], ['"x"', "false"]] if "c: Bool" in src else [['"x"']]
            for w, r, args in itertools.product(worlds, rots, argsets):
                with tempfile.TemporaryDirectory() as t:
                    t = Path(t)
                    for f in (prog, w, r):
                        shutil.copy(f, t / f.name)
                    old = relatorio.C
                    relatorio.C = t
                    try:
                        x = relatorio.run_case(a.athc, prog.stem, w.stem, r.stem, args)
                    finally:
                        relatorio.C = old
                rows.append({"caso": f"{d.name}/{prog.stem}", "mundo": w.stem, "roteiro": r.stem, "args": args,
                             "r0631": "R0631" in x["arx"]["aborto"] + x["python"]["aborto"],
                             "concordam": x["runtimes_concordam"]})
    bad = [r for r in rows if r["r0631"]]
    dis = [r for r in rows if not r["concordam"]]
    nc = sum(1 for r in rows if r.get("nao_compila"))
    print(f"{len(rows) - nc} execuções ({nc} programas recusados pelo checker); R0631: {len(bad)}; "
          f"runtimes divergem: {len(dis)}")
    for r in bad + dis:
        print(" ", r)
    if a.out:
        Path(a.out).write_text(json.dumps(rows, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
