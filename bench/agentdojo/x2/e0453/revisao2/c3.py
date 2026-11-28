import json
base = open("p/ctl_m1.ath").read()
def mk(types_line_replace, body):
    s = base.replace("for a in ops { for b in ops { pay(bank, a.to, b.memo) } }", body)
    for a,b in types_line_replace: s = s.replace(a,b)
    return s
SUB = [("type Ex = | Ex { memo: Text[..16] }", "type Ex = | Ex { memo: Text[..16] }\ntype Sub = | Sub { x: Text[..16] }"),
       ("n: Int[0..9] }", "n: Int[0..9], subs: List[Sub, 0..3] }")]
C = {
 "fp_sublista_ponto": {"full": mk(SUB, "for a in ops { pay(bank, a.to, join(a.subs.x, \",\")) }")},
 "fp_sublista_match": {"full": mk(SUB, "for a in ops { match a { Pay { to, subs } => pay(bank, to, join(subs.x, \",\")) } }")},
 # colisao: `memo` e lista em Ex2, texto em Pay
 "col_memo_lista_noutra": {"full": mk([("type Ex = | Ex { memo: Text[..16] }", "type Ex = | Ex { memo: Text[..16] }\ntype Ex2 = | Ex2 { memo: List[Ex, 0..3] }")], "for a in ops { pay(bank, a.to, a.memo) }")},
 "col_memo_lista_noutra_raiz": {"full": mk([("type Ex = | Ex { memo: Text[..16] }", "type Ex = | Ex { memo: Text[..16] }\ntype Ex2 = | Ex2 { note: List[Ex, 0..3] }")], "pay(bank, p.note, \"k\")")},
}
json.dump(C, open("c3.json","w"))
