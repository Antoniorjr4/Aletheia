#!/usr/bin/env python3
"""Sondas da E0453 (28/11/2026): gera os programas `m*` (mistura, devem ser recusados), `u*`
(uma operação, devem ser aceitos) e `m10` (só agregação sobre a lista inteira: aceito, fora da
garantia). Rodar neste diretório: `python3 gen.py`."""
T = '''module t
type Op = | Pay {{ to: Text[..16], memo: Text[..16], n: Int[0..9] }}
type Ex = | Ex {{ memo: Text[..16] }}
type Plan = | Plan {{ ops: List[Op, 0..3], extra: List[Ex, 0..3], note: Text[..16] }}
model planner {{ price_in = 0.0  price_out = 0.0  ctx = 8192  max_out = 4000  reasoning = 0  ttft = 1.0  per_tok = 0.01 }}
tool pay(c: Cap[Bank.Pay], to: Text ^ Endorsed, memo: Text) -> Unit ! {{ Bank.Pay }} reversible latency 1.0
policy Q(o: Op) -> Bool = true
policy P(p: Plan) -> Bool = true
agent A(bank: Cap[Bank.Pay]) budget [tok <= 10000, usd <= 1.0, wall <= 100.0] {{
  on go(req: Text[..200] ^ Trusted, c: Bool ^ Trusted) -> Text ! {{ Infer, Bank.Pay }} {{
    let p = refine (infer planner -> Plan {{ request = req }}) by P keeping ops by Q as fora else {{ return "none" }}
    match p {{ Plan {{ ops, extra, note }} => {{
      {body}
      "ok" }} }}
  }}
}}
'''
C = {
 # devem ser MISTURA
 "m1_dois_lacos": "for a in ops { match a { Pay { to } => for b in ops { match b { Pay { memo } => pay(bank, to, memo) } } } }",
 "m2_variaveis": "for a in ops { let t = match a { Pay { to } => to }\n for b in ops { let m = match b { Pay { memo } => memo }\n let mm = m\n pay(bank, t, mm) } }",
 "m3_reconstrucao": "for a in ops { for b in ops { let r = match a { Pay { to } => match b { Pay { memo } => Pay { to: to, memo: memo, n: 0 } } }\n match r { Pay { to, memo } => pay(bank, to, memo) } } }",
 "m4_transformacao": "for a in ops { for b in ops { match a { Pay { to } => match b { Pay { memo } => pay(bank, to, fit(memo ++ \"x\", 10)) } } } }",
 "m5_juncao": "for a in ops { for b in ops { let x = if c { a } else { b }\n match a { Pay { to } => match x { Pay { memo } => pay(bank, to, memo) } } } }",
 "m6_elemento_e_lista": "for a in ops { match a { Pay { to } => pay(bank, to, join(ops.memo, \",\")) } }",
 "m7_elemento_e_raiz": "for a in ops { match a { Pay { to } => pay(bank, to, note) } }",
 "m8_duas_listas": "for a in ops { for e in extra { match a { Pay { to } => match e { Ex { memo } => pay(bank, to, memo) } } } }",
 "m9_quantificador": "for a in ops { match a { Pay { to } => pay(bank, to, text(sum w in ops => match w { Pay { n } => n })) } }",
 "m10_so_agregados": "pay(bank, fit(join(ops.to, \"\"), 16), join(ops.memo, \"\"))",
 "m11_lista_literal": "for a in ops { for b in ops { let l = [a, b]\n for x in l { match a { Pay { to } => match x { Pay { memo } => pay(bank, to, memo) } } } } }",
 "m12_campo_por_ponto": "for a in ops { for b in ops { pay(bank, a.to, b.memo) } }",
 # dois `refine` distintos combinados num argumento escalar (achado no tripé gerado, núcleo/99)
 "m13_dois_refines_escalar": "let q = refine (infer planner -> Plan { request = req }) by P else { return \"none\" }\n for a in ops { match a { Pay { to } => match q { Plan { note } => pay(bank, to, note) } } }",
 # o `if` escolhe o campo de elementos diferentes
 "m14_if_campo_de_dois": "for a in ops { for b in ops { match a { Pay { memo } => pay(bank, if c { a.to } else { b.to }, memo) } } }",
 # `refine … else { o }` com outro elemento
 "m15_else_elemento_e_outro": "for o in ops { for o2 in ops { let q = refine (infer planner -> Op { request = req }) by Q else { o }\n match q { Pay { to } => match o2 { Pay { memo } => pay(bank, to, memo) } } } }",
 # a lista inteira escolhida em `else` aninhado com um elemento dela
 "m16_else_aninhado_elemento_e_lista": "let r = refine (infer planner -> Plan { request = req }) by P else { refine (infer planner -> Plan { request = req }) by P else { return \"none\" } }\n match r { Plan { ops } => for a in ops { match a { Pay { to } => pay(bank, to, join(ops.memo, \",\")) } } }",
 # achados da revisão independente de 28/11 (fugas da 1ª versão)
 "m17_if_ramo_ja_misturado": "for a in ops { for b in ops { let x = if c { Pay { to: a.to, memo: b.memo, n: 0 } } else { a }\n match x { Pay { to, memo } => pay(bank, to, memo) } } }",
 "m18_if_ramo_escalar_misturado": "for a in ops { for b in ops { pay(bank, if c { a.to ++ b.to } else { a.to }, \"m\") } }",
 "m19_if_sem_outro_ramo_igual": "for a in ops { for b in ops { let x = if c { a.to ++ b.memo } else { b.to }\n pay(bank, x, \"m\") } }",
 "m20_if_em_if": "for a in ops { for b in ops { let x = if c { if c { a.to ++ b.memo } else { b.to } } else { a.to }\n pay(bank, x, \"m\") } }",
 "m21_for_lista_literal_reconstruida": "for a in ops { for b in ops { for x in [Pay { to: a.to, memo: b.memo, n: 0 }] { match x { Pay { to, memo } => pay(bank, to, memo) } } } }",
 "m22_for_lista_literal_escalar": "for a in ops { for b in ops { for t in [a.to ++ b.memo] { pay(bank, t, \"m\") } } }",
 "m23_refine_else_misturado": "for a in ops { for b in ops { let q = refine (infer planner -> Op { request = req }) by Q else { Pay { to: a.to, memo: b.memo, n: 0 } }\n match q { Pay { to, memo } => pay(bank, to, memo) } } }",
 "m24_atomic": "for a in ops { for b in ops { let r = atomic recover c { b.memo }\n pay(bank, a.to, r) } }",
 "m25_atomic_on_failure": "for a in ops { for b in ops { let r = atomic recover c { a.memo } on failure(e) { b.memo }\n pay(bank, a.to, r) } }",
 "m26_ponto_duas_listas_agregadas": "pay(bank, fit(join(p.ops.to, \"\"), 16), join(p.extra.memo, \"\"))",
 "m27_reconstrucao_com_lista": "let g = Plan { ops: ops, extra: extra, note: note }\n match g { Plan { ops } => for o1 in ops { for o2 in ops { match o1 { Pay { to } => match o2 { Pay { memo } => pay(bank, to, memo) } } } } }",
 # devem ser UMA (ou transformada)
 "u1_mesmo_elemento": "for a in ops { match a { Pay { to, memo } => pay(bank, to, memo) } }",
 "u2_literal_e_confiavel": "for a in ops { match a { Pay { to } => pay(bank, to, \"pedido \" ++ req) } }",
 "u3_transformada": "for a in ops { match a { Pay { to, memo } => pay(bank, to, fit(memo ++ \"!\", 12)) } }",
 "u4_alias_e_match_duplo": "for a in ops { let x = a\n match a { Pay { to } => match x { Pay { memo } => pay(bank, to, memo) } } }",
 "u5_if_mesmo": "for a in ops { let x = if c { a } else { a }\n match x { Pay { to, memo } => pay(bank, to, memo) } }",
 "u6_lista_literal_um": "for a in ops { for b in ops { for x in [a, b] { match x { Pay { to, memo } => pay(bank, to, memo) } } } }",
 "u7_ponto_mesmo": "for a in ops { pay(bank, a.to, a.memo) }",
 # `refine A else { refine B }`: o valor vem de A ou de B, nunca dos dois (falso positivo corrigido
 # em 28/11, achado em `differential.rs`, composed/else-aninhado)
 # `refine … else { o }`: a nova operação ou o elemento, nunca os dois (revisao2/m2)
 "u9_else_elemento": "for o in ops { let q = refine (infer planner -> Op { request = req }) by Q else { o }\n match q { Pay { to, memo } => pay(bank, to, memo) } }",
 # o `if` escolhe entre o campo e o campo transformado do mesmo elemento (revisao3/g02d)
 "u10_if_transformado_mesmo": "for a in ops { match a { Pay { to, memo } => pay(bank, if c { to } else { to ++ \"\" }, memo) } }",
 # o `if` escolhe entre dois campos do mesmo elemento
 "u11_if_dois_campos_mesmo": "for a in ops { match a { Pay { memo } => pay(bank, if c { a.to } else { a.memo }, memo) } }",
 "u12_atomic_mesmo": "for a in ops { let r = atomic recover c { a.memo }\n pay(bank, a.to, r) }",
 "u13_reconstrucao_com_lista": "let g = Plan { ops: ops, extra: extra, note: note }\n match g { Plan { ops } => for o in ops { match o { Pay { to, memo } => pay(bank, to, memo) } } }",
 "u8_else_aninhado_agregado": "let r = refine (infer planner -> Plan { request = req }) by P else { refine (infer planner -> Plan { request = req }) by P else { return \"none\" } }\n match r { Plan { ops } => pay(bank, \"r\", join(ops.memo, \",\") ++ text(len(ops))) }",

 # limites declarados: aceitos, fora da garantia (dependência de controle)
 "l1_valor_por_controle": "for a in ops { for b in ops { pay(bank, a.to, if b.memo == \"z\" { \"z\" } else { a.memo }) } }",
 "l2_condicao_da_chamada": "for a in ops { for b in ops { if b.n > 3 { pay(bank, a.to, a.memo) } } }",
 # recusas conservadoras de programas legítimos (declaradas)
 "c1_dois_ifs_mesma_condicao": "for a in ops { for b in ops { let t = if c { a.to } else { b.to }\n let m = if c { a.memo } else { b.memo }\n pay(bank, t, m) } }",
 "c2_elemento_e_contagem": "for a in ops { pay(bank, a.to, a.memo ++ text(len(ops))) }",
 "c3_lista_literal_de_um": "for a in ops { for x in [a] { pay(bank, x.to, a.memo) } }",
}
for k, b in C.items():
    open(f"{k}.ath", "w").write(T.format(body=b))
