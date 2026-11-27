HEAD = '''module t
type Op = | Pay { to: Text[..16], memo: Text[..16] }
type Plan = | Plan { ops: List[Op, 0..3] }
model planner { price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 4000  reasoning = 0  ttft = 1.0  per_tok = 0.01 }
tool pay(c: Cap[Bank.Pay], to: Text[..16], memo: Text[..16]) -> Unit ! { Bank.Pay } reversible latency 1.0
  requires s.member("payees", to)
policy P(p: Plan) -> Bool = true
policy Q(o: Op) -> Bool = match o { Pay { to } => s.member("payees", to) }
agent A(bank: Cap[Bank.Pay]) budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {
  on go(req: Text[..200] ^ Trusted) -> Text ! { Infer, Bank.Pay } {
    let d0 = infer planner -> Plan { request = req, payees = s.inventory("payees") }
    let fora = match (refine d0 by P else { return "none" }) { Plan { ops } => ops }
    let d = infer planner -> Plan { request = req, payees = s.inventory("payees") }
'''
TAIL = '''    "ok"
  }
}
'''
C = {
 # `as` dentro de um bloco aninhado; uso dentro do bloco: recusados
 "b2_bloco_dentro": "{ let q = refine d by P keeping ops by Q as fora else { return \"none\" }\n for o in fora { match o { Pay { to, memo } => pay(bank, to, memo) } } }",
 # `as` dentro de um bloco; uso depois do bloco: a lista endossada de fora (controle, comprovável)
 "b2_bloco_depois": "{ let q = refine d by P keeping ops by Q as fora else { return \"none\" }\n 0 }\n for o in fora { match o { Pay { to, memo } => pay(bank, to, memo) } }",
 # `as` dentro de um braço de match
 "b2_braco": "match (refine d0 by P else { return \"none\" }) { Plan { ops } => { let q = refine d by P keeping ops by Q as fora else { return \"none\" }\n for o in fora { match o { Pay { to, memo } => pay(bank, to, memo) } } } }",
 # `as` em let cujo valor é um bloco com o refine na cauda: o checker não liga `fora`
 "b2_cauda_bloco": "let q = { refine d by P keeping ops by Q as fora else { return \"none\" } }\n for o in fora { match o { Pay { to, memo } => pay(bank, to, memo) } }",
 # `as` em let cujo valor é um if com refine: idem
 "b2_if": "let q = if true { refine d by P keeping ops by Q as fora else { return \"none\" } } else { return \"none\" }\n for o in fora { match o { Pay { to, memo } => pay(bank, to, memo) } }",
 # sombreamento pelo for
 "b2_for_sombra": "let q = refine d by P keeping ops by Q as recus else { return \"none\" }\n for fora in [recus] { for o in fora { match o { Pay { to, memo } => pay(bank, to, memo) } } }",
 # sombreamento pelo braço
 "b2_braco_sombra": "let q = refine d by P keeping ops by Q as recus else { return \"none\" }\n match Plan { ops: recus } { Plan { ops: fora } => for o in fora { match o { Pay { to, memo } => pay(bank, to, memo) } } }",
 # on failure liga um nome que sombreia
}
for k, b in C.items():
    open(k + ".ath", "w").write(HEAD + "    " + b + "\n" + TAIL)
