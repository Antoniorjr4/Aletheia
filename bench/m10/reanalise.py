"""Reanálise posterior dos programas Aletheia que o `athc` rejeitou no M10 original.

**Não altera o resultado do M10.** O resultado oficial é o de `runs/evaluation/` (critério
pré-registrado: NÃO SUPERIOR). Este script só registra, versão a versão, o que o compilador atual
diz dos mesmos arquivos e, para os que agora compilam, o que o modelo adversarial (sem custo) faz
com eles. Os 45 arquivos são banco de regressão do desenvolvimento: melhora neles não é evidência
sobre a linguagem. Evidência só com especificações cegas novas e campanha paga.

O que NÃO é medido aqui: cenários legítimos (exigem o modelo real). A coluna fica "não medido".

Uso: python bench/m10/reanalise.py [--seeds 30] [--jobs 8]
Saída: runs/reanalise/{tabela.json,TABELA.md}
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))

AUTHORING = HERE / "runs" / "authoring"
OUT = HERE / "runs" / "reanalise"

# Categoria de cada código de diagnóstico. "garantia" é a linguagem recusando um programa inseguro
# ou não verificável: não é atrito a remover, é o trabalho do compilador.
CATEGORIAS = [
    (r"^P", "sintaxe"),
    (r"^E00(01|02|03|04|05|06|09|10)$", "nomes"),
    (r"^E00(07|08|11)$", "tipos"),
    (r"^E0012$", "exaustividade"),
    (r"^E04", "garantia: política e integridade"),
    (r"^E05", "garantia: efeitos e capacidades"),
    (r"^B", "garantia: orçamento"),
]


def categoria(code: str) -> str:
    for pat, nome in CATEGORIAS:
        if re.match(pat, code):
            return nome
    return "outros"


def codes(text: str) -> list[str]:
    return re.findall(r"error\[(\w+)\]", text)


def check(athc: str, src: Path) -> tuple[bool, list[str], str]:
    r = subprocess.run([athc, "check", str(src)], capture_output=True, text=True)
    out = r.stdout + r.stderr
    return r.returncode == 0, codes(out), out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seeds", type=int, default=30)
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    ap.add_argument("--athc", default=os.environ.get("ATHC", str(ROOT / "target" / "release" / "athc")))
    a = ap.parse_args()
    os.environ["ATHC"] = a.athc
    os.environ.setdefault("M10_EMIT_CACHE", str(OUT / "emit-cache"))
    import evaluate_all  # depois de ATHC: o harness lê a variável na importação

    OUT.mkdir(parents=True, exist_ok=True)
    ev = evaluate_all.Evaluator(HERE / "packs_v2", OUT, a.seeds, real=False, jobs=a.jobs)
    linhas = []
    for res in sorted(AUTHORING.glob("*/ath/rep*/result.json")):
        d = json.loads(res.read_text())
        base = res.parent
        for cy in d["cycles"]:
            fb = cy.get("feedback") or ""
            if "athc check" not in fb:
                continue  # só as versões que o compilador rejeitou no M10
            src = base / cy["file"]
            antes = codes(fb)
            ok, agora, _ = check(a.athc, src)
            linha = {
                "pacote": d["pack"], "rep": base.name, "versao": cy["file"],
                "erros_antes": len(antes), "codigos_antes": antes,
                "erros_agora": len(agora), "codigos_agora": agora,
                "categorias_agora": dict(Counter(categoria(c) for c in agora)),
                "corrigidos": sorted(set(antes) - set(agora)),
                "remanescentes": sorted(set(antes) & set(agora)),
                "revelados": sorted(set(agora) - set(antes)),
                "compila": ok,
                "executa_corretamente": "não medido",
            }
            if ok:
                s = evaluate_all.summarize_version(ev.version("ath", src, d["pack"]))
                linha["executavel"] = s["runnable"]
                linha["cenarios_ataque_violados"] = f"{s['violated']}/{s['attack_scenarios']}"
                linha["seguro"] = s["secure"]
            linhas.append(linha)
    (OUT / "tabela.json").write_text(json.dumps(linhas, ensure_ascii=False, indent=1))
    (OUT / "TABELA.md").write_text(render(linhas, a.seeds), encoding="utf-8")
    print(render(linhas, a.seeds))
    return 0


def render(ls: list, seeds: int) -> str:
    n = len(ls)
    comp = [x for x in ls if x["compila"]]
    cat = Counter()
    for x in ls:
        cat.update(x["categorias_agora"])
    out = [
        "# M10 — reanálise das versões Aletheia rejeitadas (diagnóstico, não evidência)",
        "",
        "Gerado por `bench/m10/reanalise.py` com o compilador do branch. O resultado oficial do M10",
        "continua sendo o de `runs/evaluation/` (NÃO SUPERIOR). Estes arquivos são banco de regressão.",
        f"Ataques: {seeds} execuções do modelo adversarial por cenário, sem custo. Cenários legítimos:",
        "não medidos (exigem o modelo real).",
        "",
        f"- versões: {n}; compilam agora: {len(comp)}; executáveis: "
        f"{sum(1 for x in comp if x.get('executavel'))}; seguras: {sum(1 for x in comp if x.get('seguro'))}; "
        f"violam regra de segurança: {sum(1 for x in comp if x.get('executavel') and not x.get('seguro'))}",
        f"- erros: {sum(x['erros_antes'] for x in ls)} reportados no M10 → {sum(x['erros_agora'] for x in ls)} agora",
        "- erros agora por categoria: " + ", ".join(f"{k} {v}" for k, v in cat.most_common()),
        "",
        "| pacote | rep | versão | erros M10 | erros agora | corrigidos | remanescentes | revelados | compila | ataque violado |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for x in ls:
        out.append(
            f"| {x['pacote']} | {x['rep']} | {x['versao']} | {x['erros_antes']} | {x['erros_agora']} | "
            f"{' '.join(x['corrigidos']) or '—'} | {' '.join(x['remanescentes']) or '—'} | "
            f"{' '.join(x['revelados']) or '—'} | {'sim' if x['compila'] else 'não'} | "
            f"{x.get('cenarios_ataque_violados', '—')} |"
        )
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    sys.exit(main())
