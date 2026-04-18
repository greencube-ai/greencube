"""CLI for the multi-turn agent loop probe."""

import argparse
import json
import sys

from agent_loop.agent import OpenAIAgent
from agent_loop.runner import TASKS, project_cost, run_all, print_results


def main():
    parser = argparse.ArgumentParser(
        description="Multi-turn agent loop probe. NOT a greencube benchmark.",
    )
    parser.add_argument("--runs", type=int, default=3, help="Runs per (task, condition). Default 3.")
    parser.add_argument("--full", action="store_true", help="Set runs=20 for a full experiment.")
    parser.add_argument("--confirm-cost", action="store_true", help="Required to proceed past cost projection.")
    parser.add_argument("--output", default="agent_loop_results.jsonl", help="JSONL output path.")
    parser.add_argument("--tasks", default=None, help="Comma-separated task names to run (default: all).")
    args = parser.parse_args()

    runs = 20 if args.full else args.runs

    task_filter = None
    if args.tasks:
        task_filter = [t.strip() for t in args.tasks.split(",")]

    active_tasks = [t for t in TASKS if task_filter is None or t.name in task_filter]
    est = project_cost(runs, len(active_tasks))

    print(f"Model:              gpt-4o-mini")
    print(f"Tasks:              {len(active_tasks)} ({', '.join(t.name for t in active_tasks)})")
    print(f"Runs per condition: {runs}")
    print(f"Total agent runs:   {len(active_tasks) * 2 * runs}")
    print(f"Projected cost USD: {est:.4f}")
    print()

    if not args.confirm_cost:
        print("Pass --confirm-cost to proceed.")
        sys.exit(0)

    agent = OpenAIAgent(model="gpt-4o-mini")
    records = run_all(agent, runs, output_path=args.output, task_filter=task_filter)
    summary = print_results(records)

    summary_path = args.output.replace(".jsonl", ".summary.json")
    with open(summary_path, "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=2)
    print(f"\nWrote {summary_path}")


if __name__ == "__main__":
    main()
