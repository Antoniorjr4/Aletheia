"""Testes de unidade do aletheia_runtime (a equivalência com o ARX está na suíte diferencial
em Rust, `crates/athc/tests/differential.rs`)."""

import os
import tempfile
import unittest

from aletheia_runtime import blake3, canon
from aletheia_runtime.journal import Journal
from aletheia_runtime.mask import COMPLETE, DEAD, decode_following, viable
from aletheia_runtime.wal import Wal, WalError
from aletheia_runtime.shape import check, proposal_from_json

ACTION = {
    "t": "adt",
    "name": "Action",
    "variants": [
        {"name": "Restart", "fields": [["deploy", {"t": "text", "max": 32}]]},
        {"name": "Scale", "fields": [["deploy", {"t": "text", "max": 32}], ["to", {"t": "int", "min": 2, "max": 64}]]},
    ],
}
PLAN = {"t": "list", "elem": ACTION, "min": 1, "max": 3}


class Blake3(unittest.TestCase):
    def test_vetores(self):
        self.assertEqual(blake3.hexdigest(b""), "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262")
        self.assertEqual(blake3.hexdigest(b"abc"), "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85")
        # entradas de vários chunks (árvore de pais), padrão dos vetores oficiais: byte i = i mod 251
        for n, h in [
            (1025, "d00278ae47eb27b34faecf67b4fe263f82d5412916c1ffd97c8cb7fb814b8444"),
            (2049, "5f4d72f40d7a5f82b15ca2b2e44b1de3c2ef86c426c95c1af0b6879522563030"),
            (65537, "7c99f9840a73dfcb6e5bfe4ff6d1558acab7e015640790c26411818bdbe17eca"),
        ]:
            self.assertEqual(blake3.hexdigest(bytes(i % 251 for i in range(n))), h, n)


class Canon(unittest.TestCase):
    def test_floats_como_o_ryu(self):
        for x, s in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (2.0, "2.0"),
            (131072.0, "131072.0"),
            (9.630000000000001e-05, "0.00009630000000000001"),
            (3.3098843550000003, "3.3098843550000003"),
            (1e-5, "0.00001"),
            (1e-6, "1e-6"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (9e15, "9000000000000000.0"),
            (1e16, "1e+16"),
            (1.2345e16, "1.2345e+16"),
            (1e20, "1e+20"),
            (1.5e300, "1.5e+300"),
            (5e-324, "5e-324"),
            (123.456, "123.456"),
        ]:
            self.assertEqual(canon.dump_float(x), s, x)

    def test_objetos_e_strings(self):
        self.assertEqual(canon.dumps({"b": 1, "a": [True, None, 1.5]}), '{"a":[true,null,1.5],"b":1}')
        self.assertEqual(canon.dumps("ç\n\"\x01"), '"ç\\n\\"\\u0001"')

    def test_round_do_rust(self):
        self.assertEqual(canon.f64_round(2.5), 3.0)
        self.assertEqual(canon.f64_round(3304.5), 3305.0)
        self.assertEqual(canon.f64_round(-2.5), -3.0)


class Formas(unittest.TestCase):
    def test_listas_e_refinamentos(self):
        self.assertIsNone(check(PLAN, {"value": [{"kind": "Restart", "deploy": "api"}]}))
        self.assertEqual(check(PLAN, {"value": []}), "`value` tem 0 elemento(s); o tipo admite de 1 a 3")
        self.assertEqual(
            check(PLAN, {"value": [{"kind": "Scale", "deploy": "a", "to": 0}]}),
            "`value[0].to` = 0 fora do refinamento [2..64]",
        )
        self.assertEqual(
            proposal_from_json(PLAN, [{"deploy": "api", "kind": "Restart"}]),
            '{"value":[{"kind":"Restart","deploy":"api"}]}',
        )


class Mascara(unittest.TestCase):
    def test_injecao_volta_para_o_tipo(self):
        shape = ACTION
        text, dev = decode_following(shape, '{"value":{"kind":"Delete","deploy":"db"}}', 400)
        self.assertEqual(dev[1], "D")
        self.assertEqual(dev[2], ["R", "S"])
        self.assertEqual(viable(shape, text), COMPLETE)
        text, _ = decode_following(shape, '{"value":{"kind":"Scale","deploy":"payments","to":0}}', 400)
        self.assertEqual(text, '{"value":{"kind":"Scale","deploy":"payments","to":10}}')

    def test_teto_da_lista(self):
        one = '{"kind":"Restart","deploy":"a"}'
        self.assertEqual(viable(PLAN, '{"value":[' + ",".join([one] * 3) + ","), DEAD)
        text, _ = decode_following(PLAN, '{"value":[' + ",".join([one] * 4) + "]}", 2000)
        self.assertEqual(text, '{"value":[' + ",".join([one] * 3) + "]}")


class JournalTest(unittest.TestCase):
    def test_cadeia_e_adulteracao(self):
        j = Journal()
        j.push(0.0, "start", {"x": 1})
        j.push(1.4, "tool", {"result": "restart api"})
        self.assertIsNone(Journal.verify(j.entries))
        j.entries[1]["detail"]["result"] = "restart payments"
        self.assertEqual(Journal.verify(j.entries), 1)


class WalTest(unittest.TestCase):
    """A leitura do WAL: ausência, erro de leitura, corrupção e cauda rasgada são casos
    distintos, e só a ausência é um log vazio (como `arx::wal`)."""

    def _sample(self) -> bytes:
        w = Wal()
        w.append(0.0, "intent", {"key": "k1", "tool": "pay"})
        w.append(1.0, "close", {"key": "k1", "status": "done"})
        w.append(2.0, "intent", {"key": "k2", "tool": "pay"})
        return w.to_jsonl().encode("utf-8")

    def test_ausente_e_log_novo(self):
        with tempfile.TemporaryDirectory() as d:
            w = Wal(os.path.join(d, "w.jsonl"))
            self.assertTrue(w.fresh)
            self.assertEqual(w.log.entries, [])

    def test_erro_de_leitura_nao_e_log_vazio(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "w.jsonl")
            os.mkdir(p)
            with self.assertRaises(WalError) as cm:
                Wal(p)
            self.assertEqual(cm.exception.code, "W0001")

    def test_corrupcao_no_meio_e_erro(self):
        data = self._sample()
        with self.assertRaises(WalError) as cm:
            Wal.read(data.replace(b'"done"', b'"failed"', 1))
        self.assertEqual(cm.exception.code, "W0002")
        self.assertIn("linha 2", str(cm.exception))
        lines = data.split(b"\n")
        with self.assertRaises(WalError):
            Wal.read(lines[0] + b"\n" + lines[2] + b"\n")
        with self.assertRaises(WalError):
            Wal.read(lines[0] + b"\n\xff\xfe\n" + lines[1] + b"\n")

    def test_cauda_rasgada_e_reabertura(self):
        data = self._sample()
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "w.jsonl")
            with open(p, "wb") as f:
                f.write(data[:-9])
            w = Wal(p)
            self.assertEqual(len(w.log.entries), 2)
            self.assertIsNotNone(w.torn)
            self.assertTrue(os.path.exists(p + ".torn"))
            w.append(3.0, "escalate", {"key": "k9"})
            again = Wal(p)
            self.assertEqual(len(again.log.entries), 3)
            self.assertIsNone(again.torn)
            self.assertIsNone(Journal.verify(again.log.entries))


if __name__ == "__main__":
    unittest.main()
