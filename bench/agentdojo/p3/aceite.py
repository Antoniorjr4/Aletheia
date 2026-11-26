#!/usr/bin/env python3
"""P3: executor neutro dos casos de aceitação (formato em FUNCIONAL.md).

O mesmo caso roda nas duas implementações, sobre o ambiente oficial da suíte `banking` e o
`FunctionsRuntime` oficial. O "modelo" é roteirizado: propõe as leituras padrão e depois o plano do
caso. O resultado observado são as chamadas de escrita que chegaram ao banco e a resposta final.

    python3 aceite.py --impl A --dir <raiz de bench/agentdojo> --cases casos.json [--only C1]
    python3 aceite.py --impl P --dir <raiz de bench/agentdojo> --cases casos.json

Lado A: valores monetários do plano (`amount`, texto decimal) viram `cents` inteiros, que é como o
programa v3 os declara. Lado P: seguem como texto decimal.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import warnings
from decimal import Decimal
from pathlib import Path

warnings.filterwarnings("ignore")
READS = {"kind": "Reads", "transactions": True, "scheduled": True, "balance": False, "iban": False,
         "user": False, "file": ""}
READ_FUNCS = {"get_balance", "get_iban", "get_most_recent_transactions", "get_scheduled_transactions",
              "get_user_info", "read_file"}


def to_cents(op: dict) -> dict:
    out = {}
    for k, v in op.items():
        if k == "amount":
            c = Decimal(str(v)) * 100
            out["cents"] = int(c) if c == c.to_integral_value() else float(c)
        else:
            out[k] = v
    return out


def run_case(impl: str, root: Path, case: dict, program: Path | None):
    from agentdojo.functions_runtime import FunctionsRuntime
    from agentdojo.task_suite.load_suites import get_suite

    suite = get_suite("v1.2.2", "banking")
    env = suite.load_and_inject_default_environment({})
    runtime = FunctionsRuntime(suite.tools)
    if impl == "A":
        from aletheia_pipeline import AletheiaAgent
        from aletheia_runtime.providers import Scripted

        plan = {"kind": "Plan", "writes": [to_cents(o) for o in case["proposal"]], "answer": "done"}
        agent = AletheiaAgent("banking", str(program), lambda: Scripted({"planner": [READS] + [plan] * 6}),
                              "Banker", "task")
    else:
        from agentdojo_adapter import HardenedAgent

        plan = {"kind": "Plan", "writes": case["proposal"], "answer": "done"}
        seq = [READS, plan]

        def factory():
            n = [0]

            def llm(prompt, schema, max_tokens):
                r = seq[min(n[0], len(seq) - 1)]
                n[0] += 1
                return json.dumps({"value": r}), 100, 50

            return llm

        agent = HardenedAgent(factory, "keeping")
    _, _, _, messages, _ = agent.query(case["request"], runtime, env, [], {})
    calls = [c for c in agent.last["calls"] if c["function"] not in READ_FUNCS and not c.get("error")]
    answer = messages[-1]["content"][0]["content"] if messages else ""
    return calls, answer, agent.last.get("error")


def matches(calls: list, expect: list) -> str | None:
    if len(calls) != len(expect):
        return f"esperadas {len(expect)} escritas, houve {len(calls)}: {[(c['function'], c['args']) for c in calls]}"
    for c, e in zip(calls, expect):
        if c["function"] != e["function"]:
            return f"esperada {e['function']}, houve {c['function']}"
        for k, v in (e.get("args") or {}).items():
            got = c["args"].get(k)
            if isinstance(v, (int, float)) and not isinstance(v, bool) and isinstance(got, (int, float)):
                if abs(float(got) - float(v)) > 1e-9:
                    return f"{e['function']}.{k}: esperado {v}, houve {got}"
            elif got != v:
                return f"{e['function']}.{k}: esperado {v!r}, houve {got!r}"
    return None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--impl", choices=["A", "P"], required=True)
    ap.add_argument("--dir", required=True, help="raiz de bench/agentdojo da cópia de trabalho")
    ap.add_argument("--cases", required=True)
    ap.add_argument("--only", help="prefixo do id do caso (ex.: C1)")
    ap.add_argument("--program", default="banking_v3_keeping.ath")
    a = ap.parse_args()
    root = Path(a.dir).resolve()
    repo = root.parents[1]
    sys.path.insert(0, str(root))
    sys.path.insert(0, str(root / "hardened"))
    sys.path.insert(0, str(repo / "runtimes" / "python"))
    doc = json.loads(Path(a.cases).read_text())
    cases = doc["cases"] if "cases" in doc else [c for ch in doc["changes"] for c in ch["cases"]]
    if a.only:
        cases = [c for c in cases if c["id"].startswith(a.only)]
    program = None
    if a.impl == "A":
        athc = os.environ.get("ATHC", str(repo / "target" / "release" / "athc"))
        tmp = tempfile.mkdtemp(prefix="p3-")
        program = Path(tmp) / "agent.py"
        r = subprocess.run([athc, "emit", str(root / a.program), "--target", "py", "--out", str(program)],
                           capture_output=True, text=True)
        if r.returncode != 0:
            print("COMPILAÇÃO FALHOU\n" + r.stdout + r.stderr)
            return 2
    failed = 0
    for c in cases:
        try:
            calls, answer, err = run_case(a.impl, root, c, program)
            why = matches(calls, c["expect_calls"])
            want = c.get("expect_answer_contains")
            if why is None and want and want not in answer:
                why = f"resposta sem {want!r}: {answer[:200]!r}"
            if why is None and err and c["expect_calls"]:
                why = f"erro do agente: {err}"
        except Exception as e:  # noqa: BLE001 - qualquer exceção é falha do caso
            why = f"exceção {type(e).__name__}: {str(e)[:300]}"
        print(("ok   " if why is None else "FALHA"), c["id"], "" if why is None else "— " + why)
        failed += why is not None
    print(f"{len(cases) - failed}/{len(cases)} casos passam")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
