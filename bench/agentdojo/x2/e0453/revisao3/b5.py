from gen import mk
mk('b20_nested_quant', '''      for a in ops { for b in ops { pay(bank, text(sum v in [1, 2] => sum w in [3, 4] => if v == w { a.n } else { b.n }), "m") } }''')
mk('b21_lit_of_mixed_lit', '''      for a in ops { for b in ops { for l in [[Pay { to: a.to, memo: b.memo, n: 0, subs: a.subs }], ops] { for w in l { pay(bank, w.to, w.memo) } } } }''')
mk('b22_quant_refine_else_join', '''      for a in ops { for b in ops { let x = if c { a } else { b }
        pay(bank, x.to, text(sum i in [1, 2] => match (refine (infer planner -> Op { request = req }) by Q else { x }) { Pay { n } => n })) } }''')
mk('b23_quant_for_if_lit', '''      for a in ops { for b in ops { pay(bank, a.to, text(sum i in [1, 2] => sum w in (if i == 1 { [a] } else { [b] }) => w.n)) } }''')
mk('b24_quant_in_quant_iter_outer', '''      for a in ops { for b in ops { pay(bank, text(sum i in (if c { [a] } else { [b] }) => i.n), "m") } }''')
mk('b25_lost_g_elem_join', '''      for g in gs { for ll in [[g.xs, g.ys]] { for l in ll { pay(bank, g.tag, "m") } } }''')
