CASES.update({
 # ---- ataques
 # campo ignorado: um parâmetro com duas fontes de campos diferentes do mesmo elemento
 "y1_campo_if_xx": "for o in ops { match o { Pay { to, memo } => { let x = if c { to } else { memo }\n pay(bank, x, x) } } }",
 "y2_campo_if_um": "for o in ops { match o { Pay { to, memo } => one(bank, if c { to } else { memo }) } }",
 
 "y4_campo_ctor": "for o in ops { match o { Pay { to, memo } => { let o2 = Pay { to: if c { to } else { memo }, memo: memo }\n match o2 { Pay { to } => one(bank, to) } } } }",
 "y5_campo_dois_elem": "for a in ops { for b in ops { let x = if c { match a { Pay { to } => to } } else { match b { Pay { memo } => memo } }\n one(bank, x) } }",
 # last-id: escolha interna esconde diferença externa
 "z1_k_interno": "for a in ops { for b in ops { let v = if c { a } else { b }\n for w in [v, a] { match w { Pay { to } => match v { Pay { memo } => pay(bank, to, memo) } } } } }",
 "z2_match_v_duas": "for a in ops { for b in ops { let v = if c { a } else { b }\n match v { Pay { to } => match v { Pay { memo } => pay(bank, to, memo) } } } }",
 "z3_dois_if_mesmo_c": "for a in ops { for b in ops { let v = if c { a } else { b }\n let w = if c { b } else { a }\n match v { Pay { to } => match w { Pay { memo } => pay(bank, to, memo) } } } }",
 "z4_if_de_if": "for a in ops { for b in ops { let v = if c { a } else { b }\n let w = if c { v } else { a }\n match w { Pay { to } => match v { Pay { memo } => pay(bank, to, memo) } } } }",
 "z5_lista_de_listas": "for l in grid { for l2 in grid { let m = if c { l } else { l2 }\n for a in m { for b in m { match a { Pay { to } => match b { Pay { memo } => pay(bank, to, memo) } } } } } }",
 "z6_lista_lit_listas": "for l in [ops, ops] { for a in l { for b in l { match a { Pay { to } => match b { Pay { memo } => pay(bank, to, memo) } } } } }",
 "z7_pair_join": "for a in ops { for b in ops { let pr = if c { Pair { a: a, b: b } } else { Pair { a: b, b: a } }\n match pr { Pair { a: x, b: y } => match x { Pay { to } => match y { Pay { memo } => pay(bank, to, memo) } } } } }",
 "z8_join_unknown_variante": "for a in ops { let v = if c { a } else { Pay { to: \"evil\", memo: \"evil\" } }\n match v { Pay { to, memo } => pay(bank, to, memo) } }",
 "z9_sombra_join": "for a in ops { for b in ops { let v = if c { a } else { b }\n let v = if c { b } else { a }\n match v { Pay { to } => { let v = a\n match v { Pay { memo } => pay(bank, to, memo) } } } } }",
 "z10_lista_join_iter": "for a in ops { for b in ops { let l = if c { [a, b] } else { [b] }\n for x in l { for y in l { match x { Pay { to } => match y { Pay { memo } => pay(bank, to, memo) } } } } } }",
 "z11_if_dentro_laco_campos": "for a in ops { for b in ops { let t = if c { match a { Pay { to } => to } } else { match b { Pay { to } => to } }\n let m = if c { match a { Pay { memo } => memo } } else { match b { Pay { memo } => memo } }\n pay(bank, t, m) } }",
 "z12_rows_join": "for r in rows { for r2 in rows { let rr = if c { r } else { r2 }\n match rr { Row { ops } => for o in ops { match r { Row { ops } => for o2 in ops { match o { Pay { to } => match o2 { Pay { memo } => pay(bank, to, memo) } } } } } } } }",
 "z13_lista_join_mesmo_laco": "for a in ops { for b in ops { for x in [a, b] { let v = if c { x } else { a }\n match v { Pay { to } => match x { Pay { memo } => pay(bank, to, memo) } } } } }",
 # ---- controles: devem ficar descarregados (e corretos)
 "k1_if_mesmo_laco": "for a in ops { for b in ops { let o = if c { a } else { b }\n match o { Pay { to, memo } => pay(bank, to, memo) } } }",
 "k2_ops_join_vazio": "let l = if c { ops } else { [] }\n for o in l { match o { Pay { to, memo } => pay(bank, to, memo) } }",
 "k3_match_campo_unico": "for a in ops { for b in ops { let o = if c { a } else { b }\n let t = match o { Pay { to } => to }\n one(bank, t) } }",
 # sobre-conservadorismo: deveriam descarregar
 "o1_match_um_braco_join": "for a in ops { for b in ops { let o = if c { a } else { b }\n let t = match o { Pay { to } => to }\n let m = match o { Pay { memo } => memo }\n pay(bank, t, m) } }",
 "o2_reconstroi_join": "for o in ops { match o { Pay { to, memo } => { let o2 = if c { o } else { Pay { to: to, memo: memo } }\n match o2 { Pay { to, memo } => pay(bank, to, memo) } } } }",
 "o3_campo_ponto": "for o in ops { pay(bank, o.to, o.memo) }",
 "o4_let_campos": "for o in ops { let t = match o { Pay { to } => to }\n let m = match o { Pay { memo } => memo }\n pay(bank, t, m) }",
 "o5_if_mesmo_campo_multi": "let l = if c { ops } else { match p { Plan { rows } => [] } }\n for o in l { match o { Pay { to, memo } => { let t = if c { to } else { to }\n pay(bank, t, memo) } } }",
})
