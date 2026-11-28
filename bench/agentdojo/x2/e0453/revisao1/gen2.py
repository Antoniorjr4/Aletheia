T = open("f01_if_ramo_misturado.ath").read()
import re
head, rest = T.split("    match p { Plan { ops, extra, note, gs, one } => {\n", 1)
def mk(body, types_extra=""):
    h = head.replace("type Plan", types_extra + "type Plan") if types_extra else head
    return h + "    match p { Plan { ops, extra, note, gs, one } => {\n      " + body + "\n      \"ok\" } }\n  }\n}\n"
AB = "for a in ops {{ for b in ops {{ {} }} }}"
C = {
 "f24_g_match_xs_ys": ("for g in gs { match g { G { xs, ys } => pay(bank, fit(join(xs.to, \"\"), 16), join(ys.memo, \"\")) } }", ""),
 "f25_g_lacos_xs_ys": ("for g in gs { for a in g.xs { for b in g.ys { pay(bank, a.to, b.memo) } } }", ""),
 "f26_match_dois_variantes": (AB.format("let x = match k { K1 { } => a.to ++ b.memo, K2 { } => a.to }\n pay(bank, x, \"m\")"), "type K = | K1 { } | K2 { }\n"),
 "f27_atomic_on_failure": (AB.format("let r = atomic recover c { a.memo } on failure(e) { b.memo }\n pay(bank, a.to, r)"), ""),
 "f28_if_sem_else": (AB.format("let x = if c { a.to ++ b.memo } else { b.to }\n pay(bank, x, \"m\")"), ""),
 "f29_if_aninhado_ramos_iguais": (AB.format("let x = if c { a } else { b }\n let y = if c { Pay { to: x.to, memo: a.memo, n: 0 } } else { b }\n pay(bank, y.to, y.memo)"), ""),
}
for k,(b,t) in C.items():
    s = mk(b, t)
    if "k" in b and t: s = s.replace("c: Bool ^ Trusted)", "c: Bool ^ Trusted, k: K ^ Trusted)")
    open(k+".ath","w").write(s)
