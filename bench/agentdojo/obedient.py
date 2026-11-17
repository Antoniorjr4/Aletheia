#!/usr/bin/env python3
"""O modelo obediente: quem decide quando o modelo faz tudo o que o atacante manda?

A rodada com modelo real (`run.py`) mede o sistema inteiro. Nela, o planejador do Aletheia
nunca propôs a operação injetada, então a política nunca precisou recusar nada, e a medição
não separa o prompt da linguagem (RESULTS.md, "Quem segurou os ataques").

Este script fecha essa lacuna sem modelo e sem custo. O planejador é substituído por um
roteiro **adversarial derivado da própria suíte oficial**: para cada par (tarefa do usuário,
tarefa de injeção), o "modelo" propõe exatamente as chamadas de verdade-base da injeção
(`injection_task.ground_truth`), que são, por definição do AgentDojo, o que cumpre o objetivo
do atacante. Nada é escolhido à mão, e o IBAN do atacante não aparece no código.

Três braços, todos com o verificador de segurança e de utilidade oficiais:

* `obediente`: o modelo faz o trabalho do usuário **e** obedece à injeção (verdade-base do
  usuário + verdade-base do atacante, no mesmo plano);
* `sequestrado`: o modelo abandona o usuário e só executa a injeção;
* `legítimo`: sem ataque, o modelo propõe só a verdade-base do usuário. Mede o custo da
  política: quantas tarefas legítimas ela recusa.

Cada braço roda em dois pipelines com as mesmas propostas:

* `sem-linguagem`: as chamadas propostas são executadas direto no `FunctionsRuntime` oficial,
  como faz um laço de tool calling sem defesa;
* `aletheia`: as mesmas propostas entram como saída do `infer planner -> Plan` do agente
  `banking.ath` e só viram efeito se o programa as executar.

Uso:
    python3 bench/agentdojo/obedient.py --out bench/agentdojo/runs/obedient-banking
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import warnings
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(ROOT / "runtimes" / "python"))
warnings.filterwarnings("ignore")

from agentdojo.agent_pipeline import AgentPipeline, InitQuery, SystemMessage  # noqa: E402
from agentdojo.agent_pipeline.base_pipeline_element import BasePipelineElement  # noqa: E402
from agentdojo.functions_runtime import FunctionCall  # noqa: E402
from agentdojo.task_suite.load_suites import get_suite  # noqa: E402
from agentdojo.types import ChatAssistantMessage, text_content_block_from_string  # noqa: E402

from matrix import wilson  # noqa: E402

ATHC = os.environ.get("ATHC", str(ROOT / "target" / "release" / "athc"))
VERSION = "v1.2.2"
READ_PREFIXES = ("get_", "read_")


def money(x) -> str:
    return f"{float(x):.2f}"


def to_write(call: FunctionCall, env) -> dict | None:
    """Uma chamada de verdade-base da suíte como operação `Write` do agente, ou None se o
    vocabulário tipado do agente não tem como exprimi-la."""
    a = dict(call.args)
    f = call.function
    if f == "send_money":
        return {"kind": "Send", "recipient": str(a["recipient"]), "amount": money(a["amount"]),
                "subject": str(a["subject"])[:120], "date": str(a["date"])}
    if f == "schedule_transaction":
        return {"kind": "Schedule", "recipient": str(a["recipient"]), "amount": money(a["amount"]),
                "subject": str(a["subject"])[:120], "date": str(a["date"]), "recurring": bool(a["recurring"])}
    if f == "update_scheduled_transaction":
        amount = a.get("amount")
        if amount is None:
            cur = [t for t in env.bank_account.scheduled_transactions if t.id == a["id"]]
            amount = cur[0].amount if cur else 0
        if a.get("recipient"):
            return {"kind": "Retarget", "id": int(a["id"]), "recipient": str(a["recipient"]), "amount": money(amount)}
        return {"kind": "Update", "id": int(a["id"]), "amount": money(amount)}
    if f == "update_password":
        return {"kind": "Password", "password": str(a["password"])}
    if f == "update_user_info":
        return {"kind": "Profile", "street": str(a.get("street") or ""), "city": str(a.get("city") or "")}
    return None


def writes_of(calls, env):
    """As operações com efeito de uma verdade-base, e as que o agente não exprime."""
    writes, outside = [], []
    for c in calls:
        if c.function.startswith(READ_PREFIXES):
            continue
        w = to_write(c, env)
        (writes if w is not None else outside).append(w if w is not None else c.function)
    return writes, outside


def pruned_ops(agent, user: str, inj) -> int:
    """Quantas operações `keeping` deixou de fora, lidas do journal encadeado da execução."""
    f = agent.journal_dir / user / f"{inj.ID if inj else 'none'}.jsonl"
    n = 0
    for line in f.read_text().splitlines():
        e = json.loads(line)
        if e["kind"] == "prune":
            n += len(e["detail"]["dropped"])
    return n


ALL_READS = {"kind": "Reads", "transactions": True, "scheduled": True, "balance": True, "iban": True,
             "user": True, "file": ""}


class Direct(BasePipelineElement):
    """Sem linguagem: as chamadas propostas vão direto ao runtime oficial."""

    name = "sem-linguagem"

    def __init__(self):
        self.calls: list = []
        self.answer = ""

    def query(self, query, runtime, env=None, messages=(), extra_args={}):
        for c in self.calls:
            runtime.run_function(env, c.function, dict(c.args))
        messages = [*messages, ChatAssistantMessage(role="assistant",
                                                    content=[text_content_block_from_string(self.answer)],
                                                    tool_calls=None)]
        return query, runtime, env, messages, extra_args


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(HERE / "runs" / "obedient-banking"))
    ap.add_argument("--program", default=str(HERE / "banking.ath"))
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    from aletheia_pipeline import AletheiaAgent
    from aletheia_runtime.providers import Scripted

    suite = get_suite(VERSION, "banking")
    with tempfile.TemporaryDirectory() as tmp:
        emitted = Path(tmp) / "banking_agent.py"
        subprocess.run([ATHC, "emit", args.program, "--target", "py", "--out", str(emitted)],
                       check=True, capture_output=True)
        root = subprocess.run([ATHC, "check", args.program], capture_output=True, text=True).stdout

        script: dict = {}
        agent = AletheiaAgent("banking", str(emitted), lambda: Scripted({"planner": script["seq"]}), "Banker", "task",
                              journal_dir=Path(tmp) / "journals")
        p_ath = AgentPipeline([SystemMessage("Aletheia"), InitQuery(), agent])
        p_ath.name = "aletheia-obedient"
        direct = Direct()
        p_dir = AgentPipeline([SystemMessage("none"), InitQuery(), direct])
        p_dir.name = "direct-obedient"

        rows = []

        def run(arm, ut, inj, proposal_calls, answer):
            env0 = suite.load_and_inject_default_environment(
                {} if inj is None else {k: inj.GOAL for k in suite.get_injection_vector_defaults()})
            writes, outside = writes_of(proposal_calls, env0)
            # A proposta vai inteira: se exceder `List[Write, 0..4]`, quem recusa é o tipo
            # (decode_reject), e isso é contado à parte da recusa pela política.
            plan = {"kind": "Plan", "writes": writes, "answer": answer[:600]}
            injections = {} if inj is None else {k: inj.GOAL for k in suite.get_injection_vector_defaults()}
            res = {"arm": arm, "user_task": ut.ID, "injection_task": inj.ID if inj else "none",
                   "proposed": [w["kind"] for w in writes], "outside_vocabulary": outside}
            # sem linguagem
            direct.calls = [c for c in proposal_calls]
            direct.answer = answer
            u, s = suite.run_task_with_pipeline(p_dir, ut, inj, injections)
            res["direct"] = {"utility": bool(u), "attack": bool(s) if inj else None}
            # Aletheia: a mesma proposta como saída tipada do planejador, em toda tentativa
            script["seq"] = [ALL_READS] + [plan] * 6
            agent.current = (ut.ID, inj.ID if inj else "none")
            u, s = suite.run_task_with_pipeline(p_ath, ut, inj, injections)
            kinds = agent.last["kinds"]
            res["aletheia"] = {"utility": bool(u), "attack": bool(s) if inj else None,
                               "endorsed": "endorse" in kinds,
                               # recusa pela política: o plano inteiro (`policy_reject`) ou, com
                               # `keeping`, operação por operação (`prune`)
                               "policy_rejections": kinds.count("policy_reject") + kinds.count("prune"),
                               "pruned_ops": pruned_ops(agent, ut.ID, inj),
                               "type_rejections": kinds.count("decode_reject"),
                               "policy_saw_attack": inj is not None and arm != "legitimo"
                                                    and (kinds.count("policy_reject") + kinds.count("prune")) > 0,
                               "executed": [c["function"] for c in agent.last["calls"]
                                            if not c["function"].startswith(READ_PREFIXES)],
                               "error": agent.last["error"]}
            rows.append(res)

        for ut in suite.user_tasks.values():
            env = suite.load_and_inject_default_environment({})
            user_calls = list(ut.ground_truth(env))
            answer = str(getattr(ut, "GROUND_TRUTH_OUTPUT", "") or "done")
            run("legitimo", ut, None, user_calls, answer)
            for inj in suite.injection_tasks.values():
                env = suite.load_and_inject_default_environment({})
                atk = list(inj.ground_truth(env))
                run("obediente", ut, inj, user_calls + atk, answer)
                run("sequestrado", ut, inj, atk, "done")

    (out / "rows.jsonl").write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows))
    summary = summarize(rows)
    summary["program_check"] = root.strip().splitlines()[-1] if root.strip() else ""
    (out / "matrix.json").write_text(json.dumps(summary, ensure_ascii=False, indent=1))
    (out / "matrix.md").write_text(render(summary, rows))
    print(render(summary, rows))
    return 0


def rate(k, n):
    lo, hi = wilson(k, n)
    return {"k": k, "n": n, "lo": lo, "hi": hi}


def summarize(rows):
    s = {"suite": f"banking {VERSION}", "arms": {}}
    for arm in ("legitimo", "obediente", "sequestrado"):
        rs = [r for r in rows if r["arm"] == arm]
        a = {}
        for pipe in ("direct", "aletheia"):
            a[pipe] = {"utility": rate(sum(r[pipe]["utility"] for r in rs), len(rs))}
            if arm != "legitimo":
                a[pipe]["attack"] = rate(sum(r[pipe]["attack"] for r in rs), len(rs))
        a["aletheia"]["policy_rejections"] = sum(r["aletheia"]["policy_rejections"] > 0 for r in rs)
        a["aletheia"]["errors"] = sum(r["aletheia"]["error"] is not None for r in rs)
        a["aletheia"]["type_only"] = sum(r["aletheia"]["type_rejections"] > 0 and r["aletheia"]["policy_rejections"] == 0
                                         for r in rs)
        a["over_4_writes"] = sum(len(r["proposed"]) > 4 for r in rs)
        a["aletheia"]["pruned_ops"] = sum(r["aletheia"]["pruned_ops"] for r in rs)
        a["aletheia"]["executed_ops"] = sum(len(r["aletheia"]["executed"]) for r in rs)
        s["arms"][arm] = a
    # cobertura efetiva: pares em que a política avaliou uma proposta com a operação do atacante
    pairs = {(r["user_task"], r["injection_task"]) for r in rows if r["arm"] != "legitimo"}
    seen = {(r["user_task"], r["injection_task"]) for r in rows if r["aletheia"].get("policy_saw_attack")}
    s["effective_coverage"] = {"pairs": len(pairs), "policy_saw_attack": len(seen),
                               "never_reached_policy": sorted(pairs - seen)}
    s["breaches_aletheia"] = sorted({(r["user_task"], r["injection_task"]) for r in rows
                                     if r["arm"] != "legitimo" and r["aletheia"]["attack"]})
    s["refused_legit"] = sorted(r["user_task"] for r in rows
                                if r["arm"] == "legitimo" and not r["aletheia"]["utility"])
    s["legit_direct_fail"] = sorted(r["user_task"] for r in rows
                                    if r["arm"] == "legitimo" and not r["direct"]["utility"])
    return s


def pct(r):
    return f"{100 * r['k'] / r['n']:.1f}% ({r['k']}/{r['n']}; IC95 {100 * r['lo']:.0f}–{100 * r['hi']:.0f}%)"


def render(s, rows):
    a = s["arms"]
    lines = [f"# Modelo obediente · {s['suite']}", "",
             "| braço | pipeline | ataque cumprido (verificador oficial) | utilidade |",
             "| --- | --- | --- | --- |"]
    for arm in ("obediente", "sequestrado", "legitimo"):
        for pipe, label in (("direct", "sem linguagem"), ("aletheia", "Aletheia")):
            atk = pct(a[arm][pipe]["attack"]) if "attack" in a[arm][pipe] else "—"
            lines.append(f"| {arm} | {label} | {atk} | {pct(a[arm][pipe]['utility'])} |")
    c = s["effective_coverage"]
    lines += ["", "| braço | propostas acima de 4 operações | recusadas só pelo tipo | execuções com recusa da política | operações deixadas de fora (`keeping`) | operações executadas |",
              "| --- | --- | --- | --- | --- | --- |"]
    for arm in ("obediente", "sequestrado", "legitimo"):
        x = a[arm]
        lines.append(f"| {arm} | {x['over_4_writes']} | {x['aletheia']['type_only']} | {x['aletheia']['policy_rejections']} "
                     f"| {x['aletheia']['pruned_ops']} | {x['aletheia']['executed_ops']} |")
    lines += ["", f"Cobertura efetiva: a política avaliou a operação do atacante em {c['policy_saw_attack']} de "
              f"{c['pairs']} pares (em pelo menos um braço).",
              *[f"- sem passar pela política: `{u}` × `{i}`" for u, i in c["never_reached_policy"]]]
    lines += ["", f"Pares em que o ataque foi cumprido no Aletheia: {len(s['breaches_aletheia'])}"]
    for u, i in s["breaches_aletheia"]:
        lines.append(f"- `{u}` × `{i}`")
    lines += ["", f"Tarefas legítimas que o Aletheia não cumpriu: {', '.join(s['refused_legit']) or 'nenhuma'}",
              f"Tarefas legítimas que a execução direta não cumpriu: {', '.join(s['legit_direct_fail']) or 'nenhuma'}",
              "", f"Programa: `{s.get('program_check', '')}`", ""]
    return "\n".join(lines)


if __name__ == "__main__":
    sys.exit(main())
