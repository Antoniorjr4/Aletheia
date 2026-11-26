"""Agente SEM defesa: lê tudo (inclusive comentários), pede um plano ao modelo e executa sem checar."""


def _ops(props):
    return {"type": "array", "items": {"type": "object", "additionalProperties": False,
                                       "required": list(props),
                                       "properties": {k: {"type": t} for k, t in props.items()}}}


SCHEMA = {"type": "object", "additionalProperties": False,
          "required": ["transfer", "issue", "reorder", "answer"],
          "properties": {
              "transfer": _ops({"part_number": "string", "from_location": "string", "to_location": "string",
                                "quantity": "integer"}),
              "issue": _ops({"req_id": "string", "part_number": "string", "location": "string",
                             "quantity": "integer"}),
              "reorder": _ops({"part_number": "string", "quantity": "integer", "supplier": "string"}),
              "answer": {"type": "string"}}}


def run(request, tools, trusted, llm):
    parts = [tools.call(t) for t in ("list_requisitions", "get_stock", "get_part_master", "read_supplier_messages")]
    for rid in trusted["requested_requisitions"]:
        parts.append(f"Comments on {rid}:\n" + tools.call("read_requisition_comments", req_id=rid))
    plan = llm(f"Request: {request}\n\nContext:\n" + "\n\n".join(parts), SCHEMA)
    for op in plan["transfer"]:
        tools.call("transfer_stock", **op)
    for op in plan["issue"]:
        tools.call("issue_parts", **op)
    for op in plan["reorder"]:
        tools.call("create_reorder", **op)
    return plan["answer"]
