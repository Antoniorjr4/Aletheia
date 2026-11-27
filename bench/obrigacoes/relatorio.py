#!/usr/bin/env python3
"""Reproduções de B1 e B2 (análise de obrigações) e seus controles, nos dois runtimes.

Para cada caso: o que o compilador afirma (`athc obligations`), o que cada runtime faz (ARX e o
Python emitido), quais chamadas chegam à ferramenta, quais contratos foram conferidos e recusados,
e se houve efeito legítimo antes de uma recusa. Sem rede, sem modelo real (roteiro `scripted`).

    python3 bench/obrigacoes/relatorio.py --athc target/release/athc --out bench/obrigacoes/depois.json
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
C = HERE / "casos"

# (programa, mundo, roteiro, o que o caso mostra)
CASOS = [
    ("b1_cruzado", "mundo_pairs", "roteiro_dois_validos", "B1: destinatário e memo de elementos diferentes"),
    ("c1_mesmo_elemento", "mundo_pairs", "roteiro_dois_validos", "controle B1: mesmo elemento, plano válido"),
    ("c1_mesmo_elemento", "mundo_pairs", "roteiro_um_invalido", "controle B1: mesmo elemento, um elemento viola"),
    ("c1_alias", "mundo_pairs", "roteiro_dois_validos", "controle B1: mesmo elemento por `let x = o`"),
    ("c1_laco_interno_sem_uso", "mundo_pairs", "roteiro_dois_validos", "controle B1: laço interno sem uso"),
    ("b2_as_sombreia", "mundo_payees", "roteiro_sombra", "B2: `as fora` sombreia uma lista endossada"),
    ("b2_as_recusado_valido", "mundo_payees", "roteiro_sombra_recusado_valido",
     "B2: o recusado satisfaz o `requires`"),
    ("c2_sem_sombra", "mundo_payees", "roteiro_sem_sombra", "controle B2: plano mantido, sem sombra"),
]


def journal(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return [json.loads(x) for x in path.read_text(encoding="utf-8").splitlines() if x.strip()]


def resume(j: list[dict], final) -> dict:
    tools = [e["detail"] for e in j if e["kind"] == "tool"]
    calls = [[a for a in t["args"] if not isinstance(a, dict)] for t in tools]
    refused = [e["detail"] for e in j if e["kind"] == "contract_refused"]
    aborts = [e["detail"]["code"] for e in j if e["kind"] == "abort"]
    # a obrigação levantada conferida sobre o valor proposto, no `refine`
    lifted = [e["detail"] for e in j if e["kind"] == "obligation_reject"]
    return {"chamadas_na_ferramenta": calls, "contrato_recusado_no_efeito": refused, "aborto": aborts,
            "obrigacao_recusada_no_refine": len(lifted), "resultado": final,
            "eventos": sorted({e["kind"] for e in j})}


def run_case(athc: str, prog: str, world: str, models: str) -> dict:
    f, w, m = C / f"{prog}.ath", C / f"{world}.json", C / f"{models}.json"
    ob = subprocess.run([athc, "obligations", str(f)], capture_output=True, text=True)
    claim = [ln.strip() for ln in ob.stdout.splitlines() if ln.startswith("  ")]
    env = {k: v for k, v in os.environ.items() if k not in ("OPENAI_API_KEY", "GROQ_API_KEY", "OPENAI_BASE_URL")}
    with tempfile.TemporaryDirectory() as d:
        d = Path(d)
        r = subprocess.run([athc, "run", str(f), "--world", str(w), "--models", str(m), "--provider", "scripted",
                            "--arg", '"x"', "--journal", str(d / "arx.jsonl")], capture_output=True, text=True, env=env)
        out = [ln.strip() for ln in (r.stdout + r.stderr).splitlines() if "resultado" in ln or "abortado" in ln]
        arx = resume(journal(d / "arx.jsonl"), out[-1] if out else None)
        subprocess.run([athc, "emit", str(f), "--target", "py", "--out", str(d / "p.py")], check=True,
                       capture_output=True)
        env["PYTHONPATH"] = str(ROOT / "runtimes" / "python")
        subprocess.run(["python3", "-m", "aletheia_runtime", "run", str(d / "p.py"), "--world", str(w), "--models",
                        str(m), "--provider", "scripted", "--arg-json", '"x"', "--journal", str(d / "py.jsonl"),
                        "--report", str(d / "r.json")], capture_output=True, text=True, env=env)
        rep = json.loads((d / "r.json").read_text()) if (d / "r.json").exists() else {}
        py = resume(journal(d / "py.jsonl"), rep.get("result") if rep.get("ok") else rep.get("error"))
        heads = (journal(d / "arx.jsonl")[-1:] or [{}])[0].get("hash"), rep.get("journal_head")
    return {"compilador_afirma": claim, "arx": arx, "python": py,
            "runtimes_concordam": arx["chamadas_na_ferramenta"] == py["chamadas_na_ferramenta"]
            and arx["aborto"] == py["aborto"], "journals_identicos": heads[0] is not None and heads[0] == heads[1]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--athc", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    out = []
    for prog, world, models, what in CASOS:
        r = run_case(a.athc, prog, world, models)
        out.append({"caso": prog, "mundo": world, "roteiro": models, "mostra": what, **r})
        print(f"{prog} / {models}: {r['compilador_afirma']} | ARX {r['arx']['chamadas_na_ferramenta']} "
              f"{r['arx']['aborto']} | PY {r['python']['chamadas_na_ferramenta']} {r['python']['aborto']} | "
              f"concordam={r['runtimes_concordam']}")
    Path(a.out).write_text(json.dumps(out, ensure_ascii=False, indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()
