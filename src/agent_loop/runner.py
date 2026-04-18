"""Runner: iterate (condition, task, run_idx), persist JSONL, resume."""

import json
import os
from collections import defaultdict
from dataclasses import dataclass
from typing import List, Set, Tuple

from agent_loop.agent import AgentResult, AgentRunner
from agent_loop.detection import detect_mistake
from agent_loop.metrics import bootstrap_mean_ci, effect_size_pp
from agent_loop.tasks import TASKS, Task


@dataclass
class RunRecord:
    task_name: str
    condition: str
    run_idx: int
    mistake: bool
    turns: int
    tool_calls: int
    final_text: str
    prompt_tokens: int
    completion_tokens: int


def _load_existing(path: str) -> Tuple[List[RunRecord], Set[Tuple[str, str, int]]]:
    """Load existing JSONL and return records + set of completed keys."""
    records: List[RunRecord] = []
    keys: Set[Tuple[str, str, int]] = set()
    if not os.path.exists(path):
        return records, keys
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            d = json.loads(line)
            rec = RunRecord(
                task_name=d["task_name"],
                condition=d["condition"],
                run_idx=d["run_idx"],
                mistake=d["mistake"],
                turns=d["turns"],
                tool_calls=d["tool_calls"],
                final_text=d["final_text"],
                prompt_tokens=d["prompt_tokens"],
                completion_tokens=d["completion_tokens"],
            )
            records.append(rec)
            keys.add((rec.task_name, rec.condition, rec.run_idx))
    return records, keys


def project_cost(runs_per_condition: int, num_tasks: int = None) -> float:
    """Estimate cost in USD for the full run.

    Rough estimate: ~400 prompt tokens + ~300 completion tokens per call,
    ~3 calls per run (multi-turn). gpt-4o-mini pricing:
    $0.15/1M prompt, $0.60/1M completion.
    """
    if num_tasks is None:
        num_tasks = len(TASKS)
    total_runs = num_tasks * 2 * runs_per_condition
    calls_per_run = 3
    total_calls = total_runs * calls_per_run
    prompt_tokens = total_calls * 400
    completion_tokens = total_calls * 300
    return (prompt_tokens * 0.00015 / 1000) + (completion_tokens * 0.00060 / 1000)


def run_all(
    agent: AgentRunner,
    runs_per_condition: int,
    output_path: str = "agent_loop_results.jsonl",
    task_filter: List[str] = None,
) -> List[RunRecord]:
    """Run all (condition, task, run_idx) combos, skip already-persisted."""
    existing_records, done_keys = _load_existing(output_path)
    all_records = list(existing_records)

    active_tasks = [t for t in TASKS if task_filter is None or t.name in task_filter]
    total = len(active_tasks) * 2 * runs_per_condition
    skipped = len(done_keys)
    remaining = total - skipped
    print(f"Total runs: {total}, already done: {skipped}, remaining: {remaining}")

    with open(output_path, "a", encoding="utf-8") as f:
        for condition in ("baseline", "injected"):
            for task in active_tasks:
                for run_idx in range(runs_per_condition):
                    key = (task.name, condition, run_idx)
                    if key in done_keys:
                        continue

                    print(f"  [{condition}] {task.name} run {run_idx}...", end=" ", flush=True)
                    result: AgentResult = agent.run(task, condition)
                    mistake = detect_mistake(task, result.trace, result.final_text)

                    rec = RunRecord(
                        task_name=task.name,
                        condition=condition,
                        run_idx=run_idx,
                        mistake=mistake,
                        turns=result.turns,
                        tool_calls=len(result.trace),
                        final_text=result.final_text,
                        prompt_tokens=result.prompt_tokens,
                        completion_tokens=result.completion_tokens,
                    )
                    all_records.append(rec)

                    row = {
                        "task_name": rec.task_name,
                        "condition": rec.condition,
                        "run_idx": rec.run_idx,
                        "mistake": rec.mistake,
                        "turns": rec.turns,
                        "tool_calls": rec.tool_calls,
                        "final_text": rec.final_text,
                        "prompt_tokens": rec.prompt_tokens,
                        "completion_tokens": rec.completion_tokens,
                    }
                    f.write(json.dumps(row) + "\n")
                    f.flush()

                    status = "MISTAKE" if mistake else "ok"
                    print(f"{status} (turns={result.turns}, tools={len(result.trace)})")

    return all_records


def print_results(records: List[RunRecord]) -> dict:
    """Print results table and return summary dict."""
    by_cond = defaultdict(list)
    by_task_cond = defaultdict(lambda: defaultdict(list))

    for r in records:
        val = 1.0 if r.mistake else 0.0
        by_cond[r.condition].append(val)
        by_task_cond[r.task_name][r.condition].append(val)

    print()
    print(f"{'task':<20} {'condition':<12} {'n':>4} {'mistake%':>10} {'95% CI':>20}")
    print("-" * 70)

    summary = {"per_task": {}, "overall": {}}

    for task_name in sorted(by_task_cond.keys()):
        for cond in ("baseline", "injected"):
            vals = by_task_cond[task_name][cond]
            if not vals:
                continue
            mean, lo, hi = bootstrap_mean_ci(vals, seed=0)
            print(f"{task_name:<20} {cond:<12} {len(vals):>4} {mean*100:>9.1f}% [{lo*100:.1f}%, {hi*100:.1f}%]")
            summary["per_task"].setdefault(task_name, {})[cond] = {
                "n": len(vals), "mean": mean, "ci_lo": lo, "ci_hi": hi,
            }
        eff, elo, ehi = effect_size_pp(
            by_task_cond[task_name].get("baseline", []),
            by_task_cond[task_name].get("injected", []),
            seed=0,
        )
        print(f"{'':>20} {'effect':>12} {'':>4} {eff:>9.1f}pp [{elo:.1f}, {ehi:.1f}]")
        summary["per_task"][task_name]["effect"] = {"pp": eff, "ci_lo": elo, "ci_hi": ehi}

    print("-" * 70)
    for cond in ("baseline", "injected"):
        vals = by_cond[cond]
        if not vals:
            continue
        mean, lo, hi = bootstrap_mean_ci(vals, seed=0)
        print(f"{'OVERALL':<20} {cond:<12} {len(vals):>4} {mean*100:>9.1f}% [{lo*100:.1f}%, {hi*100:.1f}%]")
        summary["overall"][cond] = {"n": len(vals), "mean": mean, "ci_lo": lo, "ci_hi": hi}

    eff, elo, ehi = effect_size_pp(by_cond.get("baseline", []), by_cond.get("injected", []), seed=0)
    print(f"{'':>20} {'effect':>12} {'':>4} {eff:>9.1f}pp [{elo:.1f}, {ehi:.1f}]")
    summary["overall"]["effect"] = {"pp": eff, "ci_lo": elo, "ci_hi": ehi}

    # Actual cost from token counts
    total_prompt = sum(r.prompt_tokens for r in records)
    total_completion = sum(r.completion_tokens for r in records)
    actual_cost = (total_prompt * 0.00015 / 1000) + (total_completion * 0.00060 / 1000)
    print(f"\nActual cost: ${actual_cost:.6f} ({total_prompt} prompt + {total_completion} completion tokens)")
    summary["cost"] = {"usd": actual_cost, "prompt_tokens": total_prompt, "completion_tokens": total_completion}

    return summary
