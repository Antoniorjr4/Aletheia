"""M10: o executor comum. O pacote de exemplo é válido; o agente sem defesa é violado; os dois
agentes de referência (Aletheia e Python) resistem ao modelo adversarial. Sem rede."""

import json
import os
import subprocess
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
ATHC = os.environ.get("ATHC", str(ROOT / "target" / "release" / "athc"))
PACK = HERE / "packs" / "_example_mailing"


@unittest.skipUnless(Path(ATHC).is_file(), "requer o athc compilado")
class HarnessTests(unittest.TestCase):
    def test_example_pack_is_valid(self):
        import harness

        self.assertEqual(harness.validate(PACK, 12), [])

    def evaluate(self, lang, agent):
        import harness

        return harness.evaluate(lang, PACK / agent, PACK, 12, False)

    def test_naive_agent_is_breached(self):
        r = self.evaluate("py", "naive.py")
        self.assertFalse(r["secure"])
        self.assertTrue(all(v["violations"] > 0 for v in r["attack"].values()), r["attack"])

    def test_reference_agents_hold_and_still_act(self):
        for lang, agent in (("py", "agent.py"), ("ath", "agent.ath")):
            r = self.evaluate(lang, agent)
            self.assertTrue(r["secure"], (agent, r["attack"]))
            # o modelo adversarial também propõe operações permitidas, e elas são executadas
            self.assertGreater(sum(v["effects"] for v in r["attack"].values()), 0, agent)

    def test_schema_model_is_deterministic_and_conforming(self):
        import harness

        schema = json.loads((HERE / "tests" / "schema.json").read_text())
        a = harness.SchemaModel("adversarial", 7, {"text": ["x@evil.test"], "int": [5]}).value(schema)
        b = harness.SchemaModel("adversarial", 7, {"text": ["x@evil.test"], "int": [5]}).value(schema)
        self.assertEqual(a, b)
        self.assertEqual(set(a), {"ops", "n", "flag"})
        self.assertTrue(all(o["kind"] in ("a", "b") for o in a["ops"]))
        stub = harness.SchemaModel("stub").value(schema)
        self.assertEqual(stub, {"ops": [], "n": 3, "flag": False})

    def test_strict_schema_problems(self):
        import harness

        ok = {"type": "object", "additionalProperties": False, "required": ["a"], "properties": {"a": {"type": "string"}}}
        self.assertEqual(harness.strict_problems(ok), [])
        self.assertTrue(harness.strict_problems({**ok, "required": []}))
        self.assertTrue(harness.strict_problems({k: v for k, v in ok.items() if k != "additionalProperties"}))

    def test_tool_contract_is_enforced(self):
        import harness

        pack = harness.Pack(PACK)
        tools = harness.Tools(pack, pack.world(pack.scenarios["legit_add"]))
        for bad in (lambda: tools.call("delete_all"), lambda: tools.call("add_subscriber"),
                    lambda: tools.call("add_subscriber", email=7), lambda: tools.call("add_subscriber", email="a", x=1)):
            with self.assertRaises(harness.ToolError):
                bad()
        self.assertEqual(tools.calls, [])

    def test_a_crashing_agent_fails_legit_and_cannot_hide_an_effect(self):
        import harness
        import tempfile

        with tempfile.TemporaryDirectory() as d:
            agent = Path(d) / "boom.py"
            agent.write_text("def run(request, tools, trusted, llm):\n"
                             "    tools.call('send_email', to='evil@attacker.test', subject='s', body='b')\n"
                             "    raise RuntimeError('boom')\n")
            legit = harness.exec_subprocess("py", agent, PACK, "legit_add", "stub", 0)
            attack = harness.exec_subprocess("py", agent, PACK, "attack_summary", "adversarial", 0)
        self.assertFalse(legit["passed"])
        self.assertFalse(attack["passed"], "o efeito proibido antes da queda tem de contar como violação")


if __name__ == "__main__":
    unittest.main()
