#!/usr/bin/env python3
"""Fluxo de rótulos através do resultado de ferramenta: reprodução nos dois runtimes (28/11/2026).

Cada programa desta pasta que o compilador aceita roda no ARX (`athc run --host tcp://…`, com as
ferramentas num serviço TCP deste script) e no runtime Python (o módulo emitido, com um host Python).
Nos dois, `echo`/`echot` devolvem o próprio argumento, como uma ferramenta cujo resultado depende da
entrada (tradução, resumo, busca). O script grava o que cada ferramenta recebeu, e o rótulo que o
compilador atribuiu está em `athc check`. Sem rede e sem modelo: o planejador é roteirizado.

    python3 bench/ferramentas/executa.py --out bench/ferramentas/resultado.json
"""
from __future__ import annotations

import argparse
import json
import os
import re
import socket
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(ROOT / "runtimes" / "python"))
ATHC = os.environ.get("ATHC", str(ROOT / "target" / "release" / "athc"))
ARGS = ["pague", "alice", "s3gr3do"]  # req (Trusted), web (Untrusted), sec (Secret)
# nos programas em que `web` decide o efeito, ela vale "sim"
WEB = {"t1b_untrusted_decide_efeito.ath": "sim", "t1c_inline_na_condicao.ath": "sim"}
INV = {"payees": ["alice", "bob"]}
PLAN = {"kind": "Plan", "ops": [{"kind": "Pay", "to": "alice", "memo": "a"}, {"kind": "Pay", "to": "bob", "memo": "b"},
                                {"kind": "Pay", "to": "mallory", "memo": "m"}]}
ECHO = {"echo", "echot", "echou"}


def answer(tool, args):
    vals = [a for a in args if not isinstance(a, dict)]
    return vals[0] if tool in ECHO and vals else None


def check(prog):
    r = subprocess.run([ATHC, "check", str(prog)], capture_output=True, text=True)
    t = r.stdout + r.stderr
    return r.returncode == 0, sorted(set(re.findall(r"(?:error|aviso)\[([A-Z]\d{4})\]", t)))


def args_of(prog):
    return [ARGS[0], WEB.get(prog.name, ARGS[1]), ARGS[2]]


def run_arx(prog):
    received = []
    srv = socket.socket()
    srv.bind(("127.0.0.1", 0))
    srv.listen(8)
    srv.settimeout(0.2)
    port = srv.getsockname()[1]
    stop = threading.Event()

    def serve():
        while not stop.is_set():
            try:
                c, _ = srv.accept()
            except OSError:
                continue
            with c:
                req = json.loads(c.makefile().readline())
                v = None
                if req.get("op") == "call":
                    args = [a for a in req["args"] if not isinstance(a, dict)]
                    received.append({"tool": req["tool"], "args": args})
                    v = answer(req["tool"], args)
                c.sendall((json.dumps({"ok": True, "value": v}) + "\n").encode())

    t = threading.Thread(target=serve, daemon=True)
    t.start()
    with tempfile.TemporaryDirectory() as d:
        d = Path(d)
        (d / "t.json").write_text(json.dumps({"inventories": INV}))
        (d / "m.json").write_text(json.dumps({"planner": [PLAN]}))
        cmd = [ATHC, "run", str(prog), "--host", f"tcp://127.0.0.1:{port}", "--trusted", str(d / "t.json"),
               "--models", str(d / "m.json"), "--provider", "scripted", "--agent", "A", "--on", "go"]
        for a in args_of(prog):
            cmd += ["--arg", json.dumps(a)]
        r = subprocess.run(cmd + ["--journal", str(d / "j.jsonl")], capture_output=True, text=True)
        journal = [json.loads(x) for x in (d / "j.jsonl").read_text().splitlines() if x.strip()] \
            if (d / "j.jsonl").exists() else []
    stop.set()
    t.join()
    srv.close()
    abort = next((e["detail"].get("code") for e in journal if e["kind"] == "abort"), None)
    return {"recebido": received, "aborto": abort, "rc": r.returncode,
            "saida": [x for x in (r.stdout + r.stderr).splitlines() if "resultado" in x or "abort" in x][-1:]}


class Recorder:
    def __init__(self):
        self.received = []

    def lat(self, tool):
        return 0.0

    def call(self, tool, args):
        from aletheia_runtime.values import Cap
        vals = [a for a in args if not isinstance(a, Cap)]
        self.received.append({"tool": tool, "args": vals})
        return answer(tool, vals), 0.0

    def status(self, tool, key):
        raise RuntimeError("sem status")

    def trusted_snapshot(self):
        return {"managed": [], "replicas": {}, "cpu_pct": {}, "inventories": INV}

    def state(self):
        return {}


def run_py(prog):
    from aletheia_runtime.__main__ import load_program
    from aletheia_runtime.providers import Scripted
    from aletheia_runtime.runtime import RtErr, Runtime

    with tempfile.TemporaryDirectory() as d:
        py = Path(d) / "p.py"
        r = subprocess.run([ATHC, "emit", str(prog), "--target", "py", "--out", str(py)], capture_output=True, text=True)
        assert r.returncode == 0, r.stdout + r.stderr
        host = Recorder()
        rt = Runtime(load_program(str(py)), host, Scripted({"planner": [PLAN]}))
        abort = None
        try:
            rt.run("A", "go", args_of(prog))
        except RtErr as e:
            abort = e.code
    return {"recebido": host.received, "aborto": abort}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    out = []
    for prog in sorted(HERE.glob("*.ath")):
        ok, codes = check(prog)
        row = {"programa": prog.name, "compila": ok, "codigos": codes}
        if ok:
            row["arx"], row["python"] = run_arx(prog), run_py(prog)
            row["runtimes_concordam"] = (row["arx"]["recebido"], row["arx"]["aborto"]) == \
                (row["python"]["recebido"], row["python"]["aborto"])
        out.append(row)
        print(prog.name, "compila" if ok else "recusado", codes,
              *(["|", row["arx"]["recebido"], row["arx"]["aborto"], "concordam" if row["runtimes_concordam"] else "DIVERGEM"]
                if ok else []))
    Path(a.out).write_text(json.dumps(out, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
