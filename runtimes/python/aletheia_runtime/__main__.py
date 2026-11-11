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
  --grants concessões.json --run-id R
                            as concessões (`s.authorizes`) e a identidade da execução
  --journal saída.jsonl     journal encadeado
  --core-journal saída.jsonl
                            a vista do núcleo λ_Ath do journal (act, endorse, end), reencadeada
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
from .journal import core_view
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


def crash(argv: list) -> int:
    """`crash programa.py --world w.json --models m.json --at ponto:n --wal saída.jsonl [--fuel 3]`:
    roda com a queda injetada no ponto `ponto` (intent, sent, confirmed, closed) da `n`-ésima
    operação com chave; reinicia com o WAL persistido e o mesmo provedor e recupera. Escreve o
    WAL final. Código 4 se o programa terminou antes da queda."""
    from .wal import Crash, Wal

    path, rest = argv[0], argv[1:]
    opts = {rest[i].lstrip("-"): rest[i + 1] for i in range(0, len(rest), 2)}
    prog = load_program(path)
    world = SimWorld(json.load(open(opts["world"], encoding="utf-8")) if "world" in opts else {})
    models = json.load(open(opts["models"], encoding="utf-8")) if "models" in opts else {}
    point, nth = opts["at"].split(":")
    rt = Runtime(prog, world, Scripted(models))
    rt.wal = Wal()
    rt.wal.crash = (point, int(nth))
    agent = opts.get("agent") or next(iter(prog.agents))
    event = opts.get("on") or next(iter(prog.agents[agent]["handlers"]))
    try:
        rt.run(agent, event, [])
    except Crash:
        pass
    except RtErr:
        return 4
    else:
        return 4
    # o reinício: um processo novo, o WAL que sobrou e o mesmo provedor remoto
    rt2 = Runtime(prog, world, Scripted(models))
    rt2.wal = Wal()
    rt2.wal.log, _ = Wal.read(rt.wal.to_jsonl().encode("utf-8"))
    rt2.recover(int(opts.get("fuel", "3")))
    with open(opts["wal"], "w", encoding="utf-8", newline="\n") as f:
        f.write(rt2.wal.to_jsonl())
    return 0


def main(argv: list) -> int:
    if len(argv) >= 3 and argv[0] == "replay":
        return replay(argv[1:])
    if len(argv) >= 2 and argv[0] == "crash":
        return crash(argv[1:])
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
    if one("grants"):
        rt.run_id = one("run-id") or ""
        rt.load_grants(json.load(open(one("grants"), encoding="utf-8")))
    result, error = None, None
    try:
        result = to_json(rt.run(agent, event, args))
    except RtErr as e:
        error = {"code": e.code, "msg": e.msg}
    if one("journal"):
        with open(one("journal"), "w", encoding="utf-8", newline="\n") as f:
            f.write(rt.journal.to_jsonl())
    if one("core-journal"):
        with open(one("core-journal"), "w", encoding="utf-8", newline="\n") as f:
            comp = [n for n, t in prog.tools.items() if t.get("class") == "compensable"]
            f.write(core_view(rt.journal.entries, comp).to_jsonl())
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
        # Rev13 (M-V): o registro semântico (vista do núcleo, sem tempo) e o de evidências
        # (journal integral, com as medições), separados
        "digests": {
            "semantic": core_view(rt.journal.entries,
                                  {n for n, t in prog.tools.items() if t["class"] == "compensable"}).head,
            "evidence": rt.journal.head,
        },
    }
    if one("report"):
        with open(one("report"), "w", encoding="utf-8", newline="\n") as f:
            f.write(dumps(report))
    else:
        print(dumps(report))
    return 0 if error is None else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
