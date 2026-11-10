"""A estatística pareada do AgentDojo (`stats.py`): só biblioteca padrão, sem rede."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from stats import cluster_bootstrap, mcnemar_exact  # noqa: E402


class StatsTests(unittest.TestCase):
    def test_mcnemar_exact_matches_the_binomial(self):
        self.assertEqual(mcnemar_exact(0, 0), 1.0)
        self.assertAlmostEqual(mcnemar_exact(0, 5), 2 / 32)
        self.assertAlmostEqual(mcnemar_exact(1, 9), 2 * 11 / 1024)
        self.assertEqual(mcnemar_exact(3, 3), 1.0)
        self.assertAlmostEqual(mcnemar_exact(9, 1), mcnemar_exact(1, 9))

    def test_cluster_bootstrap_is_deterministic_and_refuses_zero_events(self):
        g = {"u0": (9, 9), "u1": (0, 9), "u2": (3, 9)}
        a, b = cluster_bootstrap(g), cluster_bootstrap(g)
        self.assertEqual(a, b)
        self.assertLessEqual(a[0], 12 / 27)
        self.assertGreaterEqual(a[1], 12 / 27)
        # com zero eventos o percentil degenera em [0, 0]: não é um intervalo
        self.assertIsNone(cluster_bootstrap({"u0": (0, 9), "u1": (0, 9)}))


if __name__ == "__main__":
    unittest.main()
