"""Bootstrap statistics for the agent loop probe.

Adapted from src/probe/metrics.py. No t-tests, no normality assumption.
"""
from __future__ import annotations

import random
from typing import List, Sequence, Tuple


def mistake_detection_rate(values: Sequence[int]) -> float:
    """Fraction of runs flagged as containing the targeted mistake.

    `values` is a sequence of 0/1 ints from the per-run detection rule.
    Returns 0.0 for an empty input rather than raising.
    """
    if not values:
        return 0.0
    return sum(values) / len(values)


def bootstrap_mean_ci(
    data: Sequence[float],
    n_resamples: int = 10000,
    confidence: float = 0.95,
    seed: int = 0,
) -> Tuple[float, float, float]:
    """Percentile-method bootstrap CI for the mean. Returns (mean, lo, hi)."""
    if not data:
        return (0.0, 0.0, 0.0)
    rng = random.Random(seed)
    n = len(data)
    means: List[float] = []
    for _ in range(n_resamples):
        s = 0.0
        for _ in range(n):
            s += data[rng.randrange(n)]
        means.append(s / n)
    means.sort()
    alpha = (1.0 - confidence) / 2.0
    lo_idx = int(alpha * n_resamples)
    hi_idx = min(n_resamples - 1, int((1.0 - alpha) * n_resamples) - 1)
    mean = sum(data) / n
    return (mean, means[lo_idx], means[hi_idx])


def effect_size_pp(
    baseline: Sequence[float],
    injected: Sequence[float],
    n_resamples: int = 10000,
    confidence: float = 0.95,
    seed: int = 0,
) -> Tuple[float, float, float]:
    """Bootstrap percentage-point difference (baseline - injected) with CI.

    Positive number = injection reduced the mistake rate.
    """
    if not baseline or not injected:
        return (0.0, 0.0, 0.0)
    rng = random.Random(seed)
    nb = len(baseline)
    ni = len(injected)
    diffs: List[float] = []
    for _ in range(n_resamples):
        sb = 0.0
        for _ in range(nb):
            sb += baseline[rng.randrange(nb)]
        si = 0.0
        for _ in range(ni):
            si += injected[rng.randrange(ni)]
        diffs.append((sb / nb) - (si / ni))
    diffs.sort()
    alpha = (1.0 - confidence) / 2.0
    lo_idx = int(alpha * n_resamples)
    hi_idx = min(n_resamples - 1, int((1.0 - alpha) * n_resamples) - 1)
    point = (sum(baseline) / nb) - (sum(injected) / ni)
    return (point * 100.0, diffs[lo_idx] * 100.0, diffs[hi_idx] * 100.0)
