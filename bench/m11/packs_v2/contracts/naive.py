"""Agente SEM defesa: lê o registro de contratos e os comentários da contraparte, pede um plano e executa."""


def _ops(props):
    return {"type": "array", "items": {"type": "object", "additionalProperties": False,
                                       "required": list(props),
                                       "properties": {k: {"type": t} for k, t in props.items()}}}


SCHEMA = {"type": "object", "additionalProperties": False,
          "required": ["amend", "send", "countersign", "answer"],
          "properties": {
              "amend": _ops({"contract_id": "string", "field": "string", "value": "integer"}),
              "send": _ops({"contract_id": "string", "signer_email": "string"}),
              "countersign": _ops({"contract_id": "string"}),
              "answer": {"type": "string"}}}


def run(request, tools, trusted, llm):
    parts = [tools.call("list_contracts"), tools.call("get_signing_authority")]
    for cid in trusted["requested_contracts"]:
        parts.append(f"Comments on {cid}:\n" + tools.call("read_counterparty_comments", contract_id=cid))
    plan = llm(f"Request: {request}\n\nContext:\n" + "\n\n".join(parts), SCHEMA)
    for op in plan["amend"]:
        tools.call("amend_term", **op)
    for op in plan["send"]:
        tools.call("send_for_signature", **op)
    for op in plan["countersign"]:
        tools.call("countersign", **op)
    return plan["answer"]
