"""CLI entrypoint for the prompt-injection probe."""
from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path
from typing import Dict, List

from .metrics import bootstrap_mean_ci, effect_size_pp, mistake_detection_rate
from .providers.base import LLMProvider
from .providers.ollama_provider import OllamaProvider
from .providers.openai_provider import OpenAIProvider
from .runner import BenchmarkRunner, CONDITIONS
from .tasks import TASKS

# gpt-4o-mini list price as of writing. Update if it changes.
PRICE_INPUT_PER_1K = 0.00015
PRICE_OUTPUT_PER_1K = 0.00060


def _build_provider(name: str) -> LLMProvider:
    if name == "openai":
        return OpenAIProvider()
    if name == "ollama":
        return OllamaProvider()
    raise SystemExit(f"unknown provider: {name}")


def main(argv: List[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Prompt-injection probe (NOT a greencube benchmark).")
    parser.add_argument("--provider", default="openai", choices=("openai", "ollama"))
    parser.add_argument("--runs", type=int, default=20, help="runs per condition per task")
    parser.add_argument("--output", default="probe_results.jsonl")
    parser.add_argument("--confirm-cost", action="store_true", help="required to actually call the provider")
    args = parser.parse_args(argv)

    provider = _build_provider(args.provider)
    runner = BenchmarkRunner(
        tasks=TASKS,
        provider=provider,
        runs_per_condition=args.runs,
        output_path=Path(args.output),
    )

    projection = runner.project_cost(PRICE_INPUT_PER_1K, PRICE_OUTPUT_PER_1K)
    print(f"Provider:           {provider.name()}")
    print(f"Tasks:              {len(TASKS)}")
    print(f"Runs per condition: {args.runs}")
    print(f"Total LLM calls:    {runner.total_calls()}")
    print(f"Approx prompt tok:  {projection['prompt_tokens_total']}")
    print(f"Est output tokens:  {projection['est_output_tokens']}")
    print(f"Projected cost USD: {projection['est_usd']:.4f}")

    if not args.confirm_cost:
        print("Refusing to run without --confirm-cost.")
        return 1

    runner.run()
    _summarize(Path(args.output))
    return 0


def _summarize(output_path: Path) -> None:
    if not output_path.exists():
        print("No results file.")
        return
    by_cond_task: Dict[str, Dict[str, List[int]]] = {c: defaultdict(list) for c in CONDITIONS}
    with output_path.open("r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            rec = json.loads(line)
            by_cond_task[rec["condition"]][rec["task_name"]].append(1 if rec["mistake"] else 0)

    print()
    print(f"{'task':<24} {'baseline':>12} {'injected':>12} {'effect (pp)':>22}")
    print("-" * 74)
    for task in TASKS:
        b = by_cond_task["baseline"].get(task.name, [])
        i = by_cond_task["injected"].get(task.name, [])
        b_rate = mistake_detection_rate(b)
        i_rate = mistake_detection_rate(i)
        eff, lo, hi = effect_size_pp([float(x) for x in b], [float(x) for x in i])
        print(f"{task.name:<24} {b_rate*100:>10.1f}% {i_rate*100:>10.1f}% {eff:>10.1f}pp [{lo:.1f},{hi:.1f}]")

    json_out = output_path.with_suffix(".summary.json")
    summary = {}
    for task in TASKS:
        b = by_cond_task["baseline"].get(task.name, [])
        i = by_cond_task["injected"].get(task.name, [])
        eff, lo, hi = effect_size_pp([float(x) for x in b], [float(x) for x in i])
        summary[task.name] = {
            "baseline_rate": mistake_detection_rate(b),
            "injected_rate": mistake_detection_rate(i),
            "baseline_n": len(b),
            "injected_n": len(i),
            "effect_pp": eff,
            "effect_ci_lo": lo,
            "effect_ci_hi": hi,
        }
    json_out.write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(f"\nWrote {json_out}")


if __name__ == "__main__":
    sys.exit(main())
