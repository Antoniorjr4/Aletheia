#!/usr/bin/env python3
"""Gera as variantes com erro a partir de examples/remediate.ath.

Cada variante aplica uma única edição ao programa válido; o cabeçalho diz qual
erro o `athc` deve relatar. Rode depois de mudar o exemplo base:

    python3 examples/errors/generate.py
"""
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
BASE = (HERE.parent / "remediate.ath").read_text()

VARIANTS = {
    "e0412_injection.ath": ("Integridade: aplica a amostra crua do modelo, sem passar pela política",
        [("let receipt = apply(cluster, plan, k)", "let receipt = apply(cluster, sample(draft), k)")]),
    "e0412_implicit_flow.ath": ("Integridade: o log não confiável decide se a ação roda (fluxo implícito)",
        [("    let k = keys.mint()\n    let receipt = apply(cluster, plan, k)",
          "    let receipt = if l == \"ok\" { apply(cluster, plan, keys.mint()) } else { \"ignorado\" }")]),
    "e0415_prompt_injection.ath": ("Integridade: log não confiável entra no prompt como instrução",
        [("logs = quote(fit(l, 2000)),", "logs = fit(l, 2000),")]),
    "e1104_key_reuse.ath": ("Multiplicidade: a mesma chave de idempotência em duas operações",
        [("commit page(oncall, \"remediado: \" ++ receipt, keys.mint())",
          "commit page(oncall, \"remediado: \" ++ receipt, k)")]),
    "e0502_capability.ath": ("Capacidades: tenta escrever no cluster com a capacidade do pager",
        [("apply(cluster, plan, k)", "apply(oncall, plan, k)")]),
    "e0501_effect_row.ath": ("Efeitos: o handler esconde que envia páginas",
        [("! { Infer, Cluster.Read, Cluster.Write, Page } {", "! { Infer, Cluster.Read, Cluster.Write } {")]),
    "t0301_commit.ath": ("Efeitos: efeito irrevogável sem `commit`",
        [("    commit page(oncall, \"remediado: \"", "    page(oncall, \"remediado: \"")]),
    "e0451_refinement_literal.ath": ("Refinamentos: literal fora de Int[2..64]",
        [("let receipt = apply(cluster, plan, k)",
          "let receipt = apply(cluster, Scale { deploy: \"api\", to: 0 }, k)")]),
    "b0110_unbounded_prompt.ath": ("Orçamento: texto sem limite de tokens no prompt",
        [("logs = quote(fit(l, 2000)),", "logs = quote(l),")]),
    "b0207_wall_budget.ath": ("Orçamento: 3 tentativas não cabem em 15 s no pior caminho",
        [("wall <= 60.0]", "wall <= 15.0]")]),
}

def main():
    for old in HERE.glob("*.ath"):
        old.unlink()
    for name, (why, edits) in VARIANTS.items():
        src = BASE
        for old, new in edits:
            assert src.count(old) == 1, (name, old)
            src = src.replace(old, new)
        head = f"-- ERRO ESPERADO · {why}\n-- Derivado de examples/remediate.ath por uma única edição.\n"
        (HERE / name).write_text(head + src)
    print(f"{len(VARIANTS)} variantes geradas em {HERE}")

if __name__ == "__main__":
    main()
