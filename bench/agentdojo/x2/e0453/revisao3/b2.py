from gen import mk
mk('b07_q_ll', '''      pay(bank, note, text(sum l in [ops, left.xs] => sum w in l => w.n))''')
mk('b08_for_l_root', '''      for l in [ops, left.xs] { pay(bank, note, text(sum w in l => w.n)) }''')
mk('b09_for_l_m', '''      for l in [ops, left.xs] { for m in [ops, left.xs] { pay(bank, text(len(l)), text(len(m))) } }''')
mk('b10_ll_join_to', '''      for ll in [[ops, left.xs]] { for l in ll { pay(bank, note, fit(join(l.to, ","), 16)) } }''')
mk('b11_fp_if_left_right', '''      let x = if c { left } else { right }
      pay(bank, fit(join(x.xs.to, ","), 16), "m")''')
mk('b12_ll_two_lists_join', '''      for ll in [[ops, left.xs]] { pay(bank, fit(join(ll.to, ","), 16), "m") }''')
mk('b13_ll_elem_g', '''      for g in gs { for ll in [[g.xs, g.ys]] { pay(bank, g.tag, text(sum l in ll => sum w in l => w.n)) } }''')
mk('b14_ll_with_elem', '''      for a in ops { for ll in [[ops, left.xs]] { pay(bank, a.to, text(sum l in ll => sum w in l => w.n)) } }''')
