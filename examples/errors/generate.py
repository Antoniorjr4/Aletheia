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
    # ---- integridade (E3)
    "e0412_injection.ath": ("Integridade: age sobre a amostra crua do modelo, sem passar pela política",
        [("    match plan {", "    match sample(draft) {")]),
    "e0412_implicit_flow.ath": ("Integridade: o log não confiável decide se o reinício acontece (fluxo implícito)",
        [("        commit restart(cluster, deploy, keys.mint())\n",
          "        if l == \"ok\" { commit restart(cluster, deploy, keys.mint()) } else { \"ignorado\" }\n")]),
    "e0415_prompt_injection.ath": ("Integridade: log não confiável entra no prompt como instrução",
        [("logs = quote(fit(l, 2000)),", "logs = fit(l, 2000),")]),
    "e0417_untrusted_postcondition.ath": ("Sagas: a pós-condição observa o serviço do alerta, não o deployment do plano",
        [("await healthy(cluster, deploy) within 20.0", "await healthy(cluster, service) within 20.0")]),
    # ---- multiplicidade (E1)
    "e1104_key_reuse.ath": ("Multiplicidade: a mesma chave de idempotência em duas operações",
        [("        commit restart(cluster, deploy, keys.mint())\n        commit page(oncall, deploy ++ \" · reiniciado\", keys.mint())",
          "        let k = keys.mint()\n        commit restart(cluster, deploy, k)\n        commit page(oncall, deploy ++ \" · reiniciado\", k)")]),
    "e0042_own_missing.ath": ("Multiplicidade: chave linear sem posse explícita",
        [("tool restart(c: Cap[Cluster.Write], deploy: Text[..32] ^ Endorsed, k: own IdemKey)",
          "tool restart(c: Cap[Cluster.Write], deploy: Text[..32] ^ Endorsed, k: IdemKey)")]),
    "e1107_pending_dropped.ath": ("Multiplicidade: o tratador descarta a obrigação em aberto",
        [("    reconcile p fuel 3 else {\n      transfer p\n      \"pendente\"\n    }", "    \"pendente\"")]),
    # ---- efeitos, capacidades e sagas (E2)
    "e0502_capability.ath": ("Capacidades: tenta reiniciar com a capacidade do pager",
        [("commit restart(cluster, deploy, keys.mint())", "commit restart(oncall, deploy, keys.mint())")]),
    "e0501_effect_row.ath": ("Efeitos: o handler esconde que envia páginas",
        [("! { Infer, Cluster.Read, Cluster.Write, Page, TimeoutUnknown, CompensationFailed } {",
          "! { Infer, Cluster.Read, Cluster.Write, TimeoutUnknown, CompensationFailed } {")]),
    "t0301_commit.ath": ("Efeitos: efeito irrevogável sem `commit`",
        [("        commit restart(cluster, deploy, keys.mint())", "        restart(cluster, deploy, keys.mint())")]),
    "t0302_irrevocable_in_atomic.ath": ("Sagas: reinício (irrevogável) dentro da saga",
        [("          scale(cluster, deploy, to, keys.mint())", "          commit restart(cluster, deploy, keys.mint())")]),
    "e0503_compensation_unhandled.ath": ("Sagas: saga com efeito compensável sem tratar `CompensationFailed`",
        [("        } on compensation_failed(cf) {\n          \"intervenção humana: \" ++ cf\n        }", "        }")]),
    "e0503_timeout_unhandled.ath": ("Efeitos: chamadas com chave sem tratador de `TimeoutUnknown`",
        [("\n  -- ⑧", "\n}\n\n-- ⑧"), ("  handle timeout_unknown(p: own Pending[_]) -> Text {\n    reconcile p fuel 3 else {\n      transfer p\n      \"pendente\"\n    }\n  }\n}\n", "")]),
    "e0505_compensator_signature.ath": ("Sagas: o compensador não tem a assinatura da operação original",
        [("tool unscale(c: Cap[Cluster.Write], deploy: Text[..32] ^ Endorsed, to: Int[2..64], k: own IdemKey)",
          "tool unscale(c: Cap[Cluster.Write], deploy: Text[..32] ^ Endorsed, k: own IdemKey)")]),
    # ---- refinamentos (E5) e orçamento (E6)
    "e0451_refinement_literal.ath": ("Refinamentos: literal fora de Int[2..64]",
        [("scale(cluster, deploy, to, keys.mint())", "scale(cluster, deploy, 0, keys.mint())")]),
    "b0110_unbounded_prompt.ath": ("Orçamento: texto sem limite de tokens no prompt",
        [("logs = quote(fit(l, 2000)),", "logs = quote(l),")]),
    "b0207_wall_budget.ath": ("Orçamento: o caminho de falha da saga não cabe em 45 s",
        [("wall <= 90.0]", "wall <= 45.0]")]),
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
