#!/usr/bin/env python3
"""Executa uma suíte oficial do AgentDojo contra modelo real, com e sem Aletheia.

    python bench/agentdojo/run.py --suite banking --model openai/gpt-oss-120b \\
        --pipelines baseline,aletheia --logdir bench/agentdojo/runs/banking-gpt-oss-120b \\
        --athc target/release/athc

Dois pipelines, o mesmo modelo, a mesma suíte, o mesmo ataque:

* `baseline`: o pipeline padrão do AgentDojo, sem defesa: mensagem de sistema oficial,
  laço de tool calling com o modelo e as ferramentas da suíte;
* `aletheia`: o agente `bench/agentdojo/<suite>.ath`, verificado pelo `athc`, emitido para
  Python e executado pelo `aletheia_runtime` (`AletheiaAgent`).

A execução usa as funções oficiais `run_task_without_injection_tasks` e
`run_task_with_injection_tasks`, que avaliam utilidade e segurança com os verificadores da
própria suíte e gravam os logs no formato do AgentDojo. A ordem é pareada: cada tarefa roda
nos dois pipelines antes da próxima, então uma rodada interrompida compara os mesmos pares. A
retomada é a do AgentDojo: o que já tem log não roda de novo. Cota esgotada encerra a rodada
sem gravar a tarefa como falha.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
import warnings
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))

warnings.filterwarnings("ignore")

BENCHMARK_VERSION = "v1.2.2"


def short_name(model: str) -> str:
    return model.split("/")[-1]


def emit_program(athc: str, suite: str, model: str, out_dir: Path) -> Path:
    """Fixa o modelo no contrato Δ, verifica e emite o agente da suíte."""
    src = (HERE / f"{suite}.ath").read_text(encoding="utf-8")
    src = re.sub(r'(\n\s*id\s*=\s*)"[^"]*"', lambda m: f'{m.group(1)}"{model}"', src, count=1)
    out_dir.mkdir(parents=True, exist_ok=True)
    ath = out_dir / f"{suite}.ath"
    ath.write_text(src, encoding="utf-8")
    py = out_dir / f"{suite}_agent.py"
    subprocess.run([athc, "check", str(ath)], check=True)
    subprocess.run([athc, "emit", str(ath), "--target", "py", "--out", str(py)], check=True)
    return py


class Usage:
    """Soma o `usage` de cada resposta do provedor, por tarefa."""

    def __init__(self):
        self.tok_in = self.tok_out = self.calls = 0
        self.errors: list = []

    def reset(self) -> dict:
        r = {"tok_in": self.tok_in, "tok_out": self.tok_out, "calls": self.calls, "api_errors": self.errors}
        self.tok_in = self.tok_out = self.calls = 0
        self.errors = []
        return r


def groq_llm(model: str, usage: Usage):
    """O `OpenAILLM` oficial, com a única adaptação de transporte que a API compatível pede:
    o papel `developer` vira `system` e o conteúdo em blocos vira texto. A temperatura 0 é
    enviada de fato (o `OpenAILLM` omite `temperature=0.0`)."""
    import openai
    from agentdojo.agent_pipeline.llms.openai_llm import (
        OpenAILLM,
        _function_to_openai,
        _message_to_openai,
        _openai_to_assistant_message,
    )

    client = openai.OpenAI(
        api_key=os.environ["OPENAI_API_KEY"],
        base_url=os.environ.get("OPENAI_BASE_URL", "https://api.groq.com/openai/v1"),
        max_retries=10,
        timeout=180,
    )

    def flat(m: dict) -> dict:
        m = dict(m)
        if m.get("role") == "developer":
            m["role"] = "system"
        c = m.get("content")
        if isinstance(c, list):
            m["content"] = "\n".join(b.get("text", "") for b in c if isinstance(b, dict))
        return m

    from agentdojo.types import ChatAssistantMessage, text_content_block_from_string

    class GroqLLM(OpenAILLM):
        def query(self, query, runtime, env, messages=(), extra_args={}):
            msgs = [flat(_message_to_openai(m, self.model)) for m in messages]
            tools = [_function_to_openai(t) for t in runtime.functions.values()]
            try:
                completion = self.client.chat.completions.create(
                    model=self.model,
                    messages=msgs,
                    tools=tools or openai.NOT_GIVEN,
                    tool_choice="auto" if tools else openai.NOT_GIVEN,
                    temperature=0,
                    reasoning_effort=self.reasoning_effort or openai.NOT_GIVEN,
                )
            except openai.BadRequestError as e:
                # o modelo gerou uma chamada de ferramenta que o servidor recusou (ex.:
                # `tool_use_failed`): a execução do baseline termina aqui, registrada
                usage.calls += 1
                usage.errors.append(str(getattr(e, "code", None) or e)[:200])
                out = ChatAssistantMessage(
                    role="assistant", content=[text_content_block_from_string(f"API error: {e}")], tool_calls=None
                )
                return query, runtime, env, [*messages, out], extra_args
            if completion.usage is not None:
                usage.tok_in += completion.usage.prompt_tokens or 0
                usage.tok_out += completion.usage.completion_tokens or 0
            usage.calls += 1
            out = _openai_to_assistant_message(completion.choices[0].message)
            return query, runtime, env, [*messages, out], extra_args

    return GroqLLM(client, model, reasoning_effort="low")


def baseline_pipeline(model: str, usage: Usage):
    from agentdojo.agent_pipeline import AgentPipeline, InitQuery, SystemMessage, ToolsExecutionLoop, ToolsExecutor
    from agentdojo.agent_pipeline.agent_pipeline import load_system_message
    from agentdojo.agent_pipeline.tool_execution import tool_result_to_str

    llm = groq_llm(model, usage)
    p = AgentPipeline(
        [
            SystemMessage(load_system_message(None)),
            InitQuery(),
            llm,
            ToolsExecutionLoop([ToolsExecutor(tool_result_to_str), llm]),
        ]
    )
    p.name = f"baseline-{short_name(model)}"
    return p


def aletheia_pipeline(suite: str, model: str, program: Path, journal_dir: Path, usage: Usage):
    from agentdojo.agent_pipeline import AgentPipeline, InitQuery, SystemMessage

    from aletheia_pipeline import AletheiaAgent
    from aletheia_runtime.providers import OpenAiCompat

    def provider():
        p = OpenAiCompat.from_env(model=model)
        inner = p.transport

        class Counting:
            def post(self, url, key, body):
                r = inner.post(url, key, body)
                usage.calls += 1
                try:
                    u = json.loads(r.body).get("usage") or {}
                    usage.tok_in += u.get("prompt_tokens") or 0
                    usage.tok_out += u.get("completion_tokens") or 0
                except (ValueError, AttributeError):
                    pass
                return r

            def live(self):
                return inner.live()

        p.transport = Counting()
        return p

    agent = AletheiaAgent(suite, str(program), provider, "Banker", "task", journal_dir=journal_dir)
    p = AgentPipeline(
        [
            SystemMessage(f"Aletheia agent `agentdojo.{suite}` (bench/agentdojo/{suite}.ath), checked by athc."),
            InitQuery(),
            agent,
        ]
    )
    p.name = f"aletheia-{short_name(model)}"
    return p, agent


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--suite", default="banking", choices=["banking"])
    ap.add_argument("--model", default="openai/gpt-oss-120b")
    ap.add_argument("--pipelines", default="baseline,aletheia")
    ap.add_argument("--attack", default="important_instructions")
    ap.add_argument("--logdir", required=True)
    ap.add_argument("--athc", default=str(ROOT / "target/release/athc"))
    ap.add_argument("--max-minutes", type=float, default=330.0)
    ap.add_argument("--user-tasks", default=None, help="subconjunto (vírgulas), para ensaios")
    ap.add_argument("--injection-tasks", default=None, help="subconjunto (vírgulas), para ensaios")
    ap.add_argument("--no-attacks", action="store_true")
    args = ap.parse_args()

    from agentdojo import models as ad_models
    from agentdojo.attacks.attack_registry import load_attack
    from agentdojo.benchmark import run_task_with_injection_tasks, run_task_without_injection_tasks
    from agentdojo.task_suite.load_suites import get_suite

    from aletheia_pipeline import QuotaExhausted

    try:
        import openai

        rate_errors: tuple = (openai.RateLimitError,)
    except ImportError:
        rate_errors = ()

    logdir = Path(args.logdir)
    logdir.mkdir(parents=True, exist_ok=True)
    suite = get_suite(BENCHMARK_VERSION, args.suite)
    short = short_name(args.model)
    # O ataque `important_instructions` se dirige ao modelo pelo nome; para modelos fora da
    # tabela do AgentDojo, o nome genérico que a própria tabela usa para modelos abertos.
    ad_models.MODEL_NAMES.setdefault(short, "AI assistant")

    usage = Usage()
    pipelines = {}
    agent = None
    for name in args.pipelines.split(","):
        if name == "baseline":
            pipelines[name] = baseline_pipeline(args.model, usage)
        elif name == "aletheia":
            program = emit_program(args.athc, args.suite, args.model, logdir / "program")
            pipelines[name], agent = aletheia_pipeline(
                args.suite, args.model, program, logdir / "aletheia-journals" / args.suite, usage
            )
        else:
            raise SystemExit(f"pipeline desconhecido: {name}")
    attacks = {n: load_attack(args.attack, suite, p) for n, p in pipelines.items()}

    def ids(tasks, subset):
        keys = sorted(tasks, key=lambda k: int(k.rsplit("_", 1)[-1]))
        return [k for k in keys if subset is None or k in subset.split(",")]

    user_tasks = ids(suite.user_tasks, args.user_tasks)
    injection_tasks = ids(suite.injection_tasks, args.injection_tasks)
    usage_log = logdir / "usage.jsonl"
    deadline = time.monotonic() + args.max_minutes * 60

    def record(pname: str, ut: str, inj: str, t0: float) -> bool:
        """Registra o consumo da execução; uma tarefa retomada do log não chama o modelo."""
        u = usage.reset()
        if u["calls"]:
            with usage_log.open("a", encoding="utf-8") as f:
                f.write(json.dumps({"pipeline": pipelines[pname].name, "user_task": ut, "injection_task": inj,
                                    "seconds": round(time.monotonic() - t0, 2), **u}) + "\n")
        return u["calls"] > 0

    jobs = [(ut, None) for ut in user_tasks]
    if not args.no_attacks:
        jobs += [(ut, inj) for ut in user_tasks for inj in injection_tasks]
    from agentdojo.logging import OutputLogger

    state = {"done": 0, "reason": "completa"}

    def run_all():
        for ut_id, inj_id in jobs:
            for pname, p in pipelines.items():
                if time.monotonic() > deadline:
                    state["reason"] = "tempo"
                    return
                if pname == "aletheia" and agent is not None:
                    agent.current = (ut_id, inj_id or "none")
                usage.reset()
                t0 = time.monotonic()
                ut = suite.get_user_task_by_id(ut_id)
                if inj_id is None:
                    run_task_without_injection_tasks(suite, p, ut, logdir, False, BENCHMARK_VERSION)
                else:
                    run_task_with_injection_tasks(
                        suite, p, ut, attacks[pname], logdir, False, [inj_id], BENCHMARK_VERSION
                    )
                state["done"] += record(pname, ut_id, inj_id or "none", t0)

    try:
        with OutputLogger(str(logdir), live=None):
            run_all()
    except (QuotaExhausted, *rate_errors) as e:
        state["reason"] = f"cota: {str(e)[:300]}"
    done, reason = state["done"], state["reason"]
    print(f"rodada encerrada ({reason}); {done} execução(ões) nesta rodada")
    from matrix import write_matrix

    # a matriz cobre a suíte inteira: a cobertura diz quanto dela já tem log completo
    m = write_matrix(logdir, suite, args.attack, [p.name for p in pipelines.values()],
                     ids(suite.user_tasks, None), ids(suite.injection_tasks, None))
    (logdir / "last_round.json").write_text(json.dumps({"reason": reason, "executed": done}, indent=1) + "\n")
    print(json.dumps(m["summary"], indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
