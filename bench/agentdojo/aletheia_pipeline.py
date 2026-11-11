"""A ponte oficial AgentDojo × Aletheia.

`AletheiaAgent` é um `BasePipelineElement` do AgentDojo. Cada `query` executa o agente
Aletheia da suíte (`bench/agentdojo/<suite>.ath`, verificado pelo `athc` e emitido para
Python) sobre o `aletheia_runtime`. O host liga cada ferramenta do programa à função oficial
da suíte, sobre o ambiente que o benchmark montou e injetou. Nada da suíte é reescrito:
tarefas, ataques, ambiente, ferramentas e os verificadores de utilidade e de segurança são os
do pacote `agentdojo`.

O que o host acrescenta é o que um runtime de agentes tem e o modelo não tem: os inventários
confiáveis da fotografia (`s.inventory`, `s.member`), calculados só de fontes confiáveis, a
saber o pedido do usuário e os campos estruturados do razão do banco. Texto livre de
ferramentas (assuntos de transação, arquivos) nunca entra num inventário.

Cada chamada de ferramenta vira uma mensagem `assistant` com `tool_calls` e uma mensagem
`tool` com o resultado, no formato do AgentDojo, para que os traços e os logs oficiais
registrem o que o agente fez. O journal encadeado do Aletheia de cada tarefa fica ao lado.
"""

from __future__ import annotations

import json
import re
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "runtimes" / "python"))

from agentdojo.agent_pipeline.base_pipeline_element import BasePipelineElement  # noqa: E402
from agentdojo.agent_pipeline.tool_execution import tool_result_to_str  # noqa: E402
from agentdojo.functions_runtime import EmptyEnv, Env, FunctionCall, FunctionsRuntime  # noqa: E402
from agentdojo.types import (  # noqa: E402
    ChatAssistantMessage,
    ChatToolResultMessage,
    text_content_block_from_string,
)

from aletheia_runtime.__main__ import load_program  # noqa: E402
from aletheia_runtime.journal import core_view  # noqa: E402
from aletheia_runtime.runtime import RtErr, Runtime  # noqa: E402
from aletheia_runtime.values import Cap  # noqa: E402
from aletheia_runtime.world import HostFailed  # noqa: E402


class QuotaExhausted(RuntimeError):
    """A cota do provedor acabou: a tarefa não foi avaliada e precisa ser refeita depois."""


# Ferramenta do programa → (função oficial da suíte, nomes dos argumentos depois da capacidade).
BINDINGS = {
    "banking": {
        "get_balance": ("get_balance", []),
        "get_iban": ("get_iban", []),
        "get_most_recent_transactions": ("get_most_recent_transactions", ["n"]),
        "get_scheduled_transactions": ("get_scheduled_transactions", []),
        "get_user_info": ("get_user_info", []),
        "read_file": ("read_file", ["file_path"]),
        "send_money": ("send_money", ["recipient", "amount", "subject", "date"]),
        "schedule_transaction": ("schedule_transaction", ["recipient", "amount", "subject", "date", "recurring"]),
        "update_scheduled_amount": ("update_scheduled_transaction", ["id", "amount"]),
        "retarget_scheduled": ("update_scheduled_transaction", ["id", "recipient", "amount"]),
        "update_password": ("update_password", ["password"]),
        "update_user_info": ("update_user_info", ["street", "city"]),
    },
}

# Argumentos monetários: o tipo Aletheia é texto decimal; a função oficial recebe `float`.
MONEY = {"amount"}

IBAN = re.compile(r"\b[A-Z]{2}[0-9]{2}[A-Z0-9]{11,30}\b")
QUOTED = re.compile(r"'([^'\n]{1,64})'|\"([^\"\n]{1,64})\"")


def banking_inventories(request: str, env) -> dict:
    """Os inventários confiáveis da suíte `banking`, só de fontes confiáveis.

    * `payees`: IBANs que o usuário escreveu no pedido, mais os destinatários que a própria
      conta já pagou (transações enviadas e agendadas). São campos estruturados do razão, não
      texto livre: um remetente que deposita na conta não vira beneficiário.
    * `scheduled`: os ids das transações agendadas.
    * `quoted`: os literais que o usuário citou entre aspas no pedido.
    """
    acct = env.bank_account
    own = {"me", acct.iban}
    payees = set(IBAN.findall(request))
    for t in acct.transactions:
        if t.sender in own:
            payees.add(t.recipient)
    for t in acct.scheduled_transactions:
        payees.add(t.recipient)
    payees = {p for p in payees if IBAN.fullmatch(p) and p != acct.iban}
    return {
        "payees": sorted(payees),
        "scheduled": sorted(str(t.id) for t in acct.scheduled_transactions),
        "quoted": sorted({a or b for a, b in QUOTED.findall(request)}),
    }


INVENTORIES = {"banking": banking_inventories}


class AgentDojoHost:
    """O mundo do `aletheia_runtime` sobre o `FunctionsRuntime` oficial da suíte."""

    def __init__(self, suite: str, runtime: FunctionsRuntime, env: Env, inventories: dict, messages: list):
        self.bindings = BINDINGS[suite]
        self.runtime, self.env, self.inventories, self.messages = runtime, env, inventories, messages
        self.calls: list = []

    def lat(self, tool: str) -> float:
        return 0.0

    def call(self, tool: str, args: list):
        if tool not in self.bindings:
            raise HostFailed(f"ferramenta `{tool}` sem ligação oficial")
        name, params = self.bindings[tool]
        values = [a for a in args if not isinstance(a, Cap)]
        kwargs = dict(zip(params, values))
        error = None
        for k in MONEY & kwargs.keys():
            try:
                kwargs[k] = float(str(kwargs[k]).replace(",", ""))
            except ValueError:
                error = f"ValueError: `{k}` não é um número: {kwargs[k]!r}"
        call_id = f"ath-{len(self.calls)}"
        fc = FunctionCall(function=name, args=kwargs, id=call_id)
        t0 = time.monotonic()
        result = ""
        if error is None:
            result, error = self.runtime.run_function(self.env, name, kwargs)
        lat = time.monotonic() - t0
        text = tool_result_to_str(result) if error is None else f"Error: {error}"
        self.calls.append({"tool": tool, "function": name, "args": kwargs, "error": error})
        self.messages.append(ChatAssistantMessage(role="assistant", content=None, tool_calls=[fc]))
        self.messages.append(
            ChatToolResultMessage(
                role="tool",
                content=[text_content_block_from_string(text)],
                tool_call_id=call_id,
                tool_call=fc,
                error=error,
            )
        )
        return text, lat

    def status(self, tool: str, key: int):
        raise HostFailed("as ferramentas oficiais não têm consulta de status por chave")

    def trusted_snapshot(self) -> dict:
        return {"managed": [], "replicas": {}, "cpu_pct": {}, "inventories": self.inventories}

    def state(self) -> dict:
        return {"calls": list(self.calls)}


class AletheiaAgent(BasePipelineElement):
    """O agente Aletheia como elemento de pipeline do AgentDojo.

    `program` é o módulo emitido por `athc emit --target py`; `provider_factory` cria o
    provedor de modelo de cada tarefa (Groq real, ou roteirizado nos testes).
    """

    def __init__(self, suite: str, program: str, provider_factory, agent: str, event: str, journal_dir=None):
        self.suite = suite
        self.prog = load_program(program)
        self.provider_factory = provider_factory
        self.agent, self.event = agent, event
        self.journal_dir = Path(journal_dir) if journal_dir else None
        # (tarefa do usuário, tarefa de injeção): o executor o define antes de cada tarefa
        self.current: tuple[str, str] = ("unknown", "none")
        self.last: dict = {}
        # v1.0-RC1: as concessões da execução (`s.authorizes`): `(pedido, env) -> (execução, doc)`.
        # Vêm de uma confirmação estruturada do principal; sem o gancho, não há concessão alguma.
        self.grants_for = None

    def query(
        self,
        query: str,
        runtime: FunctionsRuntime,
        env: Env = EmptyEnv(),
        messages=(),
        extra_args: dict = {},
    ):
        messages = list(messages)
        inventories = INVENTORIES[self.suite](query, env)
        host = AgentDojoHost(self.suite, runtime, env, inventories, messages)
        rt = Runtime(self.prog, host, self.provider_factory())
        if self.grants_for is not None:
            rt.run_id, doc = self.grants_for(query, env)
            rt.load_grants(doc)
        error = None
        try:
            answer = rt.run(self.agent, self.event, [query])
        except RtErr as e:
            if e.code == "R0621" and ("HTTP 429" in e.msg or "de espera" in e.msg):
                raise QuotaExhausted(e.msg) from None
            error = {"code": e.code, "msg": e.msg}
            answer = f"Aletheia aborted the task: {e.code} {e.msg}"
        if not isinstance(answer, str):
            answer = json.dumps(answer, ensure_ascii=False)
        messages.append(
            ChatAssistantMessage(role="assistant", content=[text_content_block_from_string(answer)], tool_calls=None)
        )
        self.last = {
            "error": error,
            "journal_head": rt.journal.head,
            "semantic_head": core_view(rt.journal.entries, {n for n, t in rt.p.tools.items()
                                                             if t["class"] == "compensable"}).head,
            "spent": {"tok": rt.meter.tok, "usd": rt.meter.usd, "wall": rt.meter.wall},
            "calls": host.calls,
            "inventories": inventories,
            "kinds": [e["kind"] for e in rt.journal.entries],
            "grants": {"live": len(rt.grants), "spent": len(rt.spent)},
        }
        if self.journal_dir is not None:
            user, inj = self.current
            d = self.journal_dir / user
            d.mkdir(parents=True, exist_ok=True)
            (d / f"{inj}.jsonl").write_text(rt.journal.to_jsonl(), encoding="utf-8")
            (d / f"{inj}.summary.json").write_text(
                json.dumps(self.last, ensure_ascii=False, indent=1, sort_keys=True), encoding="utf-8"
            )
        return query, runtime, env, messages, extra_args
