"""Os oráculos de um conjunto de pacotes percebem efeito duplicado? Aplica as chamadas de referência
de cada cenário legítimo uma vez e duas vezes; com duas, ou o verificador legítimo recusa ou um
verificador de ataque dispara. Uso: python3 dup_oracle.py <dir-de-pacotes>"""
import json
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
import harness as h

root = Path(sys.argv[1])
blind = []
for d in sorted(root.iterdir()):
    if not (d / "spec.json").exists():
        continue
    pack = h.Pack(d)
    attacks = {s["check"] for s in pack.scenarios.values() if s["kind"] == "attack"}
    for sid, sc in pack.scenarios.items():
        if sc["kind"] != "legit" or not sc.get("golden"):
            continue
        def run(times):
            world = pack.world(sc)
            tools = h.Tools(pack, world)
            for _ in range(times):
                for c in sc["golden"]:
                    tools.call(c["tool"], **c["args"])
            ok = getattr(pack.mod, sc["check"])(world, tools.calls, sc.get("golden_answer", ""))
            fired = [a for a in sorted(attacks) if getattr(pack.mod, a)(world, tools.calls, "")]
            state = json.dumps({k: v for k, v in world.s.items()}, sort_keys=True, default=str)
            return ok, fired, state, sum(1 for c in tools.calls if c["error"] is None)
        one, two = run(1), run(2)
        caught = (not two[0]) or bool(two[1])
        # efeito duplicado que mudou o estado do mundo (não é repetição idempotente nem erro)
        changed = two[2] != one[2]
        tag = "pega" if caught else ("NÃO PEGA (estado mudou)" if changed else "idempotente")
        print(f"{d.name:14} {sid:28} sucesso x1={one[3]} x2={two[3]}  {tag}")
        if not caught and changed:
            blind.append(f"{d.name}/{sid}")
print("duplicação que muda o mundo e não é percebida:", blind or "nenhuma")
