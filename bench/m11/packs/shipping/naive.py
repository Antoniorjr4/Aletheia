"""Agente SEM defesa: lê pedidos, tarifas e mensagens de clientes, pede um plano e executa tudo."""


def _ops(props):
    return {"type": "array", "items": {"type": "object", "additionalProperties": False,
                                       "required": list(props),
                                       "properties": {k: {"type": t} for k, t in props.items()}}}


SCHEMA = {"type": "object", "additionalProperties": False,
          "required": ["ship", "change_address", "notify", "answer"],
          "properties": {
              "ship": _ops({"order_id": "string", "ship_to": "string", "carrier": "string", "service": "string",
                            "price_cents": "integer"}),
              "change_address": _ops({"shipment_id": "string", "ship_to": "string"}),
              "notify": _ops({"customer_id": "string", "email": "string", "message": "string"}),
              "answer": {"type": "string"}}}


def run(request, tools, trusted, llm):
    parts = [tools.call(t) for t in ("list_orders", "list_shipments", "get_carrier_rates", "get_freight_budget",
                                     "read_customer_messages")]
    plan = llm(f"Request: {request}\n\nContext:\n" + "\n\n".join(parts), SCHEMA)
    for op in plan["ship"]:
        out = tools.call("create_shipment", order_id=op["order_id"], ship_to=op["ship_to"])
        sid = out.split()[-1] if out.startswith("created") else op["order_id"]
        tools.call("book_carrier", shipment_id=sid, carrier=op["carrier"], service=op["service"],
                   price_cents=op["price_cents"])
    for op in plan["change_address"]:
        tools.call("change_delivery_address", **op)
    for op in plan["notify"]:
        tools.call("notify_customer", **op)
    return plan["answer"]
