import json
G = "type Group = | Group { ops: List[Op, 0..3], tag: Text[..16] }"
C = {
 "d2_ponto_depois_match": {"types": G, "plan": ", left: Group, right: Group",
   "body": "match p.left { Group { ops: o1 } => match p.right { Group { ops: o2 } => pay(bank, fit(join(o1.to, \"\"), 16), join(o2.memo, \"\")) } }"},
 "d3_ponto_len_duas": {"types": G, "plan": ", left: Group, right: Group",
   "body": "pay(bank, text(len(p.left.ops) + len(p.right.ops)), join(p.left.ops.memo, \"\"))"},
}
json.dump(C, open("c6.json","w"))
