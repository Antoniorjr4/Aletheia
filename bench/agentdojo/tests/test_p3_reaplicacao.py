"""Pós-P3: os pedidos C2, C3, C5 e C6 reaplicados só no programa Aletheia, com o host intacto.

Diagnóstico de regressão sobre casos conhecidos, não evidência nova: o resultado do P3 (78 × 50
linhas) continua registrado em `p3/PROTOCOLO.md`. O que este teste fixa é que as quatro regras que
tinham ido para o host agora são decididas pela política, com `aletheia_pipeline.py` sem mudança.
"""

from __future__ import annotations

import os
import subprocess
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
ROOT = HERE.parents[1]
ATHC = os.environ.get("ATHC", str(ROOT / "target" / "release" / "athc"))

try:
    import agentdojo  # noqa: F401

    HAVE = Path(ATHC).is_file()
except ImportError:  # pragma: no cover
    HAVE = False


@unittest.skipUnless(HAVE, "requer o pacote agentdojo e o athc compilado")
class ReaplicacaoTests(unittest.TestCase):
    def test_the_four_host_rules_are_now_decided_by_the_policy(self):
        for change, n in (("C2", 5), ("C3", 5), ("C5", 5), ("C6", 4)):
            r = subprocess.run(
                [sys.executable, str(HERE / "p3" / "aceite.py"), "--impl", "A", "--dir", str(HERE), "--cases",
                 str(HERE / "p3" / "pedidos.json"), "--only", change, "--program", f"p3/reaplicacao/{change}.ath"],
                capture_output=True, text=True, env=dict(os.environ, ATHC=ATHC))
            self.assertEqual(r.returncode, 0, change + "\n" + r.stdout + r.stderr)
            self.assertIn(f"{n}/{n} casos passam", r.stdout, change)

    def test_the_base_program_does_not_satisfy_them(self):
        # controle: sem a mudança, os mesmos casos falham (o teste acima não passa por acaso)
        for change in ("C2", "C3", "C5", "C6"):
            r = subprocess.run(
                [sys.executable, str(HERE / "p3" / "aceite.py"), "--impl", "A", "--dir", str(HERE), "--cases",
                 str(HERE / "p3" / "pedidos.json"), "--only", change],
                capture_output=True, text=True, env=dict(os.environ, ATHC=ATHC))
            self.assertEqual(r.returncode, 1, change + "\n" + r.stdout)


if __name__ == "__main__":
    unittest.main()
