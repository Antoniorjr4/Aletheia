import json
C = {
 # controles
 "ctl_m1": "for a in ops { for b in ops { pay(bank, a.to, b.memo) } }",
 "ctl_u7": "for a in ops { pay(bank, a.to, a.memo) }",
 # F1: quant sobre lista confiavel com if dependente da iteracao
 "f1_quant_if": "for a in ops { for b in ops { pay(bank, text(sum i in [1, 2] => if i == 1 { a.n } else { b.n }), \"m\") } }",
 "f1b_quant_if_match": "for a in ops { for b in ops { pay(bank, text(sum i in [1, 2] => match a { Pay { n } => if i == 1 { n } else { match b { Pay { n } => n } } }), \"m\") } }",
 # F2: refine dentro de quantificador
 "f2_quant_refine": "pay(bank, text(sum i in [1, 2] => match (refine (infer planner -> Op { request = req }) by Q else { return \"none\" }) { Pay { n } => n }), \"m\")",
 # FP: if entre agregado e elemento
 "fp_if_agg_elem": "for a in ops { pay(bank, \"k\", if c { join(ops.memo, \",\") } else { a.memo }) }",
 # lista de listas
 "l1_lista_de_listas": "for l in [ops] { for a in l { for b in l { pay(bank, a.to, b.memo) } } }",
 "l2_lista_de_listas_agg": "for l in [ops, ops] { for a in l { pay(bank, a.to, join(l.memo, \",\")) } }",
 "l3_if_lit_ou_plano": "for a in ops { let l = if c { [a] } else { ops }\n for x in l { for y in l { pay(bank, x.to, y.memo) } } }",
 "l4_ctor_lit": "for a in ops { for b in ops { let g = Plan { ops: [a, b], extra: extra, note: note }\n match g { Plan { ops } => for x in ops { for y in ops { pay(bank, x.to, y.memo) } } } } }",
 "l5_lit_projecao": "for a in ops { for b in ops { for t in [a, b].to { pay(bank, t, b.memo) } } }",
 "l6_quant_lit": "for a in ops { for b in ops { pay(bank, a.to, text(sum w in [a, b] => w.n)) } }",
 "l7_lit_ctor_mix_proj": "for a in ops { let ll = [Plan { ops: ops, extra: extra, note: a.to }]\n for l in ll.ops { for x in l { for y in l { pay(bank, x.to, y.memo) } } } }",
 "l8_lit_aninhada": "for a in ops { for b in ops { for l in [[a, b]] { for x in l { for y in l { pay(bank, x.to, y.memo) } } } } }",
 "l9_lit_aninhada_mix": "for a in ops { for b in ops { for l in [[Pay { to: a.to, memo: b.memo, n: 0 }]] { for x in l { pay(bank, x.to, x.memo) } } } }",
 "l10_any_lit": "for a in ops { for b in ops { if any w in [a, b] => w.n > 1 { pay(bank, a.to, a.memo) } } }",
 "l11_lit_quant_stale": "for a in ops { for b in ops { let k = sum l in [Plan { ops: ops, extra: extra, note: a.to }] => 0\n let g = Plan { ops: ops, extra: extra, note: text(k) }\n match g { Plan { ops } => for x in ops { for y in ops { pay(bank, x.to, y.memo) } } } } }",
}
json.dump(C, open("c1.json","w"))
