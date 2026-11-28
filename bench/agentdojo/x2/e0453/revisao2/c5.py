import json
C = {
 "f1c_quant_bool_lista": "for a in ops { for b in ops { pay(bank, text(sum t in [true, false] => if t { a.n } else { b.n }), \"m\") } }",
 "f1d_quant_match_elem_dentro": "for a in ops { for b in ops { let x = if c { a } else { b }\n pay(bank, text(sum i in [1, 2] => match x { Pay { n } => n }), \"m\") } }",
 "f1e_quant_sobre_fora": "for a in ops { for b in ops { pay(bank, text(sum w in fora => if len(fora) > 0 { a.n } else { b.n }), \"m\") } }",
 "cat1_lista_concat": "for a in ops { for b in ops { for x in [a] ++ [b] { for y in [a] ++ [b] { pay(bank, x.to, y.memo) } } } }",
 "cat2_concat_plano": "for a in ops { for x in ops ++ [a] { pay(bank, x.to, x.memo) } }",
 "lit12_if_lit_mix_vs_plano": "for a in ops { for b in ops { let l = if c { [Pay { to: a.to, memo: b.memo, n: 0 }] } else { ops }\n for x in l { pay(bank, x.to, x.memo) } } }",
 "lit13_ctor_lit_campo_lista": "for a in ops { for b in ops { let g = Plan { ops: [Pay { to: a.to, memo: b.memo, n: 0 }], extra: extra, note: note }\n match g { Plan { ops } => for x in ops { pay(bank, x.to, x.memo) } } } }",
 "lit14_ponto_lista_literal_campo": "for a in ops { for b in ops { let ll = [Plan { ops: ops, extra: extra, note: note }]\n for l in ll.ops { for x in l { for y in l { pay(bank, x.to, y.memo) } } } } }",
 "lit15_sum_lit_dentro_for": "for a in ops { for b in ops { for x in [a, b] { pay(bank, x.to, text(sum w in [x] => w.n)) } } }",
}
json.dump(C, open("c5.json","w"))
