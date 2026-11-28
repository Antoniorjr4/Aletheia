#!/usr/bin/env python3
"""Diagnóstico posterior às correções: o avaliador congelado do M10 (sem `--real`, 30 sementes por
cenário e modo, as mesmas entradas) sobre TODAS as versões Aletheia arquivadas das 16 autorias —
primeiras, intermediárias e finais —, com o compilador apontado por ATHC. O Python não é
reavaliado: seus números de referência são os de bench/m11/runs/evaluation.

    ATHC=target/release/athc python3 bench/m11/diagnostico/avalia_todas_versoes.py
"""
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
M11 = HERE.parent
sys.path.insert(0, str(M11.parent / "m10"))
import evaluate_all  # noqa: E402

OUT = HERE / "pos_correcoes" / "evaluation"


def main():
    ev = evaluate_all.Evaluator(M11 / "packs_v2", OUT, 30, False, 8)
    rows = []
    for d in sorted((M11 / "runs" / "authoring").glob("*/ath/rep*")):
        vs = sorted(d.glob("v*.ath"), key=lambda p: int(p.stem[1:]))
        for i, v in enumerate(vs):
            pack = d.parent.parent.name
            s = evaluate_all.summarize_version(ev.version("ath", v, pack))
            kind = "primeira" if i == 0 else "final" if i == len(vs) - 1 else "intermediária"
            estado = ("não executa" if not s["runnable"] else
                      "executa e viola" if s["violated"] or s["violated_malformed"] else
                      "executa sem violação observada")
            rows.append({"autoria": f"{pack}/{d.name}", "arquivo": v.name, "tipo": kind, "estado": estado, **s})
            print(rows[-1]["autoria"], v.name, kind, estado, f"{s['violated']}/{s['attack_scenarios']}", flush=True)
    (OUT / "todas_versoes.json").write_text(json.dumps(rows, ensure_ascii=False, indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()
