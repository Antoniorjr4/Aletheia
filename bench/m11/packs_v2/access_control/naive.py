"""Agente SEM defesa: lê os tickets e as conversas, pede um plano e executa tudo."""


def _ops(props):
    return {"type": "array", "items": {"type": "object", "additionalProperties": False,
                                       "required": list(props),
                                       "properties": {k: {"type": t} for k, t in props.items()}}}


SCHEMA = {"type": "object", "additionalProperties": False,
          "required": ["revoke", "grant", "disable", "notify", "answer"],
          "properties": {
              "revoke": _ops({"user_id": "string", "group": "string"}),
              "grant": _ops({"user_id": "string", "group": "string", "expires_on": "string"}),
              "disable": _ops({"user_id": "string"}),
              "notify": _ops({"user_id": "string", "message": "string"}),
              "answer": {"type": "string"}}}


def run(request, tools, trusted, llm):
    parts = [tools.call("list_tickets"), tools.call("list_groups")]
    for tid in trusted["requested_tickets"]:
        parts.append(f"Thread of {tid}:\n" + tools.call("read_ticket_thread", ticket_id=tid))
    plan = llm(f"Request: {request}\n\nContext:\n" + "\n\n".join(parts), SCHEMA)
    for op in plan["revoke"]:
        tools.call("revoke_access", **op)
    for op in plan["grant"]:
        tools.call("grant_access", **op)
    for op in plan["disable"]:
        tools.call("disable_account", **op)
    for op in plan["notify"]:
        tools.call("notify_user", **op)
    return plan["answer"]
