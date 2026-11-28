H = '''module t
type Op = | Pay { to: Text[..16], memo: Text[..16], n: Int[0..9] }
type G2 = | G2 { xs: List[Op, 0..3] }
type G = | G { xs: List[Op, 0..3], sub: G2 }
type W = | W { sub: G }
type Plan = | Plan { ops: List[Op, 0..3], note: Text[..16], left: G }
model planner { price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 4000  reasoning = 0  ttft = 1.0  per_tok = 0.01 }
tool pay(c: Cap[Bank.Pay], to: Text ^ Endorsed, memo: Text) -> Unit ! { Bank.Pay } reversible latency 1.0
policy Q(o: Op) -> Bool = true
policy P(p: Plan) -> Bool = true
agent A(bank: Cap[Bank.Pay]) budget [tok <= 1000000, usd <= 100.0, wall <= 1000000.0] {
  on go(req: Text[..200] ^ Trusted, c: Bool ^ Trusted) -> Text ! { Infer, Bank.Pay } {
    let p = refine (infer planner -> Plan { request = req }) by P keeping ops by Q as fora else { return "none" }
    match p { Plan { ops, note, left } => {
%BODY%
      "ok" } }
  }
}
'''
def mk(n, b): open(n + '.ath', 'w').write(H.replace('%BODY%', b))
mk('b16_ctor_wrap', '''      let w = W { sub: left }
      pay(bank, fit(join(w.sub.xs.to, ","), 16), join(left.sub.xs.memo, ","))''')
mk('b16_ref_real_two', '''      pay(bank, fit(join(left.xs.to, ","), 16), join(left.sub.xs.memo, ","))''')
mk('b17_ctor_wrap_match', '''      match W { sub: left } { W { sub } => pay(bank, fit(join(sub.xs.to, ","), 16), join(left.sub.xs.memo, ",")) }''')
mk('b17_ctor_wrap_match', '''      let w = W { sub: left }
      match w { W { sub } => pay(bank, fit(join(sub.xs.to, ","), 16), join(left.sub.xs.memo, ",")) }''')
mk('b18_wrap_sum', '''      let w = W { sub: left }
      pay(bank, note, text(len(w.sub.xs) + len(left.sub.xs)))''')
