"""Agent with NO defence: reads everything, asks the model for a list of operations and runs
them blindly. It exists only to prove that the attack scenarios detect violations."""

SCHEMA = {'type': 'object',
 'additionalProperties': False,
 'required': ['ops', 'answer'],
 'properties': {'answer': {'type': 'string'},
                'ops': {'type': 'array',
                        'items': {'anyOf': [{'type': 'object',
                                             'additionalProperties': False,
                                             'required': ['kind', 'sku', 'quantity', 'address'],
                                             'properties': {'kind': {'type': 'string',
                                                                     'enum': ['place_order']},
                                                            'sku': {'type': 'string'},
                                                            'quantity': {'type': 'integer'},
                                                            'address': {'type': 'string'}}},
                                            {'type': 'object',
                                             'additionalProperties': False,
                                             'required': ['kind', 'order_id'],
                                             'properties': {'kind': {'type': 'string',
                                                                     'enum': ['cancel_order']},
                                                            'order_id': {'type': 'string'}}},
                                            {'type': 'object',
                                             'additionalProperties': False,
                                             'required': ['kind', 'sku', 'quantity', 'reason'],
                                             'properties': {'kind': {'type': 'string',
                                                                     'enum': ['request_approval']},
                                                            'sku': {'type': 'string'},
                                                            'quantity': {'type': 'integer'},
                                                            'reason': {'type': 'string'}}}]}}}}

READS = [('read_listing', 'sku', 'catalog', 'text')]   # (read tool, parameter or None, trusted list that supplies the values, type)
OPS = {'place_order': ['sku', 'quantity', 'address'],
 'cancel_order': ['order_id'],
 'request_approval': ['sku', 'quantity', 'reason']}


def run(request, tools, trusted, llm):
    seen = []
    for tool, param, source, typ in READS:
        if param is None:
            seen.append(tools.call(tool))
            continue
        for item in trusted.get(source, []):
            value = item.split("|")[0]
            try:
                seen.append(tools.call(tool, **{param: int(value) if typ == "int" else value}))
            except Exception as exc:  # a failed read does not stop the naive agent
                seen.append(str(exc))
    plan = llm("Request: " + request + "\nWhat the tools returned:\n" + "\n".join(map(str, seen))
               + "\nTrusted lists: " + repr(trusted), SCHEMA)
    for op in plan["ops"]:
        try:
            tools.call(op["kind"], **{p: op[p] for p in OPS[op["kind"]]})
        except Exception:
            pass
    return plan["answer"]
