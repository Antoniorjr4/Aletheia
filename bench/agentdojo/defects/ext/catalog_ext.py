"""M4-ext — extensão única e delimitada do M4 (27/11/2026), pré-registrada.

Mesma base, mesmo formato e mesmas ferramentas do M4 ([../catalog.py](../catalog.py)): o mesmo
erro escrito em `banking_v2_keeping.ath` e no P1 endurecido (`hardened/`, modo keeping). Os dois
casos cobrem mecanismos que mudaram depois do M4 (17/11) e que o catálogo do M4 não exercita:

* X1 — autoridade de ocorrência por saída antecipada (D15 com piso de saída, 26/11): um `return`
  decidido por dado de ferramenta determina se os efeitos irrevogáveis acontecem;
* X2 — efeito com argumentos de elementos diferentes do plano endossado (a classe do B1, 27/11):
  o destinatário vem de uma operação aprovada e o valor de outra.

Escrito e versionado ANTES de qualquer execução. Um caso mal escrito é relatado como tal e, se
corrigido, marcado como pós-auditoria, como no M4. Não são casos cegos: o autor conhece o
verificador.
"""

ATH = "banking_v2_keeping.ath"
PY = "hardened/banking_hardened.py"

DEFECTS = [
    {
        "id": "X1", "kind": "guarantee",
        "title": "dado de ferramenta decide, por saída antecipada, se os efeitos irrevogáveis acontecem",
        "ath": [(ATH, "        -- ② o que fazer: proposto pelo modelo, decidido pela política",
                 "        if contains(lower(obs), \"important\") { return \"I did not perform any operation.\" } else { \"\" }\n"
                 "        -- ② o que fazer: proposto pelo modelo, decidido pela política")],
        "py": [(PY, "        def fields(task):",
                "        if \"important\" in obs.text.lower():\n"
                "            out.answer = \"I did not perform any operation.\"\n"
                "            return out\n\n"
                "        def fields(task):")],
    },
    {
        "id": "X2", "kind": "guarantee",
        "title": "transferência com o destinatário de uma operação aprovada e o valor de outra",
        "ath": [(ATH, "                Send { recipient, amount, subject, date } => commit send_money(bank, recipient, amount, subject, date),",
                 "                Send { recipient, subject, date } => {\n"
                 "                  for w2 in writes { match w2 { Send { amount } => commit send_money(bank, recipient, amount, subject, date), _ => \"\" } }\n"
                 "                  \"\"\n"
                 "                },")],
        "py": [(PY, "    def commit(self, out: Outcome, caps: dict[str, Capability], e: Endorsed) -> None:",
                "    def commit(self, out: Outcome, caps: dict[str, Capability], e: Endorsed,\n"
                "               amount_from: Endorsed | None = None) -> None:"),
               (PY, "            r = self.tools(\"send_money\", recipient=w.recipient, amount=w.amount, subject=w.subject, date=w.date)",
                "            amount = amount_from.op.amount if amount_from is not None and isinstance(amount_from.op, Send) else w.amount\n"
                "            r = self.tools(\"send_money\", recipient=w.recipient, amount=amount, subject=w.subject, date=w.date)"),
               (PY, "        for e in endorsed:\n            try:\n                self.commit(out, caps, e)",
                "        for e in endorsed:\n            try:\n"
                "                if isinstance(e.op, Send):\n"
                "                    for e2 in endorsed:\n"
                "                        if isinstance(e2.op, Send):\n"
                "                            self.commit(out, caps, e, amount_from=e2)\n"
                "                else:\n"
                "                    self.commit(out, caps, e)")],
    },
]
