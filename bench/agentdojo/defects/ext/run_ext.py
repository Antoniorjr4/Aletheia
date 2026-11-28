#!/usr/bin/env python3
"""M4-ext: roda os casos de `catalog_ext.py` com o detector do M4 e classifica também o efeito em
execução, por execução do modelo obediente (sem rede, sem modelo real).

    python3 bench/agentdojo/defects/ext/run_ext.py --out bench/agentdojo/runs/m4-ext

Detecção (o primeiro estágio que pega, como no M4): estático · testes congelados · não detectado.
Efeito em execução (só para versões que executam), por execução do modelo obediente nos três
braços, comparado com a mesma execução da base sem defeito:
  * efeito proibido executado (o oráculo de ataque da suíte marca a execução);
  * recusa antes de qualquer efeito (erro na execução, nenhuma escrita chegou ao banco);
  * recusa depois de efeitos legítimos (erro, escritas antes dele, ataque não cumprido);
  * sem recusa.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEF = HERE.parent
BENCH = DEF.parent
sys.path.insert(0, str(DEF))
sys.path.insert(0, str(HERE))
import run_m4  # noqa: E402
from catalog_ext import ATH, DEFECTS  # noqa: E402


def obedient_rows(extra: list, env=None) -> dict:
    with tempfile.TemporaryDirectory() as out:
        r = subprocess.run([sys.executable, str(BENCH / "obedient.py"), *extra, "--out", out],
                           capture_output=True, text=True, env=env, timeout=1800)
        if r.returncode != 0:
            return {"ran": False, "stderr": r.stderr[-600:]}
        rows = [json.loads(x) for x in (Path(out) / "rows.jsonl").read_text().splitlines()]
    return {"ran": True, "rows": rows}


def key(r):
    return (r.get("arm"), r.get("user_task"), r.get("injection_task"))


def classify(rows: list) -> Counter:
    c = Counter()
    for r in rows:
        a = r["aletheia"]
        if a["attack"]:
            c["efeito proibido executado"] += 1
        elif a["error"] is not None and not a["executed"]:
            c["recusa antes de qualquer efeito"] += 1
        elif a["error"] is not None:
            c["recusa depois de efeitos legítimos"] += 1
        else:
            c["sem recusa"] += 1
    return c


def delta(base: list, mut: list) -> dict:
    b = {key(r): r for r in base}
    changed = [k for k in (key(r) for r in mut) if k in b]
    diff = {"execucoes": len(mut), "utilidade_diferente": 0, "escritas_diferentes": 0, "ataque_diferente": 0}
    m = {key(r): r for r in mut}
    for k in changed:
        x, y = b[k]["aletheia"], m[k]["aletheia"]
        diff["utilidade_diferente"] += x["utility"] != y["utility"]
        diff["escritas_diferentes"] += x["executed"] != y["executed"]
        diff["ataque_diferente"] += x["attack"] != y["attack"]
    return diff


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    base = {"ath": obedient_rows(["--program", str(BENCH / ATH)]),
            "py": obedient_rows(["--impl", "hardened-keeping"])}
    results = []
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        base_py = tmp / "base" / "hardened"
        shutil.copytree(BENCH / "hardened", base_py)
        baseline = run_m4.py_diags(base_py)
        for d in DEFECTS:
            res = {"id": d["id"], "kind": d["kind"], "title": d["title"]}
            # Aletheia
            w = tmp / d["id"] / "ath"
            w.mkdir(parents=True)
            shutil.copy(BENCH / ATH, w / ATH)
            run_m4.apply(d["ath"], w)
            rejected, codes, text = run_m4.ath_static(w / ATH)
            ath = {"codes": codes, "check_tail": text if rejected else ""}
            if rejected:
                ath["stage"] = "estático"
            else:
                o = obedient_rows(["--program", str(w / ATH)])
                agg = run_m4.obedient(["--program", str(w / ATH)])
                ath["obedient"] = agg
                ath["stage"] = "testes" if (not agg["ran"]) or any(agg[k] != v for k, v in run_m4.EXPECT.items()) \
                    else "não detectado"
                if o["ran"]:
                    ath["execucao"] = dict(classify(o["rows"]))
                    ath["base_execucao"] = dict(classify(base["ath"]["rows"]))
                    ath["diferenca_da_base"] = delta(base["ath"]["rows"], o["rows"])
            res["ath"] = ath
            # P1
            pw = tmp / d["id"] / "py"
            shutil.copytree(BENCH / "hardened", pw / "hardened")
            run_m4.apply(d["py"], pw)
            new = run_m4.py_diags(pw / "hardened") - baseline
            py = {"new_diagnostics": sorted(new)[:12]}
            env = {**os.environ, "P1_DIR": str(pw / "hardened")}
            o = obedient_rows(["--impl", "hardened-keeping"], env=env)
            if new:
                py["stage"] = "estático"
            else:
                t = subprocess.run([sys.executable, "-m", "unittest", str(BENCH / "tests" / "test_hardened.py")],
                                   capture_output=True, text=True, env=env, timeout=1800)
                py["tests_tail"] = t.stderr[-800:] if t.returncode else ""
                py["stage"] = "testes" if t.returncode else "não detectado"
            if o["ran"]:
                py["execucao"] = dict(classify(o["rows"]))
                py["base_execucao"] = dict(classify(base["py"]["rows"]))
                py["diferenca_da_base"] = delta(base["py"]["rows"], o["rows"])
            res["py"] = py
            results.append(res)
            print(f"{d['id']}: Aletheia {ath['stage']} {codes} · P1 {py['stage']}", flush=True)
    (out / "m4-ext.json").write_text(json.dumps(results, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
