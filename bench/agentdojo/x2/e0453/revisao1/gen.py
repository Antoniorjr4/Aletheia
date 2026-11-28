import sys
T = '''module t
type Op = | Pay {{ to: Text[..16], memo: Text[..16], n: Int[0..9] }}
type Ex = | Ex {{ memo: Text[..16] }}
type G = | G {{ xs: List[Op, 0..3], ys: List[Op, 0..3] }}
type Plan = | Plan {{ ops: List[Op, 0..3], extra: List[Ex, 0..3], note: Text[..16], gs: List[G, 0..2], one: List[Op, 0..1] }}
model planner {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 4000  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
tool pay(c: Cap[Bank.Pay], to: Text ^ Endorsed, memo: Text) -> Unit ! {{ Bank.Pay }} reversible latency 1.0
tool echo(c: Cap[Bank.Pay], x: Text) -> Text ! {{ Bank.Pay }} reversible latency 1.0
policy Q(o: Op) -> Bool = true
policy P(p: Plan) -> Bool = true
agent A(bank: Cap[Bank.Pay]) budget [tok <= 100000, usd <= 10.0, wall <= 10000.0] {{
  on go(req: Text[..200] ^ Trusted, c: Bool ^ Trusted) -> Text ! {{ Infer, Bank.Pay }} {{
    let p = refine (infer planner -> Plan {{ request = req }}) by P keeping ops by Q as fora else {{ return "none" }}
    match p {{ Plan {{ ops, extra, note, gs, one }} => {{
      {body}
      "ok" }} }}
  }}
}}
'''
AB = "for a in ops {{ for b in ops {{ {} }} }}"
C = {
 # fugas candidatas
 "f01_if_ramo_misturado": AB.format("let x = if c { Pay { to: a.to, memo: b.memo, n: 0 } } else { a }\n match x { Pay { to, memo } => pay(bank, to, memo) }"),
 "f02_if_ramo_misturado_escalar": AB.format("pay(bank, if c { a.to ++ b.to } else { a.to }, \"m\")"),
 "f03_for_lista_literal_ctor": AB.format("for x in [Pay { to: a.to, memo: b.memo, n: 0 }] { match x { Pay { to, memo } => pay(bank, to, memo) } }"),
 "f04_for_lista_literal_escalar": AB.format("for t in [a.to ++ b.memo] { pay(bank, t, \"m\") }"),
 "f05_refine_else_misturado": AB.format("let q = refine (infer planner -> Op { request = req }) by Q else { Pay { to: a.to, memo: b.memo, n: 0 } }\n match q { Pay { to, memo } => pay(bank, to, memo) }"),
 "f06_atomic": AB.format("let r = atomic recover c { a.memo ++ b.memo }\n pay(bank, a.to, r)"),
 "f07_atomic_ok": AB.format("let r = atomic recover c { b.memo }\n pay(bank, a.to, r)"),
 "f08_echo_ferramenta": AB.format("let r = echo(bank, b.memo)\n pay(bank, a.to, r)"),
 "f09_infer_lavagem": AB.format("let d = infer planner -> Ex { request = b.memo }\n match d { Ex { memo } => pay(bank, a.to, memo) }"),
 "f10_controle_if": AB.format("pay(bank, a.to, if b.memo == \"z\" { \"z\" } else { a.memo })"),
 "f11_controle_cond_chamada": AB.format("if b.n > 3 { pay(bank, a.to, a.memo) }"),
 "f12_fora": "for a in ops { for r in fora { match r { Pay { memo } => pay(bank, a.to, memo) } } }",
 "f13_ponto_duas_listas_agregadas": "pay(bank, fit(join(p.ops.to, \"\"), 16), join(p.extra.memo, \"\"))",
 "f13b_match_duas_listas_agregadas": "pay(bank, fit(join(ops.to, \"\"), 16), join(extra.memo, \"\"))",
 "f14_g_xs_ys": "for g in gs { pay(bank, fit(join(g.xs.to, \"\"), 16), join(g.ys.memo, \"\")) }",
 "f15_match_join_dois_ramos": AB.format("let x = match a { Pay { to } => to ++ b.memo }\n pay(bank, x, \"m\")"),
 "f16_lista_de_listas": AB.format("for l in [[a, b]] { for x in l { match x { Pay { to, memo } => pay(bank, to, memo) } } }"),
 "f17_lista_de_listas_duas": AB.format("for l in [[a, b]] { for x in l { for y in l { pay(bank, x.to, y.memo) } } }"),
 "f18_quant_ramo_misturado": AB.format("pay(bank, a.to, text(sum w in [b] => w.n))"),
 "f19_if_em_if": AB.format("let x = if c { if c { a.to ++ b.memo } else { b.to } } else { a.to }\n pay(bank, x, \"m\")"),
 "f20_bloco_retorno": AB.format("let x = { let t = a.to\n t ++ b.memo }\n pay(bank, x, \"m\")"),
 "f21_sombreamento": AB.format("let t = a.to\n let a = b\n pay(bank, t, a.memo)"),
 "f22_metodo": AB.format("pay(bank, a.to, b.memo.trim())"),
 "f23_refine_no_laco_combinado": AB.format("let q = refine (Pay { to: a.to, memo: b.memo, n: 0 }) by Q else { return \"x\" }\n match q { Pay { to, memo } => pay(bank, to, memo) }"),
 # falsos positivos candidatos
 "p01_dois_ifs_mesma_cond": AB.format("let t = if c { a.to } else { b.to }\n let m = if c { a.memo } else { b.memo }\n pay(bank, t, m)"),
 "p02_elemento_e_len": "for a in ops { pay(bank, a.to, a.memo ++ text(len(ops))) }",
 "p03_elemento_e_len_lista_de_1": "for a in one { pay(bank, a.to, a.memo ++ text(len(one))) }",
 "p04_elemento_e_nota_do_plano": "for a in ops { pay(bank, a.to, note) }",
 "p05_for_literal_um": "for a in ops { for x in [a] { pay(bank, x.to, a.memo) } }",
 "p06_refine_e_elemento_mesmo": "for a in ops { let q = refine (infer planner -> Op { request = a.memo }) by Q else { a }\n match q { Pay { to } => pay(bank, to, a.memo) } }",
 "p07_if_mesmo_ou_literal": "for a in ops { let t = if c { a.to } else { a.to ++ \"\" }\n pay(bank, t, a.memo) }",
 "p08_if_dois_campos_e_campo": "for a in ops { let t = if c { a.to } else { a.memo }\n pay(bank, t, a.memo) }",
 "p09_if_elemento_ou_raiz_depois_elemento": "for a in ops { let t = if c { a.to } else { note }\n pay(bank, t, a.memo) }",
 "p10_count_aggregate_only_one": "pay(bank, \"x\", text(len(ops)) ++ join(ops.memo, \",\"))",
 "p11_atomic_mesmo": "for a in ops { let r = atomic recover c { a.memo }\n pay(bank, a.to, r) }",
 "p12_dois_matches_ponto": "for a in ops { let x = match a { Pay { to } => to }\n let y = match a { Pay { memo } => memo }\n pay(bank, x, y) }",
}
for k, b in C.items():
    open(f"{k}.ath", "w").write(T.format(body=b))
