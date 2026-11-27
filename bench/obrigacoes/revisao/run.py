#!/usr/bin/env python3
"""uso: run.py prog.ath mundo.json roteiro.json [athc...]  -- obligations + ARX + PY"""
import json, os, subprocess, sys, tempfile
from pathlib import Path
ROOT = Path("/home/claude/aletheia")
def jr(p):
    return [json.loads(x) for x in p.read_text().splitlines() if x.strip()] if p.exists() else []
def summ(j):
    return {"calls": [[a for a in e["detail"]["args"] if not isinstance(a, dict)] for e in j if e["kind"] == "tool"],
            "refused": [e["detail"] for e in j if e["kind"] == "contract_refused"],
            "abort": [e["detail"].get("code") for e in j if e["kind"] == "abort"],
            "oblrej": sum(1 for e in j if e["kind"] == "obligation_reject")}
prog, world, rot = sys.argv[1:4]
ARGS = json.loads(os.environ.get('ARGS', '["\\"x\\""]'))
comps = sys.argv[4:] or [str(ROOT / "target/release/athc")]
env = {k: v for k, v in os.environ.items() if k not in ("OPENAI_API_KEY", "GROQ_API_KEY", "OPENAI_BASE_URL")}
for athc in comps:
    print(f"### {athc}")
    ob = subprocess.run([athc, "obligations", prog], capture_output=True, text=True)
    print((ob.stdout + ob.stderr).strip())
    if ob.returncode != 0:
        continue
    with tempfile.TemporaryDirectory() as d:
        d = Path(d)
        r = subprocess.run([athc, "run", prog, "--world", world, "--models", rot, "--provider", "scripted",
                            *sum((["--arg", a] for a in ARGS), []), "--journal", str(d / "a.jsonl")], capture_output=True, text=True, env=env)
        tail = [l.strip() for l in (r.stdout + r.stderr).splitlines() if "resultado" in l or "abortado" in l or "R0" in l]
        print("ARX:", summ(jr(d / "a.jsonl")), tail[-1:] )
        e = subprocess.run([athc, "emit", prog, "--target", "py", "--out", str(d / "p.py")], capture_output=True, text=True)
        if e.returncode != 0:
            print("emit falhou", e.stderr[-300:]); continue
        env2 = dict(env, PYTHONPATH=str(ROOT / "runtimes/python"))
        subprocess.run(["python3", "-m", "aletheia_runtime", "run", str(d / "p.py"), "--world", world, "--models", rot,
                        "--provider", "scripted", *sum((["--arg-json", a] for a in ARGS), []), "--journal", str(d / "p.jsonl"), "--report",
                        str(d / "r.json")], capture_output=True, text=True, env=env2)
        rep = json.loads((d / "r.json").read_text()) if (d / "r.json").exists() else {}
        print("PY: ", summ(jr(d / "p.jsonl")), rep.get("result") if rep.get("ok") else rep.get("error"))
