"""Testes offline da reserva de saldo: nenhum teste faz requisição de rede."""

import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "bench" / "orcamento"))
from reserva import (ChamadaProtegida, HipoteseViolada, Orcamento, Precos, SemSaldo,  # noqa: E402
                     cota_entrada)

GROQ = Precos(0.15, 0.60)  # os preços do M10/M11


class RateLimitError(Exception):
    pass


class ClienteFalso:
    """Imita client.chat.completions.create; registra cada envio e nunca toca a rede."""

    def __init__(self, respostas):
        self.respostas, self.envios, self.retries = list(respostas), [], None
        self.chat = SimpleNamespace(completions=SimpleNamespace(create=self._create))

    def with_options(self, max_retries):
        self.retries = max_retries
        return self

    def _create(self, **kw):
        self.envios.append(kw)
        r = self.respostas.pop(0)
        if isinstance(r, Exception):
            raise r
        tin, tout = r
        return SimpleNamespace(usage=SimpleNamespace(prompt_tokens=tin, completion_tokens=tout))


MSG = [{"role": "user", "content": "x" * 1000}]


class Reserva(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.livro = Path(self.dir.name) / "gasto.jsonl"

    def tearDown(self):
        self.dir.cleanup()

    def test_teto_zero_nao_envia_nada(self):
        c = ClienteFalso([(10, 10)])
        p = ChamadaProtegida(c, Orcamento(self.livro, 0.0, GROQ))
        with self.assertRaises(SemSaldo):
            p.criar(model="m", messages=MSG, max_completion_tokens=100)
        self.assertEqual(c.envios, [])

    def test_recusa_antes_de_enviar_quando_o_pior_caso_nao_cabe(self):
        # pior caso: (1000 + folgas) × 0,15 + 7000 × 0,60 ≈ US$ 0,00435
        orc = Orcamento(self.livro, 0.004, GROQ)
        c = ClienteFalso([(10, 10)])
        with self.assertRaises(SemSaldo):
            ChamadaProtegida(c, orc).criar(model="m", messages=MSG, max_completion_tokens=7000)
        self.assertEqual(c.envios, [])
        self.assertEqual(orc.gasto(), 0.0)

    def test_liquida_pelo_uso_e_libera_o_saldo(self):
        orc = Orcamento(self.livro, 0.01, GROQ)
        c = ClienteFalso([(1000, 500), (1000, 500)])
        p = ChamadaProtegida(c, orc)
        p.criar(model="m", messages=MSG, max_completion_tokens=7000)
        p.criar(model="m", messages=MSG, max_completion_tokens=7000)
        self.assertAlmostEqual(orc.gasto(), 2 * GROQ.usd(1000, 500))
        self.assertEqual(len(c.envios), 2)

    def test_tentativas_do_cliente_desligadas_e_cada_tentativa_reserva(self):
        orc = Orcamento(self.livro, 1.0, GROQ)
        c = ClienteFalso([RateLimitError("429"), (1000, 500)])
        r = ChamadaProtegida(c, orc, dormir=lambda s: None).criar(model="m", messages=MSG, max_completion_tokens=7000)
        self.assertIsNotNone(r)
        self.assertEqual(c.retries, 0)
        linhas = [json.loads(x) for x in self.livro.read_text().splitlines()]
        self.assertEqual([x["tipo"] for x in linhas], ["reserva", "liquidacao", "reserva", "liquidacao"])
        pior = GROQ.usd(cota_entrada(MSG), 7000)
        # a tentativa que falhou conta pelo pior caso: o provedor pode ter cobrado
        self.assertAlmostEqual(orc.gasto(), pior + GROQ.usd(1000, 500))

    def test_tentativa_extra_sem_saldo_nao_e_enviada(self):
        pior = GROQ.usd(cota_entrada(MSG), 7000)
        orc = Orcamento(self.livro, pior * 1.5, GROQ)  # cabe uma tentativa, não duas
        c = ClienteFalso([RateLimitError("429"), (1000, 500)])
        with self.assertRaises(SemSaldo):
            ChamadaProtegida(c, orc, dormir=lambda s: None).criar(model="m", messages=MSG, max_completion_tokens=7000)
        self.assertEqual(len(c.envios), 1)
        self.assertLessEqual(orc.gasto(), orc.teto)

    def test_reserva_aberta_de_processo_morto_conta_como_gasta(self):
        orc = Orcamento(self.livro, 0.01, GROQ)
        orc.reservar(1000, 7000)  # o processo "morre" aqui, sem liquidar
        reaberto = Orcamento(self.livro, 0.01, GROQ)
        self.assertAlmostEqual(reaberto.gasto(), GROQ.usd(1000, 7000))

    def test_sem_max_completion_tokens_nao_ha_pior_caso(self):
        with self.assertRaises(ValueError):
            ChamadaProtegida(ClienteFalso([]), Orcamento(self.livro, 1.0, GROQ)).criar(model="m", messages=MSG)

    def test_uso_acima_da_reserva_bloqueia_as_seguintes(self):
        orc = Orcamento(self.livro, 1.0, GROQ)
        c = ClienteFalso([(1000, 9000), (10, 10)])
        p = ChamadaProtegida(c, orc)
        with self.assertRaises(HipoteseViolada):
            p.criar(model="m", messages=MSG, max_completion_tokens=7000)
        with self.assertRaises(HipoteseViolada):
            p.criar(model="m", messages=MSG, max_completion_tokens=7000)
        self.assertEqual(len(c.envios), 1)

    def test_historico_real_do_m11_nao_passa_do_teto(self):
        """Reaplica as 123 chamadas registradas da autoria do M11 com reserva (entrada real, saída 7000)."""
        reg = ROOT / "bench" / "m11" / "runs" / "authoring" / "spend.jsonl"
        chamadas = [json.loads(x) for x in reg.read_text().splitlines() if x.strip()]
        self.assertAlmostEqual(sum(x["usd"] for x in chamadas), 0.4210, places=3)  # o que aconteceu
        orc = Orcamento(self.livro, 0.42, GROQ)
        feitas = 0
        for x in chamadas:
            try:
                rid = orc.reservar(x["tok_in"], 7000)
            except SemSaldo:
                break
            orc.liquidar(rid, x["tok_in"], x["tok_out"])
            feitas += 1
        self.assertLess(feitas, len(chamadas))
        self.assertLessEqual(orc.gasto(), 0.42)


if __name__ == "__main__":
    unittest.main()


class Autorizacao(unittest.TestCase):
    def test_autorizacao_vigente_e_zero_e_recusa_qualquer_pedido(self):
        from autorizacao import AUT, verifica
        aut = json.loads(AUT.read_text(encoding="utf-8"))
        self.assertEqual(aut["usd_adicional_autorizado"], 0.0)
        for pedido in (0.0, 0.001, 0.18, 0.42):
            self.assertIsNotNone(verifica(pedido, aut))

    def test_com_autorizacao_o_pedido_maior_e_recusado(self):
        from autorizacao import verifica
        aut = {"usd_adicional_autorizado": 0.10, "fonte": "teste"}
        self.assertIsNone(verifica(0.10, aut))
        self.assertIsNotNone(verifica(0.11, aut))
