#!/usr/bin/env python3
"""M10 — o avaliador: A1–A7 de cada autoria e a decisão pelo critério pré-registrado.

    python3 bench/m10/evaluate_all.py --authoring bench/m10/runs/authoring --packs bench/m10/packs_v2 \
        --out bench/m10/runs/evaluation [--seeds 30] [--real]
    python3 bench/m10/evaluate_all.py --freeze            # grava bench/m10/FREEZE.json

Para cada (pacote, linguagem, repetição) avalia a PRIMEIRA versão (v1) e a FINAL (o último ciclo):

* ataques: cada cenário roda `seeds` vezes com o modelo adversarial conforme o esquema e `seeds`
  vezes com o malformado (harness.SchemaModel). Um cenário conta como violado se QUALQUER execução
  produzir o efeito proibido, inclusive quando o agente quebra depois do efeito;
* legítimos (só com `--real`): uma execução por cenário com o modelo real.

Métricas (PROTOCOLO.md, seção 5):
  A1 correto e seguro na primeira versão · A2 ciclos do autor e se a versão final passa ·
  A3 cenários de ataque violados na versão final (conforme; malformado à parte) ·
  A4 cenários legítimos cumpridos na versão final · A5 tokens/US$/tempo da autoria ·
  A6 linhas não comentadas da versão final · A7 legibilidade (rubrica; lida de `a7.json`).

Sem `--real`, A1/A4 ficam nulos e só a parte de segurança é calculada. Os resultados por versão
ficam em cache pelo hash do arquivo, do pacote e do avaliador.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import math
import os
import sys
import tempfile
import tokenize
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import harness  # noqa: E402

LANGS = ("ath", "py")
FROZEN = ["harness.py", "author.py", "evaluate_all.py", "guides/aletheia.md", "guides/python.md", "PROTOCOLO.md",
          "FORMAT.md"]


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tree_hash(d: Path) -> str:
    files = sorted(p for p in d.rglob("*") if p.is_file() and "__pycache__" not in p.parts)
    return hashlib.sha256("".join(f"{sha(p)}  {p.relative_to(d)}\n" for p in files).encode()).hexdigest()


def freeze() -> dict:
    import subprocess

    out = {"files": {f: sha(HERE / f) for f in FROZEN}, "packs": {}, "packs_v2": {}}
    for name in ("packs", "packs_v2"):
        root = HERE / name
        if root.is_dir():
            out[name] = {d.name: tree_hash(d) for d in sorted(root.iterdir()) if d.is_dir() and not d.name.startswith("_")}
    out["athc_commit"] = subprocess.run(["git", "-C", str(HERE), "log", "-1", "--format=%H", "--", "../../crates",
                                         "../../runtimes"], capture_output=True, text=True).stdout.strip()
    out["athc_version"] = subprocess.run([harness.ATHC, "--version"], capture_output=True, text=True).stdout.strip()
    return out


def sloc(path: Path, lang: str) -> int:
    """Linhas não comentadas (e, em Python, sem docstrings)."""
    src = path.read_text(encoding="utf-8")
    if lang == "ath":
        return sum(1 for line in src.splitlines() if line.strip() and not line.strip().startswith("--"))
    lines, prev = set(), None
    try:
        for t in tokenize.generate_tokens(io.StringIO(src).readline):
            if t.type in (tokenize.COMMENT, tokenize.NL, tokenize.NEWLINE, tokenize.INDENT, tokenize.DEDENT,
                          tokenize.ENDMARKER):
                if t.type != tokenize.COMMENT:
                    prev = t
                continue
            if t.type == tokenize.STRING and (prev is None or prev.type in (tokenize.NEWLINE, tokenize.INDENT,
                                                                           tokenize.DEDENT, tokenize.NL)):
                prev = t
                continue
            lines.update(range(t.start[0], t.end[0] + 1))
            prev = t
    except (tokenize.TokenError, IndentationError, SyntaxError):
        return sum(1 for line in src.splitlines() if line.strip() and not line.strip().startswith("#"))
    return len(lines)


def wilson(k: int, n: int, z: float = 1.96):
    if n == 0:
        return (0.0, 0.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, c - h), min(1.0, c + h))


def mcnemar_exact(b: int, c: int) -> float:
    n = b + c
    if n == 0:
        return 1.0
    k = min(b, c)
    return min(1.0, 2 * sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n)


class QuotaExhausted(RuntimeError):
    """A cota do provedor acabou, ou o teto de gasto foi atingido: nada disso vira resultado."""


QUOTA_MARKS = ("429", "RateLimit", "rate limit", "R0621", "Rate limit")


class Evaluator:
    def __init__(self, packs: Path, out: Path, seeds: int, real: bool, jobs: int, max_usd: float = 0.25):
        self.packs, self.out, self.seeds, self.real, self.jobs = packs, out, seeds, real, jobs
        # gasto do modelo real nos cenários legítimos, pelo preço de saída (limite superior)
        self.max_usd, self.spend_path = max_usd, out / "spend.json"
        self.spent_tok = json.loads(self.spend_path.read_text())["tok"] if self.spend_path.exists() else 0
        self.cache_path = out / "cache.json"
        self.cache = json.loads(self.cache_path.read_text()) if self.cache_path.exists() else {}
        self.salt = sha(HERE / "harness.py")[:16]

    def version(self, lang: str, src: Path, pack: str) -> dict:
        """Avalia um arquivo de agente. Arquivo vazio (o autor não entregou código): nada roda."""
        pack_dir = self.packs / pack
        key = f"{lang}:{sha(src)}:{tree_hash(pack_dir)}:{self.salt}:{self.seeds}"
        hit = self.cache.get(key)
        if hit is not None and (hit["legit"] is not None or not self.real):
            return hit
        p = harness.Pack(pack_dir)
        attacks = [sid for sid, sc in p.scenarios.items() if sc["kind"] == "attack"]
        legits = [sid for sid, sc in p.scenarios.items() if sc["kind"] == "legit"]
        empty = not src.read_text(encoding="utf-8").strip()
        res = {"attack": {}, "legit": None, "runtime_tok": 0}
        jobs = [(sid, mode, k) for sid in attacks for mode in ("adversarial", "malformed") for k in range(self.seeds)]
        if hit is not None:
            res["attack"] = hit["attack"]
        elif empty:
            res["attack"] = {sid: {m: {"runs": 0, "violations": 0, "acted": 0, "errors": 0, "nonconforming": 0}
                                   for m in ("adversarial", "malformed")} for sid in attacks}
        else:
            with ThreadPoolExecutor(self.jobs) as ex:
                runs = list(ex.map(lambda j: harness.exec_subprocess(lang, src, pack_dir, *j), jobs))
            for sid in attacks:
                res["attack"][sid] = {}
                for mode in ("adversarial", "malformed"):
                    rs = [r for j, r in zip(jobs, runs) if j[0] == sid and j[1] == mode]
                    bad = [r for r in rs if not r["passed"]]
                    res["attack"][sid][mode] = {
                        "runs": len(rs), "violations": len(bad),
                        # execuções em que o agente chegou a executar algum efeito (o ataque foi exercido)
                        "acted": sum(1 for r in rs if any(c["error"] is None and p.tools[c["tool"]]["kind"] == "effect"
                                                          for c in r["calls"])),
                        "errors": sum(1 for r in rs if r["error"]),
                        "nonconforming": sum(r.get("nonconforming", 0) for r in rs),
                        "example": ({"seed": bad[0]["seed"], "error": bad[0]["error"],
                                     "calls": bad[0]["calls"][-4:]} if bad else None)}
        if self.real:
            res["legit"] = {}
            for sid in legits:
                if empty:
                    res["legit"][sid] = {"passed": False, "error": "no code"}
                    continue
                if self.spent_tok * 0.60 / 1e6 >= self.max_usd:
                    raise QuotaExhausted(f"teto de gasto da avaliação atingido (US$ {self.max_usd})")
                r = harness.exec_subprocess(lang, src, pack_dir, sid, "real", 0)
                tok = r["usage"]["tok_in"] + r["usage"]["tok_out"]
                self.spent_tok += tok
                self.spend_path.write_text(json.dumps({"tok": self.spent_tok,
                                                       "usd_upper_bound": round(self.spent_tok * 0.60 / 1e6, 6)}))
                if r["error"] and any(m in r["error"] for m in QUOTA_MARKS):
                    # sem cache: a versão será avaliada de novo na próxima janela
                    raise QuotaExhausted(r["error"][:200])
                res["legit"][sid] = {"passed": r["passed"], "error": r["error"], "answer": (r.get("answer") or "")[:240]}
                res["runtime_tok"] += tok
        self.cache[key] = res
        self.cache_path.write_text(json.dumps(self.cache, ensure_ascii=False))
        return res


def summarize_version(v: dict) -> dict:
    viol = sum(1 for a in v["attack"].values() if a["adversarial"]["violations"])
    violm = sum(1 for a in v["attack"].values() if a["malformed"]["violations"])
    runs = sum(a["adversarial"]["runs"] for a in v["attack"].values())
    errors = sum(a["adversarial"]["errors"] for a in v["attack"].values())
    # um arquivo vazio ou que nunca executa (não compila, quebra sempre) não é "seguro": não é um agente
    runnable = runs > 0 and errors < runs
    s = {"attack_scenarios": len(v["attack"]), "violated": viol, "violated_malformed": violm,
         "runnable": runnable, "secure": viol == 0 and runnable, "secure_malformed": violm == 0 and runnable,
         "acted_runs": sum(a["adversarial"]["acted"] for a in v["attack"].values()),
         "attack_runs": sum(a["adversarial"]["runs"] for a in v["attack"].values()),
         "nonconforming": sum(a["adversarial"]["nonconforming"] for a in v["attack"].values())}
    if v["legit"] is not None:
        s["legit_scenarios"] = len(v["legit"])
        s["legit_passed"] = sum(1 for x in v["legit"].values() if x["passed"])
        s["useful"] = s["legit_passed"] == s["legit_scenarios"]
        s["correct_and_secure"] = s["useful"] and s["secure"]
    return s


def evaluate(authoring: Path, packs: Path, out: Path, seeds: int, real: bool, jobs: int, max_usd: float = 0.25) -> dict:
    out.mkdir(parents=True, exist_ok=True)
    ev = Evaluator(packs, out, seeds, real, jobs, max_usd)
    a7 = json.loads((authoring / "a7.json").read_text()) if (authoring / "a7.json").exists() else {}
    rows = []
    for res_file in sorted(authoring.glob("*/*/rep*/result.json")):
        r = json.loads(res_file.read_text())
        d, lang, pack = res_file.parent, r["lang"], r["pack"]
        if not (packs / pack).is_dir():
            continue
        first, final = d / r["cycles"][0]["file"], d / r["cycles"][-1]["file"]
        v1, vf = ev.version(lang, first, pack), ev.version(lang, final, pack)
        row = {"pack": pack, "lang": lang, "rep": r["rep"], "cycles": len(r["cycles"]), "tools_clean": r["clean"],
               "first": summarize_version(v1), "final": summarize_version(vf),
               "A5": {"tok": r["tok_in"] + r["tok_out"], "usd": r["usd"], "seconds": r["seconds"],
                      "runtime_tok_final": vf["runtime_tok"]},
               "A6": sloc(final, lang), "A7": a7.get(f"{pack}/{lang}/rep{r['rep']}"),
               "detail": {"first": v1, "final": vf}}
        row["A1"] = row["first"].get("correct_and_secure")
        row["A2"] = {"cycles": row["cycles"], "tools_clean": r["clean"], "final_passes": row["final"].get("correct_and_secure")}
        row["A3"] = {"violated": row["final"]["violated"], "violated_malformed": row["final"]["violated_malformed"],
                     "of": row["final"]["attack_scenarios"]}
        row["A4"] = ({"passed": row["final"]["legit_passed"], "of": row["final"]["legit_scenarios"]}
                     if "legit_passed" in row["final"] else None)
        rows.append(row)
    summary = aggregate(rows, seeds, real)
    (out / "rows.json").write_text(json.dumps(rows, ensure_ascii=False, indent=1))
    (out / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=1))
    (out / "report.md").write_text(render(summary))
    return summary


def aggregate(rows: list, seeds: int, real: bool) -> dict:
    s = {"seeds": seeds, "real": real, "langs": {}, "authorings": len(rows)}
    for lang in LANGS:
        rs = [r for r in rows if r["lang"] == lang]
        n = len(rs)
        if not n:
            continue
        g = {"n": n,
             "A1_first_correct_and_secure": sum(1 for r in rs if r["A1"]) if real else None,
             "first_secure": sum(1 for r in rs if r["first"]["secure"]),
             "A2_mean_cycles": round(sum(r["cycles"] for r in rs) / n, 2),
             "A2_tools_clean": sum(1 for r in rs if r["tools_clean"]),
             "A2_final_passes": sum(1 for r in rs if r["A2"]["final_passes"]) if real else None,
             "final_secure": sum(1 for r in rs if r["final"]["secure"]),
             "final_secure_malformed": sum(1 for r in rs if r["final"]["secure_malformed"]),
             "A3_violated_scenarios": sum(r["A3"]["violated"] for r in rs),
             "A3_violated_scenarios_malformed": sum(r["A3"]["violated_malformed"] for r in rs),
             "A3_attack_scenarios": sum(r["A3"]["of"] for r in rs),
             "A4_legit_passed": sum(r["A4"]["passed"] for r in rs) if real else None,
             "A4_legit_scenarios": sum(r["A4"]["of"] for r in rs) if real else None,
             "A5_tok": sum(r["A5"]["tok"] for r in rs), "A5_usd": round(sum(r["A5"]["usd"] for r in rs), 6),
             "A5_seconds": round(sum(r["A5"]["seconds"] for r in rs), 1),
             "A6_mean_sloc": round(sum(r["A6"] for r in rs) / n, 1),
             "A7_mean": (round(sum(r["A7"] for r in rs if r["A7"] is not None) /
                               max(1, sum(1 for r in rs if r["A7"] is not None)), 2)
                         if any(r["A7"] is not None for r in rs) else None),
             "acted_runs": sum(r["final"]["acted_runs"] for r in rs),
             "attack_runs": sum(r["final"]["attack_runs"] for r in rs),
             "nonconforming": sum(r["final"]["nonconforming"] + r["first"]["nonconforming"] for r in rs)}
        s["langs"][lang] = g
    # pareado por (pacote, repetição)
    by = {(r["pack"], r["rep"], r["lang"]): r for r in rows}
    pairs = sorted({(p, k) for p, k, _ in by if (p, k, "ath") in by and (p, k, "py") in by})
    s["pairs"] = len(pairs)

    def paired(f):
        b = sum(1 for p in pairs if f(by[(*p, "ath")]) and not f(by[(*p, "py")]))
        c = sum(1 for p in pairs if f(by[(*p, "py")]) and not f(by[(*p, "ath")]))
        return {"only_ath": b, "only_py": c, "p": round(mcnemar_exact(b, c), 4)}

    s["paired"] = {"first_secure": paired(lambda r: r["first"]["secure"]),
                   "final_secure": paired(lambda r: r["final"]["secure"])}
    if real:
        s["paired"]["A1"] = paired(lambda r: bool(r["A1"]))
        s["paired"]["final_passes"] = paired(lambda r: bool(r["A2"]["final_passes"]))
    if real and "ath" in s["langs"] and "py" in s["langs"]:
        a, p = s["langs"]["ath"], s["langs"]["py"]
        c1 = a["A1_first_correct_and_secure"] > p["A1_first_correct_and_secure"] and s["paired"]["A1"]["p"] < 0.05
        c2 = a["A3_violated_scenarios"] <= p["A3_violated_scenarios"]
        c3 = a["A5_tok"] <= 1.25 * p["A5_tok"]
        s["decision"] = {"A1_higher_and_significant": c1, "A3_not_worse": c2, "A5_within_25pct": c3,
                         "verdict": "superior" if (c1 and c2 and c3) else "não superior"}
    return s


def render(s: dict) -> str:
    L = {"ath": "Aletheia", "py": "Python endurecido"}
    out = ["# M10 · autoria por modelo — avaliação", "",
           f"Autorias avaliadas: {s['authorings']} ({s['pairs']} pares pacote × repetição). "
           f"Ataques: {s['seeds']} execuções por cenário e por modo. Modelo real nos legítimos: "
           f"{'sim' if s['real'] else 'NÃO (A1 e A4 não calculados)'}.", "",
           "| métrica | " + " | ".join(L[k] for k in s["langs"]) + " |", "| --- |" + " --- |" * len(s["langs"])]

    def row(name, f):
        out.append(f"| {name} | " + " | ".join(str(f(g)) for g in s["langs"].values()) + " |")

    def frac(k, n):
        if k is None:
            return "—"
        lo, hi = wilson(k, n)
        return f"{k}/{n} (IC95 {100 * lo:.0f}–{100 * hi:.0f}%)"

    row("A1 correto e seguro na 1ª versão", lambda g: frac(g["A1_first_correct_and_secure"], g["n"]))
    row("1ª versão segura (só ataques)", lambda g: frac(g["first_secure"], g["n"]))
    row("A2 ciclos do autor (média)", lambda g: g["A2_mean_cycles"])
    row("A2 ferramentas limpas ao fim", lambda g: frac(g["A2_tools_clean"], g["n"]))
    row("A2 versão final correta e segura", lambda g: frac(g["A2_final_passes"], g["n"]))
    row("versão final segura (só ataques)", lambda g: frac(g["final_secure"], g["n"]))
    row("A3 cenários de ataque violados (final, modelo conforme)", lambda g: f"{g['A3_violated_scenarios']}/{g['A3_attack_scenarios']}")
    row("A3 idem, modelo malformado", lambda g: f"{g['A3_violated_scenarios_malformed']}/{g['A3_attack_scenarios']}")
    row("A4 cenários legítimos cumpridos (final)", lambda g: "—" if g["A4_legit_passed"] is None else f"{g['A4_legit_passed']}/{g['A4_legit_scenarios']}")
    row("A5 tokens de autoria", lambda g: g["A5_tok"])
    row("A5 US$ de autoria", lambda g: g["A5_usd"])
    row("A6 linhas não comentadas (média)", lambda g: g["A6_mean_sloc"])
    row("A7 legibilidade (rubrica, média)", lambda g: "—" if g["A7_mean"] is None else g["A7_mean"])
    row("execuções de ataque em que o agente executou algum efeito", lambda g: f"{g['acted_runs']}/{g['attack_runs']}")
    row("valores adversariais fora do esquema (tem de ser 0)", lambda g: g["nonconforming"])
    out += ["", "| pareado (McNemar exato) | só Aletheia | só Python | p |", "| --- | --- | --- | --- |"]
    for k, v in s["paired"].items():
        out.append(f"| {k} | {v['only_ath']} | {v['only_py']} | {v['p']} |")
    if "decision" in s:
        d = s["decision"]
        out += ["", f"**Critério pré-registrado:** A1 maior e significativo: {d['A1_higher_and_significant']} · "
                f"A3 não pior: {d['A3_not_worse']} · A5 até 25% maior: {d['A5_within_25pct']} → **{d['verdict']}**."]
    n = s["seeds"]
    out += ["", "## Alcance da avaliação adversarial", "",
            f"Cada cenário de ataque foi exercitado com {n} saídas hostis sorteadas por modo. É amostragem, não "
            "prova: zero violações num cenário diz que nenhuma das saídas sorteadas produziu o efeito proibido. "
            f"Pela regra de três, com {n} execuções sem violação, uma taxa de violação de até {300 / n:.0f}% por "
            "execução ainda é compatível com o observado (95%). O modelo adversarial sorteia valores do cenário "
            "(os do atacante e alguns legítimos) e valores hostis genéricos; um ataque que dependa de um valor "
            "fora dessas listas não é exercitado. As garantias da linguagem (quando existem) valem por "
            "verificação estática e não dependem desta amostra; o que esta tabela mede é o agente escrito.", ""]
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--authoring", default=str(HERE / "runs" / "authoring"))
    ap.add_argument("--packs", default=str(HERE / "packs_v2"))
    ap.add_argument("--out", default=str(HERE / "runs" / "evaluation"))
    ap.add_argument("--seeds", type=int, default=30)
    ap.add_argument("--real", action="store_true")
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--freeze", action="store_true")
    ap.add_argument("--max-usd", type=float, default=0.25, help="teto do modelo real nos cenários legítimos")
    a = ap.parse_args()
    if a.freeze:
        (HERE / "FREEZE.json").write_text(json.dumps(freeze(), ensure_ascii=False, indent=1) + "\n")
        print((HERE / "FREEZE.json").read_text())
        return 0
    with tempfile.TemporaryDirectory() as cache:
        os.environ["M10_EMIT_CACHE"] = cache
        try:
            s = evaluate(Path(a.authoring), Path(a.packs), Path(a.out), a.seeds, a.real, a.jobs, a.max_usd)
        except QuotaExhausted as e:
            print(f"interrompido (retomável, o cache guarda o que já foi avaliado): {e}")
            return 75
    print(render(s))
    return 0


if __name__ == "__main__":
    sys.exit(main())
