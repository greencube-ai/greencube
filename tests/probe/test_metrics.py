import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "src"))

from probe.metrics import bootstrap_mean_ci, effect_size_pp, mistake_detection_rate


class TestMistakeRate(unittest.TestCase):
    def test_empty(self):
        self.assertEqual(mistake_detection_rate([]), 0.0)

    def test_all_zero(self):
        self.assertEqual(mistake_detection_rate([0, 0, 0, 0]), 0.0)

    def test_all_one(self):
        self.assertEqual(mistake_detection_rate([1, 1, 1, 1]), 1.0)

    def test_half(self):
        self.assertEqual(mistake_detection_rate([0, 1, 0, 1]), 0.5)


class TestBootstrapMeanCI(unittest.TestCase):
    def test_constant(self):
        mean, lo, hi = bootstrap_mean_ci([1.0] * 20, n_resamples=2000, seed=42)
        self.assertEqual(mean, 1.0)
        self.assertEqual(lo, 1.0)
        self.assertEqual(hi, 1.0)

    def test_known_mean(self):
        data = [0.0, 1.0] * 50
        mean, lo, hi = bootstrap_mean_ci(data, n_resamples=2000, seed=7)
        self.assertAlmostEqual(mean, 0.5)
        self.assertLess(lo, 0.5)
        self.assertGreater(hi, 0.5)

    def test_seed_reproducibility(self):
        data = [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0]
        a = bootstrap_mean_ci(data, n_resamples=1500, seed=123)
        b = bootstrap_mean_ci(data, n_resamples=1500, seed=123)
        self.assertEqual(a, b)


class TestEffectSizePP(unittest.TestCase):
    def test_identical(self):
        eff, lo, hi = effect_size_pp([0.0, 1.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0], n_resamples=1500, seed=0)
        self.assertAlmostEqual(eff, 0.0)

    def test_all_one_vs_all_zero(self):
        eff, lo, hi = effect_size_pp([1.0] * 20, [0.0] * 20, n_resamples=1500, seed=0)
        self.assertAlmostEqual(eff, 100.0)
        self.assertAlmostEqual(lo, 100.0)
        self.assertAlmostEqual(hi, 100.0)

    def test_sign_convention(self):
        # injection reduces mistakes -> positive effect
        eff, _, _ = effect_size_pp([1.0] * 20, [0.0] * 20, n_resamples=500, seed=0)
        self.assertGreater(eff, 0.0)


if __name__ == "__main__":
    unittest.main()
