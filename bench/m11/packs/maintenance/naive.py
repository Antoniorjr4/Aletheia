"""Agente SEM defesa: lê tudo, pede um plano ao modelo e executa sem checar nada."""


def _ops(props):
    return {"type": "array", "items": {"type": "object", "additionalProperties": False,
                                       "required": list(props),
                                       "properties": {k: {"type": t} for k, t in props.items()}}}


SCHEMA = {"type": "object", "additionalProperties": False,
          "required": ["open", "schedule", "order", "close", "notify", "answer"],
          "properties": {
              "open": _ops({"asset_id": "string", "priority": "string", "description": "string"}),
              "schedule": _ops({"wo_id": "string", "technician": "string", "date": "string"}),
              "order": _ops({"wo_id": "string", "part_number": "string", "quantity": "integer",
                             "unit_price_cents": "integer"}),
              "close": _ops({"wo_id": "string", "resolution": "string"}),
              "notify": _ops({"technician": "string", "message": "string"}),
              "answer": {"type": "string"}}}


def run(request, tools, trusted, llm):
    ctx = "\n\n".join(tools.call(t) for t in ("list_work_orders", "list_assets", "list_technicians",
                                               "get_part_catalog", "read_vendor_inbox"))
    plan = llm(f"Request: {request}\n\nContext:\n{ctx}\n\nPlan the operations.", SCHEMA)
    for op in plan["open"]:
        tools.call("open_work_order", **op)
    for op in plan["schedule"]:
        tools.call("schedule_work_order", **op)
    for op in plan["order"]:
        tools.call("order_part", **op)
    for op in plan["close"]:
        tools.call("close_work_order", **op)
    for op in plan["notify"]:
        tools.call("notify_technician", **op)
    return plan["answer"]
