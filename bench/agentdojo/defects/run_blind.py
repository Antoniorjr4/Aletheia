#!/usr/bin/env python3
"""M4 com o catálogo CEGO: defeitos escritos por um agente que só viu o P1, traduzidos para a
Aletheia por outro agente que não viu os resultados (defects/blind/).

Categorias da tradução: `program` (o erro cabe no programa .ath), `host` (no código Python do
host que o usuário da Aletheia também escreve), `none` (o mecanismo mora no compilador/runtime
da linguagem: o desenvolvedor do agente não consegue introduzir esse erro; um bug equivalente
pode existir no próprio runtime).

Estágios: estático (`athc check` para o .ath; `ruff` + `mypy --strict`, diagnósticos novos, para
código Python, inclusive o host da Aletheia) · testes congelados (P1: tests/test_hardened.py;
Aletheia: expectativas do modelo obediente + tests/test_pipeline.py) · não detectado.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent
sys.path.insert(0, str(HERE))
from run_m4 import EXPECT, ath_static, obedient  # noqa: E402

PYFILES = ("banking_hardened.py", "agentdojo_adapter.py")


def diags(files: list[Path], cwd: Path) -> Counter:
    import re
    out = []
    r = subprocess.run(["ruff", "check", "--output-format", "concise", "--no-cache", *map(str, files)],
                       capture_output=True, text=True)
    out += [re.sub(r"^.*?:\d+:\d+: ", "ruff: ", x) for x in r.stdout.splitlines() if re.match(r".*:\d+:\d+: ", x)]
    r = subprocess.run(["mypy", "--strict", "--ignore-missing-imports", "--no-error-summary", "--hide-error-context",
                        "--no-incremental", *map(str, files)], capture_output=True, text=True, cwd=cwd)
    out += [re.sub(r"^.*?:\d+: ", "mypy: ", x) for x in r.stdout.splitlines() if re.match(r".*:\d+: ", x)]
    return Counter(out)


def apply(edits, files: dict[str, Path]):
    for e in edits:
        p = files[e["file"].split("/")[-1]]
        s = p.read_text()
        if s.count(e["old"]) != 1:
            raise SystemExit(f"edição ambígua: {e['file']} {e['old'][:60]!r}")
        p.write_text(s.replace(e["old"], e["new"]))


def main() -> int:
    py_cat = {d["id"]: d for d in json.loads((HERE / "blind" / "catalog_py.json").read_text())}
    ath_cat = {d["id"]: d for d in json.loads((HERE / "blind" / "catalog_ath.json").read_text())}
    results = []
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        b = tmp / "base"
        shutil.copytree(BENCH / "hardened", b / "hardened")
        shutil.copy(BENCH / "aletheia_pipeline.py", b / "aletheia_pipeline.py")
        base_p1 = diags([b / "hardened" / f for f in PYFILES], b / "hardened")
        base_host = diags([b / "aletheia_pipeline.py"], b)
        for i in sorted(py_cat):
            pd, ad = py_cat[i], ath_cat[i]
            res = {"id": i, "title": pd["title"], "control": pd["control"], "ath_category": ad["category"],
                   "ath_note": ad.get("justification", "")}
            # P1
            w = tmp / i / "p1"
            shutil.copytree(BENCH / "hardened", w / "hardened")
            apply(pd["edits"], {f: w / "hardened" / f for f in PYFILES})
            new = diags([w / "hardened" / f for f in PYFILES], w / "hardened") - base_p1
            env = {**os.environ, "P1_DIR": str(w / "hardened")}
            p1 = {"new_diagnostics": sorted(new)[:10], "obedient": obedient(["--impl", "hardened-keeping"], env=env)}
            if new:
                p1["stage"] = "estático"
            else:
                t = subprocess.run([sys.executable, "-m", "unittest", str(BENCH / "tests" / "test_hardened.py")],
                                   capture_output=True, text=True, env=env, timeout=900)
                p1["tests_tail"] = t.stderr[-600:] if t.returncode else ""
                p1["stage"] = "testes" if t.returncode else "não detectado"
            res["p1"] = p1
            # Aletheia
            if ad["category"] == "none":
                res["ath"] = {"stage": "não exprimível no programa"}
            elif not ad.get("edits"):
                res["ath"] = {"stage": "base já defeituosa"}
            else:
                w = tmp / i / "ath"
                w.mkdir(parents=True)
                shutil.copy(BENCH / "banking_v2_keeping.ath", w / "banking_v2_keeping.ath")
                shutil.copy(BENCH / "aletheia_pipeline.py", w / "aletheia_pipeline.py")
                apply(ad["edits"], {"banking_v2_keeping.ath": w / "banking_v2_keeping.ath",
                                    "aletheia_pipeline.py": w / "aletheia_pipeline.py"})
                rejected, codes, text = ath_static(w / "banking_v2_keeping.ath")
                hostnew = diags([w / "aletheia_pipeline.py"], w) - base_host
                ath = {"codes": codes, "host_new_diagnostics": sorted(hostnew)[:10],
                       "check_errors": [x for x in text.splitlines() if "error[" in x][:4] if rejected else []}
                if rejected or hostnew:
                    ath["stage"] = "estático"
                else:
                    env = {**os.environ, "ALETHEIA_HOST_DIR": str(w)}
                    o = obedient(["--program", str(w / "banking_v2_keeping.ath")], env=env)
                    ath["obedient"] = o
                    bad = (not o["ran"]) or any(o[k] != v for k, v in EXPECT.items())
                    t = subprocess.run([sys.executable, "-m", "unittest", str(BENCH / "tests" / "test_pipeline.py")],
                                       capture_output=True, text=True, env=env, timeout=900)
                    ath["tests_tail"] = t.stderr[-600:] if t.returncode else ""
                    ath["stage"] = "testes" if (bad or t.returncode) else "não detectado"
                res["ath"] = ath
            results.append(res)
            print(i, "Aletheia:", res["ath"]["stage"], res["ath"].get("codes", ""), "· P1:", p1["stage"], flush=True)
    out = BENCH / "runs" / "m4"
    (out / "blind.json").write_text(json.dumps(results, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
