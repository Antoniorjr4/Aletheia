#!/usr/bin/env python3
"""M4 com o P1+: o P1 com análise estática de contaminação (Pysa, pyre-check 0.10).

O P1+ é o P1 com UMA reorganização, a que um time que adota o Pysa faria: toda leitura de
ferramenta passa por `HardenedBanker._tool_read` (fonte `ToolData`) e todo efeito por
`HardenedBanker._tool_effect` (sumidouro `Effect`); `_endorse`, a porta da política, é o
sanitizador. Modelos em defects/pysa/. A transformação é mecânica (`to_p1plus`) e é aplicada
DEPOIS da mutação, então cada defeito do catálogo vale igual para P1 e P1+.

    python3 bench/agentdojo/defects/run_pysa.py [--catalog blind.json] [--out runs/m4]
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import site
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent
sys.path.insert(0, str(HERE))
from catalog import DEFECTS, POSTHOC, corrected  # noqa: E402

EFFECTS = "send_money|schedule_transaction|update_scheduled_transaction|update_password|update_user_info"


def to_p1plus(s: str) -> str:
    s = re.sub(rf'self\.tools\("({EFFECTS})"', r'self._tool_effect("\1"', s)
    s = s.replace("return Untrusted.of(str(self.tools(name, **kw))).text",
                  "return Untrusted.of(str(self._tool_read(name, **kw))).text")
    s = s.replace('''    def __init__(self, llm: Callable, tools: Callable, mode: str = "keeping"):''',
                  '''    def _tool_read(self, name: str, **kw: Any) -> str:
        return str(self.tools(name, **kw))

    def _tool_effect(self, name: str, **kw: Any) -> str:
        return str(self.tools(name, **kw))

    def __init__(self, llm: Callable, tools: Callable, mode: str = "keeping"):''')
    return s


def pysa(files: dict[str, str]) -> list[dict]:
    with tempfile.TemporaryDirectory() as d:
        d = Path(d)
        for name, text in files.items():
            (d / name).write_text(to_p1plus(text) if name == "banking_hardened.py" else text)
        (d / "taint").mkdir()
        for f in ("taint.config", "models.pysa"):
            shutil.copy(HERE / "pysa" / f, d / "taint" / f)
        (d / ".pyre_configuration").write_text(json.dumps({
            "source_directories": ["."], "taint_models_path": ["taint"],
            "search_path": [site.getsitepackages()[0]], "exclude": [".*/taint/.*"]}))
        r = subprocess.run(["pyre", "--noninteractive", "analyze", "--no-verify", "--save-results-to", str(d / "out")],
                           cwd=d, capture_output=True, text=True, timeout=900)
        if r.returncode not in (0, 1):
            raise SystemExit(r.stderr[-2000:])
        issues = []
        for line in (d / "out" / "taint-output.json").read_text().splitlines():
            e = json.loads(line)
            if e.get("kind") == "issue":
                issues.append({"code": e["data"]["code"], "callable": e["data"]["callable"], "line": e["data"]["line"]})
        return issues


def mutated(edits) -> dict[str, str]:
    files = {f: (BENCH / "hardened" / f).read_text() for f in ("banking_hardened.py", "agentdojo_adapter.py")}
    for f, old, new in edits:
        name = f.split("/")[-1]
        assert files[name].count(old) == 1, (f, old[:60])
        files[name] = files[name].replace(old, new)
    return files


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--catalog", default=None, help="JSON do catálogo cego (lista com id, title e edits)")
    ap.add_argument("--out", default=str(BENCH / "runs" / "m4"))
    a = ap.parse_args()
    if a.catalog:
        items = [(d["id"], d["title"], [(e["file"], e["old"], e["new"]) for e in d["edits"]])
                 for d in json.loads(Path(a.catalog).read_text())]
        name = "pysa-blind"
    else:
        ds = [corrected(d) if d["id"] in POSTHOC else d for d in DEFECTS]
        items = [(d["id"], d["title"], d["py"]) for d in ds]
        name = "pysa"
    base = pysa(mutated([]))
    out = {"baseline_issues": base, "defects": []}
    for i, title, edits in items:
        found = pysa(mutated(edits))
        out["defects"].append({"id": i, "title": title, "issues": found, "detected": len(found) > len(base)})
        print(i, "detectado" if len(found) > len(base) else "não detectado", found, flush=True)
    Path(a.out).mkdir(parents=True, exist_ok=True)
    (Path(a.out) / f"{name}.json").write_text(json.dumps(out, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
