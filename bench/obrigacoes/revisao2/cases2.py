CASES.update({
 "k4_grade": "for l in grid { for o in l { match o { Pay { to, memo } => pay(bank, to, memo) } } }",
 "k5_grade_join": "for l in grid { for l2 in grid { let m = if c { l } else { l2 }\n for o in m { match o { Pay { to, memo } => pay(bank, to, memo) } } } }",
 "k6_join_listas_mistas": "for r in rows { match r { Row { ops: rops } => { let l = if c { ops } else { rops }\n for o in l { match o { Pay { to, memo } => pay(bank, to, memo) } } } } }",
 "k7_match_dentro_if": "for o in ops { if c { match o { Pay { to, memo } => pay(bank, to, memo) } } else { \"x\" } }",
 # if dentro do laço que escolhe entre elemento atual e lista literal com o mesmo elemento
 "k8_lista_lit_join": "for a in ops { for b in ops { for x in (if c { [a] } else { [b, a] }) { match x { Pay { to, memo } => pay(bank, to, memo) } } } }",
 # ataques extra
 "z14_grade_cruzada_join": "for l in grid { for l2 in grid { let m = if c { l } else { l2 }\n for a in m { match a { Pay { to } => for b in l { match b { Pay { memo } => pay(bank, to, memo) } } } } } }",
 "z15_join_mesmo_if_dois_lados": "for a in ops { for b in ops { let v = if c { a } else { b }\n for x in [v] { for y in [v] { match x { Pay { to } => match y { Pay { memo } => pay(bank, to, memo) } } } } } }",
 "z16_sombra_var_laco": "for a in ops { for b in ops { let v = if c { a } else { b }\n match v { Pay { to } => { let v = if c { b } else { a }\n match v { Pay { memo } => pay(bank, to, memo) } } } } }",
})
