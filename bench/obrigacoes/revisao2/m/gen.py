HEAD = '''module t
type Op = | Pay { to: Text[..16], memo: Text[..16] }
type Plan = | Plan { ops: List[Op, 0..3] }
model planner { price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 4000  reasoning = 0  ttft = 1.0  per_tok = 0.01 }
tool pay(c: Cap[Bank.Pay], to: Text ^ Endorsed, memo: Text ^ Endorsed) -> Unit ! { Bank.Pay } reversible latency 1.0
  requires s.binds("pairs", to, memo)
tool one(c: Cap[Bank.Pay], to: Text ^ Endorsed) -> Unit ! { Bank.Pay } reversible latency 1.0
  requires s.member("payees", to)
policy P(p: Plan) -> Bool = true
policy PO(o: Op) -> Bool = true
agent A(bank: Cap[Bank.Pay]) budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {
  on go(req: Text[..200] ^ Trusted, c: Bool ^ Trusted) -> Text ! { Infer, Bank.Pay } {
'''
TAIL = '''    "ok"
  }
}
'''
CASES = {
 # fallback: proposta nova, senão a anterior (deveria descarregar: o valor é um só)
 "m1_else_fallback_op": '''let a = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let b = refine (infer planner -> Op { request = req }) by PO else { a }
    match b { Pay { to, memo } => pay(bank, to, memo) }''',
 "m2_else_fallback_laco": '''let p = refine (infer planner -> Plan { request = req }) by P else { return "none" }
    let ops = match p { Plan { ops } => ops }
    for o in ops { let q = refine (infer planner -> Op { request = req }) by PO else { o }
      match q { Pay { to, memo } => pay(bank, to, memo) } }''',
 # dois refines, escolha por elemento (sólido)
 "m3_if_dois_refines": '''let a = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let b = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let v = if c { a } else { b }
    match v { Pay { to, memo } => pay(bank, to, memo) }''',
 # dois refines, um campo de cada via o mesmo ponto de escolha? (v e w do mesmo if não existem); mistura com v e a
 "m4_v_e_a": '''let a = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let b = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let v = if c { a } else { b }
    match v { Pay { to } => match a { Pay { memo } => pay(bank, to, memo) } }''',
 # campo ignorado com dois refines: a.to ou b.memo (sólido por span)
 "m5_campo_dois_refines": '''let a = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let b = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let x = if c { match a { Pay { to } => to } } else { match b { Pay { memo } => memo } }
    one(bank, x)''',
 # campo ignorado num span, com dois refines: a.to, a.memo ou b.to
 "m6_campo_misto": '''let a = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let b = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let x = if c { match a { Pay { to } => to } } else { if c { match b { Pay { to } => to } } else { match a { Pay { memo } => memo } } }
    one(bank, x)''',
 # refine-else com if dentro: else devolve a ou b; depois campos
 "m7_else_if": '''let a = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let b = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let z = refine (infer planner -> Op { request = req }) by PO else { if c { a } else { b } }
    match z { Pay { to, memo } => pay(bank, to, memo) }''',
 # dois ifs com o mesmo span? não; z vem de if que junta a/b e o campo de outro if com mesmas fontes
 "m8_dois_if_iguais": '''let a = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let b = refine (infer planner -> Op { request = req }) by PO else { return "none" }
    let v = if c { a } else { b }
    let w = if c { b } else { a }
    match v { Pay { to } => match w { Pay { memo } => pay(bank, to, memo) } }''',
}
for k, body in CASES.items():
    open(f"{k}.ath", "w").write(HEAD + "    " + body + "\n" + TAIL)
