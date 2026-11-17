"""Ciclo 1, D12: os 11 pares em que o Aletheia v2 keeping respondeu "não tenho os dados".

Nos mesmos pares, o P1 acertou (runs/banking-v2-p1-openai-gpt-oss-120b). Estes testes são
determinísticos e sem custo. Reconstroem o prompt de plano com o runtime ATUAL, a partir das
injeções exatas registradas no log oficial e das leituras que o modelo escolheu naquela execução.
Eles separam o que é da linguagem do que é comportamento do modelo:

* os dados que a tarefa precisa estão no prompt, dentro do bloco de dado (não foi truncamento
  nem leitura errada do programa);
* a mensagem de sistema e o cabeçalho não citam a linguagem (a mudança de prompt do D12);
* os cabeçalhos que o programa escreve dentro do dado não são alterados pela neutralização do D9.

Se a mudança de prompt resolveu o comportamento do modelo, isso só uma rodada real dirá
(d12/casos.json; estimativa em docs/CICLO-1.md).
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
import warnings
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(ROOT / "runtimes" / "python"))
warnings.filterwarnings("ignore")
ATHC = os.environ.get("ATHC", str(ROOT / "target" / "release" / "athc"))
RUN = HERE / "runs" / "banking-v2-p1-openai-gpt-oss-120b" / "aletheia-v2-keeping-gpt-oss-120b" / "banking"

try:
    from agentdojo.agent_pipeline import AgentPipeline, InitQuery, SystemMessage
    from agentdojo.task_suite.load_suites import get_suite

    HAVE = Path(ATHC).is_file()
except ImportError:  # pragma: no cover
    HAVE = False

NEEDS = {"user_task_1": "2022-03", "user_task_7": "gift"}


@unittest.skipUnless(HAVE, "requer o pacote agentdojo e o athc compilado")
class D12Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from aletheia_pipeline import AletheiaAgent
        from aletheia_runtime.providers import Scripted

        cls.tmp = tempfile.TemporaryDirectory()
        prog = Path(cls.tmp.name) / "bk.py"
        subprocess.run([ATHC, "emit", str(HERE / "banking_v2_keeping.ath"), "--target", "py", "--out", str(prog)],
                       check=True, capture_output=True)
        suite = get_suite("v1.2.2", "banking")
        cls.cases = json.loads((HERE / "d12" / "casos.json").read_text())["casos"]
        cls.prompts = {}
        for c in cls.cases:
            log = json.loads((RUN / c["user_task"] / "important_instructions" / f"{c['injection_task']}.json").read_text())
            seen: list = []
            reads = {"kind": "Reads", **c["reads"]}

            class Spy:
                n = 0

                def name(self):
                    return "scripted"

                def complete(self, req):
                    seen.append(req.prompt)
                    Spy.n += 1
                    out = reads if Spy.n == 1 else {"kind": "Plan", "writes": [], "answer": "x"}
                    return Scripted({req.model: [out]}).complete(req)

            agent = AletheiaAgent("banking", str(prog), lambda: Spy(), "Banker", "task")
            p = AgentPipeline([SystemMessage("x"), InitQuery(), agent])
            p.name = "d12"
            suite.run_task_with_pipeline(p, suite.get_user_task_by_id(c["user_task"]),
                                         suite.get_injection_task_by_id(c["injection_task"]), log["injections"])
            cls.prompts[(c["user_task"], c["injection_task"])] = seen[1]

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_eleven_cases(self):
        self.assertEqual(len(self.cases), 11)

    def test_the_data_the_task_needs_is_inside_the_data_block(self):
        for (u, i), p in self.prompts.items():
            data = p[p.index("<<<"):p.rindex(">>>")]
            self.assertIn(NEEDS[u], data.lower(), (u, i))
            self.assertLess(len(data), 7500, (u, i))  # sem truncamento pelo fit(…, 7500)

    def test_prompt_and_system_message_do_not_name_the_language(self):
        from aletheia_runtime import providers

        self.assertNotIn("Aletheia", providers.SYSTEM)
        for k, p in self.prompts.items():
            self.assertNotIn("Aletheia", p, k)

    def test_program_headers_inside_the_data_are_untouched(self):
        for k, p in self.prompts.items():
            self.assertIn("\n## recent transactions\n", p, k)
            self.assertNotIn("＃", p.split("<<<", 1)[0], k)


if __name__ == "__main__":
    unittest.main()
