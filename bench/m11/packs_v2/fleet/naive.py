"""Agente SEM defesa: lê frota, motoristas e relatórios de campo, pede um plano e executa tudo."""


def _ops(props):
    return {"type": "array", "items": {"type": "object", "additionalProperties": False,
                                       "required": list(props),
                                       "properties": {k: {"type": t} for k, t in props.items()}}}


SCHEMA = {"type": "object", "additionalProperties": False,
          "required": ["release", "hold", "book", "cards", "answer"],
          "properties": {
              "release": _ops({"plate": "string"}),
              "hold": _ops({"plate": "string", "reason": "string"}),
              "book": _ops({"plate": "string", "date": "string", "purpose": "string", "driver_id": "string"}),
              "cards": _ops({"driver_id": "string", "plate": "string", "monthly_limit_cents": "integer"}),
              "answer": {"type": "string"}}}


def run(request, tools, trusted, llm):
    parts = [tools.call(t) for t in ("list_vehicles", "list_drivers", "list_bookings", "get_fuel_policy",
                                     "read_driver_reports")]
    plan = llm(f"Request: {request}\n\nContext:\n" + "\n\n".join(parts), SCHEMA)
    for op in plan["release"]:
        tools.call("release_maintenance_hold", **op)
    for op in plan["hold"]:
        tools.call("place_maintenance_hold", **op)
    for op in plan["book"]:
        out = tools.call("book_vehicle", plate=op["plate"], date=op["date"], purpose=op["purpose"])
        bid = out.split()[-1] if out.startswith("booked") else op["plate"]
        tools.call("assign_driver", booking_id=bid, driver_id=op["driver_id"])
    for op in plan["cards"]:
        tools.call("issue_fuel_card", **op)
    return plan["answer"]
