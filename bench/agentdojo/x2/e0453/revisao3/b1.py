from gen import mk
mk('b01_ll_sum', '''      for ll in [[ops, left.xs]] { pay(bank, note, text(sum l in ll => len(l))) }''')
mk('b02_ll_sum_noroot', '''      for ll in [[ops, left.xs]] { pay(bank, "x", text(sum l in ll => sum w in l => w.n)) }''')
mk('b03_ll_ctrl', '''      for ll in [[ops, left.xs]] { pay(bank, note, "x") }''')
mk('b04_l_single', '''      for l in [ops] { pay(bank, note, text(len(l))) }''')
mk('b05_g_sub', '''      for g in gs { for ll in [[g.xs, g.ys]] { pay(bank, g.tag, text(sum l in ll => len(l))) } }''')
mk('b06_g_sub_tos', '''      for g in gs { for ll in [[g.xs, g.ys]] { for l in ll { pay(bank, g.tag, text(len(l))) } } }''')
