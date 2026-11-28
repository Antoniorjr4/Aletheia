#!/usr/bin/env python3
"""Aceitação de todo programa `.ath` versionado, com o compilador anterior à E0453 e o atual.

    python3 bench/agentdojo/x2/e0453/corpus.py --antes /tmp/athc-antes-e0453 --out bench/agentdojo/x2/e0453/corpus.json

`--antes` é o `athc` do commit 2950dc5 (protótipo sem efeito na aceitação). Para cada programa
cuja aceitação mudou, registra os códigos novos e o que a análise de obrigações afirmava antes.
Desde a decisão final de 28/11 a E0453 é aviso: registra também os programas com `aviso[E0453]`.
"""
import argparse, json, re, subprocess
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]


def check(athc, f):
    r = subprocess.run([athc, "check", f], capture_output=True, text=True, cwd=ROOT)
    t = r.stdout + r.stderr
    return r.returncode == 0, sorted(set(re.findall(r"error\[(E\d{4})\]", t))), "aviso[E0453]" in t


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--antes", required=True)
    ap.add_argument("--depois", default=str(ROOT / "target" / "release" / "athc"))
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    files = subprocess.run(["git", "ls-files", "*.ath"], capture_output=True, text=True, cwd=ROOT).stdout.split()
    groups, changed, warned = Counter(), [], []
    for f in files:
        (ok0, _, _), (ok1, codes, warn) = check(a.antes, f), check(a.depois, f)
        if warn:
            warned.append(f)
        g = "m10/m11 autorias" if re.match(r"bench/m1[01]/", f) else f.split("/")[0] + "/" + f.split("/")[1]
        groups[(g, "aceito" if ok0 else "recusado", "aceito" if ok1 else "recusado")] += 1
        if ok0 != ok1:
            ob = subprocess.run([a.antes, "obligations", f], capture_output=True, text=True, cwd=ROOT).stdout
            changed.append({"programa": f, "antes": ok0, "depois": ok1, "codigos": codes,
                            "obrigacoes_antes": [x.strip() for x in ob.splitlines() if x.startswith("  ")]})
    out = {"programas": len(files),
           "grupos": [{"grupo": g, "antes": x, "depois": y, "n": n} for (g, x, y), n in sorted(groups.items())],
           "mudaram": changed, "com_aviso_e0453": warned}
    Path(a.out).write_text(json.dumps(out, ensure_ascii=False, indent=1))
    print(f"{len(files)} programas; {len(changed)} mudaram de aceitação; {len(warned)} com aviso E0453")
    for c in changed:
        print(" ", c["programa"], c["codigos"], "|", "; ".join(c["obrigacoes_antes"])[:100])


if __name__ == "__main__":
    main()
