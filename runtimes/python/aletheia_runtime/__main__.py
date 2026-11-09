"""`python -m aletheia_runtime run programa.py [opções]`: executa um programa emitido pelo athc.

  --world mundo.json        mundo simulado (padrão: vazio)
  --models modelos.json     roteiro de respostas por modelo
  --provider scripted|masked
  --agent A --on evento     (padrão: o primeiro agente e o primeiro handler)
  --arg-json JSON           argumento do evento no formato `Value::to_json` (repetível)
  --journal saída.jsonl     journal encadeado
  --report relatório.json   resultado, erro, mundo, orçamento e cabeça do journal
"""

from __future__ import annotations

import importlib.util
import json
import sys

from .canon import dumps
from .providers import Masked, Scripted
from .runtime import RtErr, Runtime
from .shape import from_arg
from .values import to_json
from .world import SimWorld


def load_program(path: str):
    spec = importlib.util.spec_from_file_location("aletheia_emitted", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.PROGRAM


def main(argv: list) -> int:
    if len(argv) < 2 or argv[0] != "run":
        print(__doc__, file=sys.stderr)
        return 2
    path, rest = argv[1], argv[2:]
    opts: dict = {}
    i = 0
    while i < len(rest):
        k = rest[i].lstrip("-")
        opts.setdefault(k, []).append(rest[i + 1])
        i += 2
    one = lambda k, d=None: opts.get(k, [d])[-1]
    prog = load_program(path)
    world = SimWorld(json.load(open(one("world"), encoding="utf-8")) if one("world") else {})
    models = json.load(open(one("models"), encoding="utf-8")) if one("models") else {}
    provider = Scripted(models)
    if one("provider", "masked") == "masked":
        provider = Masked(provider)
    agent = one("agent") or next(iter(prog.agents))
    event = one("on") or next(iter(prog.agents[agent]["handlers"]))
    params = prog.agents[agent]["handlers"][event][1]
    raw = [json.loads(a) for a in opts.get("arg-json", [])]
    args = [from_arg(shape, j) if shape is not None else j for (_, shape, _), j in zip(params, raw)]
    rt = Runtime(prog, world, provider)
    result, error = None, None
    try:
        result = to_json(rt.run(agent, event, args))
    except RtErr as e:
        error = {"code": e.code, "msg": e.msg}
    if one("journal"):
        with open(one("journal"), "w", encoding="utf-8", newline="\n") as f:
            f.write(rt.journal.to_jsonl())
    report = {
        "ok": error is None,
        "result": result,
        "error": error,
        "provider": provider.name(),
        "world": world.state(),
        "spent": {"tok": rt.meter.tok, "usd": rt.meter.usd, "wall": rt.meter.wall},
        "journal_head": rt.journal.head,
    }
    if one("report"):
        with open(one("report"), "w", encoding="utf-8", newline="\n") as f:
            f.write(dumps(report))
    else:
        print(dumps(report))
    return 0 if error is None else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
