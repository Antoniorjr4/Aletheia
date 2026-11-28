import json
G = "type Group = | Group { ops: List[Op, 0..3], tag: Text[..16] }"
C = {
 # duas listas distintas, alcancadas por ponto atraves de um registro aninhado
 "d1_ponto_aninhado_duas_listas": {"types": G, "plan": ", left: Group, right: Group",
   "body": "pay(bank, fit(join(p.left.ops.to, \"\"), 16), join(p.right.ops.memo, \"\"))"},
 "d1c_match_aninhado_duas_listas": {"types": G, "plan": ", left: Group, right: Group",
   "body": "match p { Plan { left, right } => match left { Group { ops } => match right { Group { ops: o2 } => pay(bank, fit(join(ops.to, \"\"), 16), join(o2.memo, \"\")) } } }"},
 "d1d_ponto_aninhado_lista_e_tag": {"types": G, "plan": ", left: Group, right: Group",
   "body": "pay(bank, fit(join(p.left.ops.to, \"\"), 16), p.right.tag)"},
 "d1e_ponto_aninhado_elem_e_lista": {"types": G, "plan": ", left: Group, right: Group",
   "body": "for a in p.left.ops { pay(bank, a.to, join(p.right.ops.memo, \"\")) }"},
 "d1f_mesmo_grupo_ponto": {"types": G, "plan": ", left: Group, right: Group",
   "body": "pay(bank, fit(join(p.left.ops.to, \"\"), 16), join(p.left.ops.memo, \"\"))"},
 "d1g_ponto_e_match_mesma_lista": {"types": G, "plan": ", left: Group, right: Group",
   "body": "match p { Plan { left } => match left { Group { ops } => pay(bank, fit(join(ops.to, \"\"), 16), join(p.left.ops.memo, \"\")) } }"},
 # FP: elemento com sublista
 "fp_sublista_do_elemento": {"full": None},
 # FP: reconstrucao e agregacao so de ops
 "fp_reconstrucao_agregado": "let g = Plan { ops: ops, extra: extra, note: note }\n match g { Plan { ops } => pay(bank, fit(join(ops.to, \"\"), 16), \"k\") }",
 # colisao de nomes: campo `memo` e lista numa variante, texto noutra
}
json.dump({k:v for k,v in C.items() if not (isinstance(v,dict) and "full" in v)}, open("c2.json","w"))
