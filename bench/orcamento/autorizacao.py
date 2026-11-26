#!/usr/bin/env python3
"""Trava de autorização para os jobs pagos: falha antes de qualquer requisição se o pedido passar do
gasto adicional autorizado em AUTORIZACAO.json. Com autorização 0, todo job pago falha aqui.

    python3 bench/orcamento/autorizacao.py --pedido 0.18
"""
import argparse
import json
import sys
from pathlib import Path

AUT = Path(__file__).resolve().parent / "AUTORIZACAO.json"


def verifica(pedido: float, aut: dict) -> str | None:
    livre = float(aut["usd_adicional_autorizado"])
    if livre <= 0:
        return f"nenhum gasto adicional autorizado ({aut['fonte']})"
    if pedido > livre:
        return f"pedido de US$ {pedido} passa do autorizado (US$ {livre})"
    return None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--pedido", type=float, required=True)
    erro = verifica(ap.parse_args().pedido, json.loads(AUT.read_text(encoding="utf-8")))
    if erro:
        print(f"::error::{erro}")
        return 1
    print("autorizado")
    return 0


if __name__ == "__main__":
    sys.exit(main())
