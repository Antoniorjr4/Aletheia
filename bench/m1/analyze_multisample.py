#!/usr/bin/env python3
"""Aggregate the preregistered M1 live multiamostra results; no synthetic substitutions."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
from statistics import mean


def percentile(values: list[float], q: float) -> float | None:
    """Linear-interpolated sample quantile; N=21 is still small for p95."""
    if not values:
        return None
    ordered = sorted(values)
    index = (len(ordered) - 1) * q
    lower = int(index)
    upper = min(lower + 1, len(ordered) - 1)
    weight = index - lower
    return round(ordered[lower] * (1 - weight) + ordered[upper] * weight, 3)


def latency(rows: list[dict], system: str) -> dict:
    samples = [float(row[system]["wall_s"]) for row in rows
               if isinstance(row[system].get("wall_s"), (int, float))]
    return {"n": len(samples), "p50_s": percentile(samples, 0.50),
            "p95_s": percentile(samples, 0.95), "mean_s": round(mean(samples), 3) if samples else None}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    obj = json.loads(args.results.read_text(encoding="utf-8"))
    summary, rows = obj["summary"], obj["rows"]
    assert summary["model"] == "openai/gpt-oss-120b" and summary["mode"] == "live", "wrong model or simulation"
    assert summary["reps"] >= 3, "not a multiamostra evaluation"
    assert len(rows) == summary["scenarios"] * summary["reps"], "incomplete scenario matrix"

    attacked = [r for r in rows if r["attacked"]]
    benign = [r for r in rows if not r["attacked"]]
    by = {key: [r for r in rows if r["id"] == key] for key in sorted({r["id"] for r in rows})}

    # Duas réguas: o oráculo canônico da Rev6 (`out_of_policy`, comparável com as rodadas
    # anteriores) e, quando presente, o contextual da Rev7 (`out_of_policy_contextual`).
    # Rodadas antigas não declaram o oráculo; a de `449b0cf` mediu `out_of_policy` com o
    # contextual. Sem declaração, o rótulo não afirma qual régua foi usada: veja
    # `bench/m1/rescore.py` para reavaliar sob as duas.
    contextual = "out_of_policy_contextual" in summary["python"]
    canonical = "oráculo canônico da Rev6" if "oracles" in summary else "oráculo da rodada (ver rescore.py)"

    report = [
        f"# M1 real no Groq `{summary['model']}` · multiamostra",
        "",
        f"Commit: `{summary['commit']}` · {summary['scenarios']} cenários × {summary['reps']} repetições, "
        f"{len(attacked)} execuções com injeção e {len(benign)} controles, **por sistema**.",
        "Modelo remoto real; ferramentas e mundos simulados, políticas não relaxadas.",
        "As duas variantes recebem a mesma lista confiável de deployments gerenciados, mas "
        "somente Aletheia aplica capacidades, gramática tipada e política no runtime.",
        "",
        "## Placar agregado",
        "",
        "| Métrica | Python clássico | Aletheia |",
        "| --- | ---: | ---: |",
        f"| Ataques que atingiram objetivo | {summary['python']['attack_success']}/{len(attacked)} | "
        f"{summary['aletheia']['attack_success']}/{len(attacked)} |",
        f"| Ações fora da política · {canonical} | {summary['python']['out_of_policy']} | "
        f"{summary['aletheia']['out_of_policy']} |",
    ]
    if contextual:
        report += [
            f"| Ações fora da política · oráculo contextual da Rev7 | "
            f"{summary['python']['out_of_policy_contextual']} | {summary['aletheia']['out_of_policy_contextual']} |",
        ]
    report += [
        f"| Tarefas cumpridas sob ataque | {summary['python']['utility_under_attack']}/{len(attacked)} | "
        f"{summary['aletheia']['utility_under_attack']}/{len(attacked)} |",
        f"| Tarefas cumpridas sem ataque | {summary['python']['utility_benign']}/{len(benign)} | "
        f"{summary['aletheia']['utility_benign']}/{len(benign)} |",
        f"| Erros de infraestrutura | {summary['python']['errors']} | {summary['aletheia']['errors']} |",
        f"| Tokens (total) | {summary['python']['tokens']:,} | {summary['aletheia']['tokens']:,} |",
    ]
    report += ["", "## Latência por cenário (wall-clock de uma execução, sem espera do pacer)", "",
               "| Sistema | N | p50 | p95 | Média |",
               "| --- | ---: | ---: | ---: | ---: |"]
    for system in ("python", "aletheia"):
        stats = latency(rows, system)
        report.append(f"| {system} | {stats['n']} | {stats['p50_s']} s | {stats['p95_s']} s | {stats['mean_s']} s |")
    report += ["", "## Matriz por cenário (cada linha tem N repetições)", "",
               "| Cenário | Python ataque | Aletheia ataque | Python tarefa | Aletheia tarefa | "
               "Python fora (Rev6 / ctx) | Aletheia fora (Rev6 / ctx) |",
               "| --- | ---: | ---: | ---: | ---: | ---: | ---: |"]
    for id_, subset in by.items():
        n = len(subset)
        def hits(system: str, k: str) -> str:
            return f"{sum(bool(r[system][k]) for r in subset)}/{n}"
        def outside(system: str) -> str:
            canonical = sum(len(r[system]["out_of_policy"]) for r in subset)
            if not contextual:
                return f"{canonical} / —"
            return f"{canonical} / {sum(len(r[system]['out_of_policy_contextual']) for r in subset)}"
        atk_py = hits("python", "attack") if subset[0]["attacked"] else "—"
        atk_ath = hits("aletheia", "attack") if subset[0]["attacked"] else "—"
        report.append(f"| `{id_}` | {atk_py} | {atk_ath} | {hits('python','utility')} | "
                      f"{hits('aletheia','utility')} | {outside('python')} | {outside('aletheia')} |")
    report += ["", "## Limites de interpretação", "",
               "- N=3 por cenário é uma amostra pequena; não justifica alegar segurança inatacável.",
               "- As duas colunas de 'fora' usam réguas diferentes; só a canônica da Rev6 é comparável com rodadas anteriores.",
               "- 0 violações observadas não é uma prova estatística de 0 probabilidade de violação.",
               "- A métrica 'utility' do harness pode ser verdadeira apesar de ações indevidas; leia ao lado de out_of_policy.",
               "- O p95 usa interpolação linear; não é a latência interna da LPU nem inclui esperas do limitador TPM.",
               "- Os agentes recebem formatos/contratos de saída diferentes por desenho; interpretação causal requer controles adicionais.",
               "- Esta é a M1 customizada, não as suítes oficiais do AgentDojo.",
               ""]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(report), encoding="utf-8")
    print("\n".join(report))


if __name__ == "__main__":
    main()
