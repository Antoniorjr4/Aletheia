import sys
HEAD = '''module t
type Op = | Pay { to: Text[..16], memo: Text[..16] }
type Row = | Row { ops: List[Op, 0..3] }
type Plan = | Plan { ops: List[Op, 0..3], rows: List[Row, 0..2] }
model planner { price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 4000  reasoning = 0  ttft = 1.0  per_tok = 0.01 }
tool pay(c: Cap[Bank.Pay], to: Text ^ Endorsed, memo: Text ^ Endorsed) -> Unit ! { Bank.Pay } reversible latency 1.0
  requires s.binds("pairs", to, memo)
policy P(p: Plan) -> Bool = true
agent A(bank: Cap[Bank.Pay]) budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {
  on go(req: Text[..200] ^ Trusted, c: Bool ^ Trusted) -> Text ! { Infer, Bank.Pay } {
    let d = infer planner -> Plan { request = req }
    let p = refine d by P else { return "none" }
    let ops = match p { Plan { ops } => ops }
    let rows = match p { Plan { rows } => rows }
'''
TAIL = '''    "ok"
  }
}
'''
CASES = {
 # controles: origem comum comprovável
 "k_if_mesmo_laco": "for a in ops { for b in ops { let o = if c { a } else { b }\n match o { Pay { to, memo } => pay(bank, to, memo) } } }",
 "k_aninhado": "for r in rows { match r { Row { ops } => for o in ops { match o { Pay { to, memo } => pay(bank, to, memo) } } } }",
 "k_reconstroi": "for o in ops { match o { Pay { to, memo } => { let o2 = Pay { to: to, memo: memo }\n match o2 { Pay { to, memo } => pay(bank, to, memo) } } } }",
 "k_lista_um": "for o in ops { for x in [o] { match x { Pay { to, memo } => pay(bank, to, memo) } } }",
 "k_lista_dois": "for a in ops { for b in ops { for x in [a, b] { match x { Pay { to, memo } => pay(bank, to, memo) } } } }",
 "k_match_duas_vezes": "for o in ops { match o { Pay { to } => match o { Pay { memo } => pay(bank, to, memo) } } }",
 "k_sombra_laco": "for o in ops { for o in ops { match o { Pay { to, memo } => pay(bank, to, memo) } } }",
 "k_alias_lista": "let l = ops\n for o in l { match o { Pay { to, memo } => pay(bank, to, memo) } }",
 "k_troca": "for o in ops { match o { Pay { to: memo, memo: to } => pay(bank, to, memo) } }",
 "k_if_campos_mesmo": "for o in ops { match o { Pay { to, memo } => { let x = if c { to } else { to }\n pay(bank, x, memo) } } }",
 # ataques
 "x_lista_cruzada": "for a in ops { for b in ops { for x in [a, b] { for y in [a, b] { match x { Pay { to } => match y { Pay { memo } => pay(bank, to, memo) } } } } } }",
 "x_ctor_cruzado": "for a in ops { for b in ops { match a { Pay { to } => match b { Pay { memo } => { let o = Pay { to: to, memo: memo }\n match o { Pay { to, memo } => pay(bank, to, memo) } } } } } }",
 "x_if_campos": "for a in ops { for b in ops { let x = if c { match a { Pay { to } => to } } else { match b { Pay { to } => to } }\n let y = if c { match b { Pay { memo } => memo } } else { match a { Pay { memo } => memo } }\n pay(bank, x, y) } }",
 "x_aninhado_cruzado": "for r in rows { for r2 in rows { match r { Row { ops } => for o in ops { match r2 { Row { ops } => for o2 in ops { match o { Pay { to } => match o2 { Pay { memo } => pay(bank, to, memo) } } } } } } } }",
 "x_aninhado_mesma_linha": "for r in rows { match r { Row { ops } => for o in ops { for o2 in ops { match o { Pay { to } => match o2 { Pay { memo } => pay(bank, to, memo) } } } } } }",
 "x_lista_de_listas": "for l in [ops, ops] { for l2 in [ops] { for a in l { for b in l2 { match a { Pay { to } => match b { Pay { memo } => pay(bank, to, memo) } } } } } }",
 "x_alias_let_laco": "for a in ops { let x = a\n for b in ops { let y = b\n match x { Pay { to } => match y { Pay { memo } => pay(bank, to, memo) } } } }",
 "x_bloco_sombra": "for a in ops { for b in ops { match a { Pay { to } => { let to2 = to\n match b { Pay { memo } => { let w = { let to = memo\n to }\n pay(bank, to2, w) } } } } } }",
}
for k, body in CASES.items():
    open(f"{k}.ath", "w").write(HEAD + "    " + body + "\n" + TAIL)
