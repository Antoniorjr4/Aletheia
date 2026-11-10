"""Rev7 pre-adapter for the *official* AgentDojo workspace/banking task suites.

This is a fail-closed boundary scaffold, not an execution of the official benchmark.
No unknown tool is authorized; no API key is read; no side effects are performed.
"""
from __future__ import annotations

import dataclasses
from typing import Mapping


BENCHMARK_VERSION = "v1.2.2"
SUITES = frozenset({"workspace", "banking"})


@dataclasses.dataclass(frozen=True)
class BoundTool:
    name: str
    effect: str
    mutates: bool
    # Review and human-approved policy binding must precede any call.
    policy_id: str


@dataclasses.dataclass(frozen=True)
class Decision:
    allowed: bool
    reason: str


def authorize_tool(
    suite: str,
    name: str,
    declared_capabilities: frozenset[str],
    bindings: Mapping[str, BoundTool],
    approved_policies: frozenset[str],
) -> Decision:
    """Pure static decision; runtime enforcement/replay adapter is upcoming."""
    if suite not in SUITES:
        return Decision(False, "suite unsupported")
    tool = bindings.get(name)
    if tool is None or tool.name != name:
        return Decision(False, "unknown/unmapped AgentDojo tool")
    if not tool.policy_id or tool.policy_id not in approved_policies:
        return Decision(False, "tool lacks approved policy")
    if tool.effect not in declared_capabilities:
        return Decision(False, "missing Cap[E]")
    return Decision(True, "capability and explicit policy present")


def manifest(suite_name: str, suite: object) -> dict:
    """Read only the official AgentDojo TaskSuite metadata; no task is executed."""
    if suite_name not in SUITES:
        raise ValueError(f"unsupported suite: {suite_name}")
    user_tasks = getattr(suite, "user_tasks")
    injection_tasks = getattr(suite, "injection_tasks")
    tools = getattr(suite, "tools")
    if not isinstance(user_tasks, Mapping) or not isinstance(injection_tasks, Mapping):
        raise TypeError("unexpected AgentDojo TaskSuite version")
    return {
        "benchmark": "AgentDojo official",
        "benchmark_version": BENCHMARK_VERSION,
        "suite": suite_name,
        "user_tasks": sorted(str(k) for k in user_tasks),
        "injection_tasks": sorted(str(k) for k in injection_tasks),
        "tools": sorted(str(k) for k in tools),
        "adapter_status": "metadata_only_no_agent_execution",
        "results": None,
    }
