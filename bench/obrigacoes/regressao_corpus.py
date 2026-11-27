#!/usr/bin/env python3
"""Regressão do reparo B1/B2: `athc obligations` em todos os programas versionados, dois compiladores.

    python3 bench/obrigacoes/regressao_corpus.py --antes /tmp/athc-antes --depois target/release/athc
"""
import argparse
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def ob(athc, f):
    r = subprocess.run([athc, "obligations", str(f)], capture_output=True, text=True)
    return r.returncode, [ln.strip() for ln in r.stdout.splitlines() if ln.startswith("  ")]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--antes", required=True)
    ap.add_argument("--depois", required=True)
    ap.add_argument("--out", default=str(Path(__file__).with_name("regressao_corpus.json")))
    a = ap.parse_args()
    files = subprocess.run(["git", "-C", str(ROOT), "ls-files", "*.ath"], capture_output=True, text=True).stdout.split()
    rows, mudou = [], []
    for rel in files:
        x, y = ob(a.antes, ROOT / rel), ob(a.depois, ROOT / rel)
        with_req = any("descarregada" in s or "dinâmica" in s for s in x[1] + y[1])
        rows.append({"arquivo": rel, "antes": x[1], "depois": y[1], "com_requires": with_req})
        if x != y:
            mudou.append(rel)
    Path(a.out).write_text(json.dumps({"programas": len(files), "mudaram": mudou, "linhas": rows},
                                      ensure_ascii=False, indent=1), encoding="utf-8")
    sites = lambda k: sum(1 for r in rows for s in r[k] if " descarregada " in f" {s} ")
    dyn = lambda k: sum(1 for r in rows for s in r[k] if " dinâmica " in f" {s} ")
    print(f"{len(files)} programas; {sum(r['com_requires'] for r in rows)} com sítios de `requires`")
    print(f"sítios descarregados: antes {sites('antes')}, depois {sites('depois')}; dinâmicos: antes {dyn('antes')}, depois {dyn('depois')}")
    print("mudaram:", mudou)


if __name__ == "__main__":
    main()
