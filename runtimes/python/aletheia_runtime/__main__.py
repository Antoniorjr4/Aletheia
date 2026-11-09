"""Executa programas emitidos pelo athc.

`python -m aletheia_runtime run programa.py [opções]`

  --world mundo.json        mundo simulado (padrão: vazio)
  --models modelos.json     roteiro de respostas por modelo
  --provider scripted|masked|openai
                            openai: modelo real (OPENAI_API_KEY, OPENAI_BASE_URL, ALETHEIA_MODEL)
  --record cassete.json     com --provider openai: grava as trocas HTTP cruas
  --replay cassete.json     reproduz as respostas gravadas (requisições idênticas exigidas)
  --agent A --on evento     (padrão: o primeiro agente e o primeiro handler)
  --arg-json JSON           argumento do evento no formato `Value::to_json` (repetível)
  --journal saída.jsonl     journal encadeado
  --report relatório.json   resultado, erro, mundo, orçamento e cabeça do journal

`python -m aletheia_runtime replay cassete.json programa.py [--journal saída.jsonl]`

  reexecuta o programa contra as respostas gravadas, com o mundo e os argumentos do manifesto
  do cassete, e exige o mesmo journal gravado, byte a byte
"""

from __future__ import annotations

import importlib.util
import json
import sys

from .canon import dumps
from .cassette import Cassette, first_divergence
from .providers import Masked, OpenAiCompat, Recorder, Replay, Scripted
from .runtime import RtErr, Runtime
from .shape import from_arg
from .values import to_json
from .world import SimWorld


def load_program(path: str):
    spec = importlib.util.spec_from_file_location("aletheia_emitted", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.PROGRAM


def _opts(rest: list) -> dict:
    opts: dict = {}
    i = 0
    while i < len(rest):
        opts.setdefault(rest[i].lstrip("-"), []).append(rest[i + 1])
        i += 2
    return opts


def replay(argv: list) -> int:
    cas = Cassette.load(argv[0])
    prog = load_program(argv[1])
    opts = _opts(argv[2:])
    m = cas.manifest
    agent, event = m["agent"], m["event"]
    params = prog.agents[agent]["handlers"][event][1]
    args = [from_arg(shape, j) if shape is not None else j for (_, shape, _), j in zip(params, m.get("args") or [])]
    provider = OpenAiCompat(cas.config, Replay(cas.exchanges))
    rt = Runtime(prog, SimWorld(m.get("world") or {}), provider)
    try:
        shown = repr(to_json(rt.run(agent, event, args)))
    except RtErr as e:
        shown = f"{e.code} {e.msg}"
    journal = rt.journal.to_jsonl()
    if opts.get("journal"):
        with open(opts["journal"][-1], "w", encoding="utf-8", newline="\n") as f:
            f.write(journal)
    print(f"replay {m.get('source_path', '?')} · {len(cas.exchanges)} troca(s) · {cas.provider} → {shown}")
    d = first_divergence(cas.journal, journal)
    if d is None:
        print(f"✓ journal idêntico ao gravado: {len(rt.journal.entries)} entradas · cabeça #{rt.journal.head[:12]}")
        return 0
    i, a, b = d
    print(f"✗ journal diverge na entrada {i}\n  gravado: {a}\n  replay:  {b}")
    return 1


def main(argv: list) -> int:
    if len(argv) >= 3 and argv[0] == "replay":
        return replay(argv[1:])
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
    recorder = None
    if one("replay"):
        cas = Cassette.load(one("replay"))
        provider = OpenAiCompat(cas.config, Replay(cas.exchanges))
    elif one("provider") == "openai":
        provider = OpenAiCompat.from_env(one("model"), one("base-url"))
        if one("record"):
            recorder = Recorder(provider.transport)
            provider.transport = recorder
    else:
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
    if recorder is not None:
        manifest = {"source_path": one("source") or path, "agent": agent, "event": event, "args": raw,
                    "world": json.load(open(one("world"), encoding="utf-8")) if one("world") else {}}
        if one("source"):
            # o fonte .ath embutido permite reproduzir o cassete também no ARX (`athc replay`)
            manifest["program"] = open(one("source"), encoding="utf-8").read()
        Cassette(provider.name(), provider.config_json(), manifest, recorder.tape, rt.journal.to_jsonl()).save(one("record"))
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
