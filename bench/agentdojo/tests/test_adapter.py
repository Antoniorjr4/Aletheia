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
