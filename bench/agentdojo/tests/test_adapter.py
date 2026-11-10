"""Offline tests of the fail-closed tool mapping, independent of AgentDojo installation."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from adapter import BoundTool, authorize_tool, manifest


class AdapterTests(unittest.TestCase):
    def test_unknown_tool_denied(self):
        d = authorize_tool("banking", "transfer_money", frozenset({"Bank.Write"}), {}, frozenset())
        self.assertFalse(d.allowed)

    def test_no_approval_denied(self):
        bindings = {"read": BoundTool("read", "Bank.Read", False, "reviewed")}
        d = authorize_tool("banking", "read", frozenset({"Bank.Read"}), bindings, frozenset())
        self.assertFalse(d.allowed)

    def test_missing_capability_denied(self):
        bindings = {"read": BoundTool("read", "Bank.Read", False, "reviewed")}
        d = authorize_tool("banking", "read", frozenset(), bindings, frozenset({"reviewed"}))
        self.assertFalse(d.allowed)

    def test_explicit_capability_and_policy(self):
        bindings = {"read": BoundTool("read", "Bank.Read", False, "reviewed")}
        d = authorize_tool("banking", "read", frozenset({"Bank.Read"}), bindings, frozenset({"reviewed"}))
        self.assertTrue(d.allowed)

    def test_manifest_read_only(self):
        class Suite:
            user_tasks = {"user_task_1": None}
            injection_tasks = {"injection_task_1": None}
            tools = {"get_data": None}
        m = manifest("workspace", Suite())
        self.assertEqual(m["results"], None)
        self.assertEqual(m["user_tasks"], ["user_task_1"])
        self.assertEqual(m["adapter_status"], "metadata_only_no_agent_execution")


if __name__ == "__main__":
    unittest.main()


class DiscoveryScript(unittest.TestCase):
    """O script de descoberta roda direto pelo caminho documentado no README.

    Ele se chamava `inspect.py` e sombreava o módulo `inspect` da biblioteca padrão: o
    `dataclasses` importado pelo `adapter` carregava o próprio script, num import circular.
    """

    def test_runs_by_its_documented_path(self):
        import subprocess

        script = Path(__file__).resolve().parents[1] / "discover.py"
        r = subprocess.run([sys.executable, str(script), "--help"], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("--suite", r.stdout)

    def test_no_module_shadows_the_standard_library(self):
        import sys as _sys

        here = Path(__file__).resolve().parents[1]
        names = {p.stem for p in here.glob("*.py")}
        self.assertFalse(names & set(_sys.stdlib_module_names), names & set(_sys.stdlib_module_names))
