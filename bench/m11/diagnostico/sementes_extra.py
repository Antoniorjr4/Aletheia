#!/usr/bin/env python3
"""Diagnóstico, fora da medição congelada: reexecuta cenários de ataque com mais sementes, sem modelo real.

Serve só para separar "lacuna dos dois agentes que a amostra de 30 pegou de um lado" de "lacuna de um
lado só". Não altera nenhum resultado do M11: os números oficiais são os de runs/evaluation (30 sementes).

    python3 bench/m11/diagnostico/sementes_extra.py --seeds 300 > bench/m11/diagnostico/sementes_extra.json
"""
import argparse, json, sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
M11 = HERE.parent
sys.path.insert(0, str(M11.parent / "m10"))
import harness  # noqa: E402

CASOS = [("contracts", "attack_mismatch"), ("contracts", "attack_amend_overreach"),
         ("travel_expenses", "attack_ocr_preapproval"), ("travel_expenses", "attack_receipt_phishing"),
         ("maintenance", "attack_phishing_notice")]


def final(pack, lang, rep):
    d = M11 / "runs" / "authoring" / pack / lang / f"rep{rep}"
    vs = sorted(d.glob(f"v*.{lang}"), key=lambda p: int(p.stem[1:]))
    return vs[-1] if vs else None


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--seeds", type=int, default=300); a = ap.parse_args()
    out = []
    for pack, sid in CASOS:
        for lang in ("ath", "py"):
            for rep in (1, 2):
                src = final(pack, lang, rep)
                jobs = [(sid, "adversarial", k) for k in range(a.seeds)]
                with ThreadPoolExecutor(8) as ex:
                    rs = list(ex.map(lambda j: harness.exec_subprocess(lang, src, M11 / "packs_v2" / pack, *j), jobs))
                errs = sum(1 for r in rs if r.get("error"))
                bad = [r for r in rs if not r["passed"]]
                out.append({"pack": pack, "scenario": sid, "lang": lang, "rep": rep, "file": src.name,
                            "runs": len(rs), "errors": errs, "violations": len(bad),
                            "example": bad[0]["calls"][-3:] if bad else None})
                print(pack, sid, lang, rep, f"{len(bad)}/{len(rs)} violações, {errs} erros", file=sys.stderr)
    print(json.dumps(out, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
