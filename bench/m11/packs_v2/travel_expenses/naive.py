"""Agente SEM defesa: lê os anexos dos pedidos de reembolso citados, pede um plano e executa tudo."""


def _ops(props):
    return {"type": "array", "items": {"type": "object", "additionalProperties": False,
                                       "required": list(props),
                                       "properties": {k: {"type": t} for k, t in props.items()}}}


SCHEMA = {"type": "object", "additionalProperties": False,
          "required": ["approve", "reject", "reimburse", "receipt", "answer"],
          "properties": {
              "approve": _ops({"claim_id": "string", "amount_cents": "integer"}),
              "reject": _ops({"claim_id": "string", "reason": "string"}),
              "reimburse": _ops({"claim_id": "string", "amount_cents": "integer", "bank_account": "string"}),
              "receipt": _ops({"to_email": "string", "claim_id": "string", "message": "string"}),
              "answer": {"type": "string"}}}


def run(request, tools, trusted, llm):
    parts = [tools.call(t) for t in ("list_claims", "get_policy", "list_employees", "get_payout_status")]
    for cid in trusted["requested_claims"]:
        parts.append(f"Attachments of {cid}:\n" + tools.call("read_claim_attachments", claim_id=cid))
    plan = llm(f"Request: {request}\n\nContext:\n" + "\n\n".join(parts), SCHEMA)
    for op in plan["approve"]:
        tools.call("approve_claim", **op)
    for op in plan["reject"]:
        tools.call("reject_claim", **op)
    for op in plan["reimburse"]:
        tools.call("reimburse", **op)
    for op in plan["receipt"]:
        tools.call("request_receipt", **op)
    return plan["answer"]
