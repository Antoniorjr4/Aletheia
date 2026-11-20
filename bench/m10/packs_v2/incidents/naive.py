"""Agent with NO defence: reads everything, asks the model for a list of operations and runs
them blindly. It exists only to prove that the attack scenarios detect violations."""

SCHEMA = {'type': 'object',
 'additionalProperties': False,
 'required': ['ops', 'answer'],
 'properties': {'answer': {'type': 'string'},
                'ops': {'type': 'array',
                        'items': {'anyOf': [{'type': 'object',
                                             'additionalProperties': False,
                                             'required': ['kind', 'person', 'message'],
                                             'properties': {'kind': {'type': 'string', 'enum': ['page']},
                                                            'person': {'type': 'string'},
                                                            'message': {'type': 'string'}}},
                                            {'type': 'object',
                                             'additionalProperties': False,
                                             'required': ['kind', 'service'],
                                             'properties': {'kind': {'type': 'string',
                                                                     'enum': ['restart_service']},
                                                            'service': {'type': 'string'}}},
                                            {'type': 'object',
                                             'additionalProperties': False,
                                             'required': ['kind', 'service', 'replicas'],
                                             'properties': {'kind': {'type': 'string',
                                                                     'enum': ['scale_service']},
                                                            'service': {'type': 'string'},
                                                            'replicas': {'type': 'integer'}}},
                                            {'type': 'object',
                                             'additionalProperties': False,
                                             'required': ['kind', 'channel', 'text'],
                                             'properties': {'kind': {'type': 'string',
                                                                     'enum': ['post_status']},
                                                            'channel': {'type': 'string'},
                                                            'text': {'type': 'string'}}},
                                            {'type': 'object',
                                             'additionalProperties': False,
                                             'required': ['kind', 'alert_id', 'minutes'],
                                             'properties': {'kind': {'type': 'string',
                                                                     'enum': ['silence_alert']},
                                                            'alert_id': {'type': 'string'},
                                                            'minutes': {'type': 'integer'}}}]}}}}

READS = [('read_alert_logs', None, None, None)]   # (read tool, parameter or None, trusted list that supplies the values, type)
OPS = {'page': ['person', 'message'],
 'restart_service': ['service'],
 'scale_service': ['service', 'replicas'],
 'post_status': ['channel', 'text'],
 'silence_alert': ['alert_id', 'minutes']}


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
