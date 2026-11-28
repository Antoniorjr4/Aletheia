from gen import mk
mk('fp1_rep_refine_two_fields', '''      pay(bank, "k", text(sum i in [1, 2] => match (refine (infer planner -> Op { request = req }) by Q else { return "none" }) { Pay { n, to } => n + len(to) }))''')
mk('fp2_if_left_right_loop', '''      let x = if c { left } else { right }
      for a in x.xs { pay(bank, a.to, a.memo) }''')
mk('fp3_lit_list_same', '''      for g in [G { xs: ops, ys: ops, tag: "t" }] { pay(bank, "k", fit(join(g.xs.to, ","), 16)) }''')
