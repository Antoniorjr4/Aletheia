exec(open('b4.py').read().split("def mk")[0])
def mk(n, b): open(n + '.ath', 'w').write(H.replace('%BODY%', b))
mk('fp4_wrap_same_list', '''      let w = W { sub: left }
      pay(bank, fit(join(w.sub.xs.to, ","), 16), join(left.xs.memo, ","))''')
