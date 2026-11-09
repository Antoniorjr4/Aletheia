#!/usr/bin/env python3
"""Imprime os registros brutos de uma execução ao vivo no log do job.

O artefato fica disponível para download na página da execução; este resumo garante
que os dados brutos (journal, amostras de inferência, chamadas do laço Python) também
fiquem legíveis no próprio log, com a chave da API ausente de tudo.
"""

import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "live")


def section(title: str) -> None:
    print(f"\n===== {title} =====", flush=True)


for name in ("run.txt", "journal.jsonl"):
    p = root / name
    if p.exists():
        section(name)
        print(p.read_text().rstrip())

res = root / "m1" / "results.md"
if res.exists():
    section("m1/results.md")
    print(res.read_text().rstrip())

data = root / "m1" / "results.json"
if data.exists():
    section("m1: amostras por execução")
    rows = json.loads(data.read_text())["rows"]
    for r in rows:
        tag = f"{r['id']} #{r['rep']}"
        al = r["aletheia"]
        print(json.dumps({"run": tag, "sistema": "aletheia", "resultado": al.get("result"), "erro": al.get("error"),
                          "acoes": al["actions"], "journal_head": al.get("journal_head")}, ensure_ascii=False))
        for i in al.get("infers") or []:
            print(json.dumps({"run": tag, "infer": i}, ensure_ascii=False))
        run_dir = root / "m1" / "runs" / f"{r['id']}-{r['rep']}"
        jp = run_dir / "journal.jsonl"
        if jp.exists():
            for line in jp.read_text().splitlines():
                e = json.loads(line)
                if e["kind"] == "infer":
                    d = e["detail"]
                    keep = ("attempt", "tok_in", "tok_out", "tok_reasoning", "latency_s", "queued_s", "retries",
                            "contract_s", "request_id", "finish_reason", "output", "reasoning", "invalid")
                    print(json.dumps({"run": tag, "journal_infer": {k: d.get(k) for k in keep}}, ensure_ascii=False))
        py = r["python"]
        calls = [c for m in (py.get("transcript") or []) for c in (m.get("tool_calls") or [])]
        print(json.dumps({"run": tag, "sistema": "python", "erro": py.get("error"), "tokens": py.get("tokens"),
                          "chamadas": calls, "fora_da_politica": py["out_of_policy"]}, ensure_ascii=False))
