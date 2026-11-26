#!/usr/bin/env python3
"""Diagnóstico posterior às correções: o compilador congelado (223fb53) × a versão nova, sobre os
mesmos arquivos, sem editar nenhum deles.

Para cada programa `.ath`: aceito ou não, códigos e mensagens dos diagnósticos, raiz do Codex e o
Python emitido (sha256), nos dois compiladores. Saída: JSON com tudo e um resumo em markdown.

    python3 bench/m11/diagnostico/reanalise_compilador.py --antigo /tmp/frozen-target/release/athc \
        --novo target/release/athc --out bench/m11/diagnostico/pos_correcoes
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import tempfile
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
AUTH = ROOT / "bench" / "m11" / "runs" / "authoring"
DIAG = re.compile(r"error\[([A-Z]\d{4})\]: (.*)\n\s*--> [^\n]*?:(\d+):(\d+)")


def run(athc: str, f: Path) -> dict:
    r = subprocess.run([athc, "check", str(f)], capture_output=True, text=True, timeout=60)
    out = r.stdout + r.stderr
    diags = [{"code": c, "msg": m.strip(), "at": [int(ln), int(col)]} for c, m, ln, col in DIAG.findall(out)]
    res = {"ok": r.returncode == 0, "diags": diags, "root": None, "emit": None}
    if res["ok"]:
        # a raiz do Codex (linha `raiz #…` do check) e a tabela inteira de objetos do `hash`
        m = re.search(r"raiz #([0-9a-f]+)", out)
        h = subprocess.run([athc, "hash", str(f)], capture_output=True, text=True, timeout=60)
        res["root"] = (m.group(1) if m else None, hashlib.sha256(h.stdout.encode()).hexdigest())
        with tempfile.TemporaryDirectory() as d:
            o = Path(d) / "p.py"
            e = subprocess.run([athc, "emit", str(f), "--target", "py", "--out", str(o)], capture_output=True, text=True,
                               timeout=60)
            res["emit"] = hashlib.sha256(o.read_bytes()).hexdigest() if e.returncode == 0 and o.exists() else None
    return res


def versions():
    for d in sorted(AUTH.glob("*/ath/rep*")):
        vs = sorted(d.glob("v*.ath"), key=lambda p: int(p.stem[1:]))
        for i, v in enumerate(vs):
            yield d, v, ("primeira" if i == 0 else "final" if i == len(vs) - 1 else "intermediária")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--antigo", required=True)
    ap.add_argument("--novo", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)

    # 1. as 16 autorias Aletheia do M11, todas as versões
    m11 = []
    for d, v, kind in versions():
        old, new = run(a.antigo, v), run(a.novo, v)
        m11.append({"autoria": f"{d.parent.parent.name}/rep{d.name[3:]}", "arquivo": v.name, "tipo": kind,
                    "vazio": not v.read_text(encoding="utf-8").strip(), "antigo": old, "novo": new})

    # 2. o corpus versionado fora das autorias: nada pode mudar de aceitação, raiz ou emissão
    corpus = []
    files = subprocess.run(["git", "-C", str(ROOT), "ls-files", "*.ath"], capture_output=True, text=True).stdout.split()
    for rel in files:
        if rel.startswith(("bench/m11/runs/", "bench/m11/diagnostico/")):
            continue
        f = ROOT / rel
        old, new = run(a.antigo, f), run(a.novo, f)
        corpus.append({"arquivo": rel, "antigo": old, "novo": new})

    md = resumo(m11, corpus)  # também anota a transição de cada versão
    (out / "reanalise.json").write_text(json.dumps({"m11": m11, "corpus": corpus}, ensure_ascii=False, indent=1),
                                        encoding="utf-8")
    (out / "reanalise.md").write_text(md, encoding="utf-8")
    print(md)


def first_code(r):
    return r["diags"][0]["code"] if r["diags"] else ("ok" if r["ok"] else "?")


def resumo(m11, corpus) -> str:
    L = ["# Reanálise: compilador congelado × versão nova", "",
         "Diagnóstico posterior às correções. Nenhum programa foi editado. Não diz como o modelo teria",
         "respondido às mensagens novas.", ""]
    by = {}
    for x in m11:
        by.setdefault(x["tipo"], []).append(x)
    L += ["| versões | compilam (congelado) | compilam (nova) |", "|---|---|---|"]
    for k in ("primeira", "intermediária", "final"):
        xs = by.get(k, [])
        L.append(f"| {k} ({len(xs)}) | {sum(x['antigo']['ok'] for x in xs)} | {sum(x['novo']['ok'] for x in xs)} |")
    L.append(f"| todas ({len(m11)}) | {sum(x['antigo']['ok'] for x in m11)} | {sum(x['novo']['ok'] for x in m11)} |")
    L += ["", "## Transições por versão (primeiro diagnóstico, congelado → nova)", ""]
    trans = Counter()
    for x in m11:
        o, n = x["antigo"], x["novo"]
        if o["ok"] and n["ok"]:
            t = "compilava e compila"
        elif o["ok"] and not n["ok"]:
            t = "compilava e deixou de compilar"
        elif not o["ok"] and n["ok"]:
            t = f"{first_code(o)} → compila"
        elif first_code(o) == first_code(n) and o["diags"][0]["msg"] == n["diags"][0]["msg"] \
                and o["diags"][0]["at"] != n["diags"][0]["at"]:
            # a mesma recusa, com o diagnóstico apontando outro lugar (E0422: a comparação responsável)
            t = f"{first_code(o)}, mesma recusa, o diagnóstico aponta outro lugar"
        elif first_code(o) != first_code(n) or o["diags"][0]["at"] != n["diags"][0]["at"]:
            # outro código ou outro lugar: o primeiro erro saiu e o seguinte ficou visível
            t = f"{first_code(o)} → {first_code(n)} (erro seguinte ficou visível)"
        elif o["diags"][0]["msg"] != n["diags"][0]["msg"]:
            t = f"{first_code(o)}, mesmo lugar, só a mensagem mudou"
        else:
            t = f"{first_code(o)}, mesmo primeiro diagnóstico"
        trans[t] += 1
        x["transicao"] = t
    L += ["| transição | versões |", "|---|---|"] + [f"| {t} | {n} |" for t, n in sorted(trans.items(), key=lambda z: -z[1])]
    L += ["", "## Por autoria", "", "| autoria | versões | congelado | nova |", "|---|---|---|---|"]
    for au in sorted({x["autoria"] for x in m11}):
        xs = [x for x in m11 if x["autoria"] == au]
        L.append(f"| {au} | {len(xs)} | " + " ".join(first_code(x["antigo"]) for x in xs) + " | "
                 + " ".join(first_code(x["novo"]) for x in xs) + " |")
    ch = [c for c in corpus if c["antigo"]["ok"] != c["novo"]["ok"] or c["antigo"]["root"] != c["novo"]["root"]
          or c["antigo"]["emit"] != c["novo"]["emit"] or [d["code"] for d in c["antigo"]["diags"]]
          != [d["code"] for d in c["novo"]["diags"]]]
    L += ["", f"## Corpus versionado ({len(corpus)} programas fora das autorias)", "",
          f"Aceitos: {sum(c['antigo']['ok'] for c in corpus)} congelado, {sum(c['novo']['ok'] for c in corpus)} nova. "
          f"Programas com mudança de aceitação, códigos, raiz do Codex ou Python emitido: {len(ch)}."]
    for c in ch:
        L.append(f"- `{c['arquivo']}`: {first_code(c['antigo'])} → {first_code(c['novo'])}")
    return "\n".join(L) + "\n"


if __name__ == "__main__":
    main()
