#!/usr/bin/env python3
"""M11: o executor do M10 (bench/m10, congelado) apontado para os pacotes, guias e saídas do M11.

Nada do M10 é alterado: este arquivo só troca os caminhos que o M10 lê.

    python3 bench/m11/run.py author --packs a,b --reps 3 --max-usd-total 0.42
    python3 bench/m11/run.py eval --real --max-usd 0.18
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
M10 = HERE.parent / "m10"
PACKS = HERE / "packs_v2" if (HERE / "packs_v2").exists() else HERE / "packs"
os.environ["M10_PACKS"] = str(PACKS)
sys.path.insert(0, str(M10))


def main() -> int:
    cmd, rest = sys.argv[1], sys.argv[2:]
    if cmd == "author":
        import author

        author.HERE = HERE  # os guias v2 em bench/m11/guides; nada mais em `author` usa HERE em tempo de execução
        sys.argv = ["author.py", *rest, "--out", str(HERE / "runs" / "authoring")]
        return author.main()
    if cmd == "eval":
        import evaluate_all

        sys.argv = ["evaluate_all.py", *rest, "--authoring", str(HERE / "runs" / "authoring"), "--packs",
                    str(PACKS), "--out", str(HERE / "runs" / "evaluation")]
        return evaluate_all.main()
    raise SystemExit(f"comando desconhecido: {cmd}")


if __name__ == "__main__":
    sys.exit(main())
