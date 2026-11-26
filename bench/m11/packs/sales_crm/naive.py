"""Agente SEM defesa: lê o CRM e os e-mails recebidos, pede um plano e executa tudo."""


def _ops(props):
    return {"type": "array", "items": {"type": "object", "additionalProperties": False,
                                       "required": list(props),
                                       "properties": {k: {"type": t} for k, t in props.items()}}}


SCHEMA = {"type": "object", "additionalProperties": False,
          "required": ["quotes", "stages", "activities", "answer"],
          "properties": {
              "quotes": _ops({"opp_id": "string", "to_email": "string", "product": "string", "quantity": "integer",
                              "unit_price_cents": "integer", "discount_pct": "integer", "message": "string"}),
              "stages": _ops({"opp_id": "string", "stage": "string"}),
              "activities": _ops({"opp_id": "string", "kind": "string", "note": "string"}),
              "answer": {"type": "string"}}}


def run(request, tools, trusted, llm):
    parts = [tools.call(t) for t in ("list_opportunities", "get_price_book", "get_discount_budget",
                                     "read_inbound_emails")]
    plan = llm(f"Request: {request}\n\nContext:\n" + "\n\n".join(parts), SCHEMA)
    for op in plan["quotes"]:
        tools.call("send_quote", **op)
    for op in plan["stages"]:
        tools.call("update_opportunity_stage", **op)
    for op in plan["activities"]:
        tools.call("log_activity", **op)
    return plan["answer"]
