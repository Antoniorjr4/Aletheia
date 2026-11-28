import json
base = open("p/ctl_m1.ath").read()
B = "for a in ops { for b in ops { pay(bank, a.to, b.memo) } }"
def body(b): return {"full": base.replace(B, b)}
H = base.replace("Text ! { Infer, Bank.Pay }", "Text ! { Infer, Bank.Pay, TimeoutUnknown }")
def handle(hb):
    s = H.replace(B, "pay(bank, \"x\", \"y\")")
    i = s.rstrip().rfind("}")
    return {"full": s[:i] + "  handle timeout_unknown(pp: own Pending[_]) -> Text {\n" + hb + "\n  }\n}\n"}
C = {
 "a1_atomic_tres_ramos": body("for a in ops { for b in ops { let r = atomic recover c { a.memo } on failure(e) { b.memo } on compensation_failed(cf) { a.memo }\n pay(bank, r, r) } }"),
 "a2_atomic_corpo_misturado": body("for a in ops { for b in ops { let r = atomic recover c { a.to ++ b.memo }\n pay(bank, r, \"k\") } }"),
 "a3_atomic_cf_outro": body("for a in ops { for b in ops { let r = atomic recover c { a.memo } on failure(e) { a.memo } on compensation_failed(cf) { b.memo }\n pay(bank, a.to, r) } }"),
 "a4_atomic_registro": body("for a in ops { for b in ops { let r = atomic recover c { a } on failure(e) { b }\n match a { Pay { to } => match r { Pay { memo } => pay(bank, to, memo) } } } }"),
 "a5_atomic_pay_dentro": body("for a in ops { for b in ops { let r = atomic recover c { pay(bank, a.to, b.memo)\n \"k\" }\n pay(bank, \"x\", r) } }"),
 "r1_reconcile_mix": handle("    reconcile pp fuel 3 else {\n      transfer pp\n      \"p\"\n    }"),
}
json.dump(C, open("c4.json","w"))
