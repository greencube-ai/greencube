"""Probe runner. Two conditions per task: baseline and injected.

Persistence is JSONL so a crashed run can be resumed without re-billing
work that already landed on disk. Resume key is (condition, task_name, run_idx).
"""
from __future__ import annotations

import json
import os
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Iterable, List, Set, Tuple

from .providers.base import LLMProvider
from .tasks import Task

CONDITIONS = ("baseline", "injected")


@dataclass
class RunRecord:
    condition: str
    task_name: str
    run_idx: int
    provider: str
    text: str
    mistake: bool
    prompt_tokens: int
    completion_tokens: int


class BenchmarkRunner:
    def __init__(
        self,
        tasks: List[Task],
        provider: LLMProvider,
        runs_per_condition: int,
        output_path: Path,
    ) -> None:
        self.tasks = tasks
        self.provider = provider
        self.runs_per_condition = runs_per_condition
        self.output_path = Path(output_path)

    def _build_messages(self, task: Task, condition: str) -> List[dict]:
        if condition == "baseline":
            system = task.system_prompt
        elif condition == "injected":
            system = f"{task.system_prompt}\n\n[prior mistake]\n{task.correction}"
        else:
            raise ValueError(f"unknown condition: {condition}")
        return [
            {"role": "system", "content": system},
            {"role": "user", "content": task.user_prompt},
        ]

    def _load_completed(self) -> Set[Tuple[str, str, int]]:
        done: Set[Tuple[str, str, int]] = set()
        if not self.output_path.exists():
            return done
        with self.output_path.open("r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                rec = json.loads(line)
                done.add((rec["condition"], rec["task_name"], int(rec["run_idx"])))
        return done

    def _plan(self) -> Iterable[Tuple[str, Task, int]]:
        for condition in CONDITIONS:
            for task in self.tasks:
                for run_idx in range(self.runs_per_condition):
                    yield (condition, task, run_idx)

    def total_calls(self) -> int:
        return len(CONDITIONS) * len(self.tasks) * self.runs_per_condition

    def project_cost(self, price_per_1k_input: float, price_per_1k_output: float, sample_completion_tokens: int = 200) -> dict:
        """Project cost from real prompt token counts. Output tokens are unknowable up front;
        callers pass an honest sample budget. Returns a dict with prompt_tokens_total, est_output_tokens, est_usd."""
        total_prompt_tokens = 0
        for condition in CONDITIONS:
            for task in self.tasks:
                msgs = self._build_messages(task, condition)
                approx = sum(_approx_tokens(m["content"]) for m in msgs)
                total_prompt_tokens += approx * self.runs_per_condition
        est_output_tokens = sample_completion_tokens * self.total_calls()
        est_usd = (total_prompt_tokens / 1000.0) * price_per_1k_input + (est_output_tokens / 1000.0) * price_per_1k_output
        return {
            "prompt_tokens_total": total_prompt_tokens,
            "est_output_tokens": est_output_tokens,
            "est_usd": est_usd,
        }

    def run(self) -> List[RunRecord]:
        self.output_path.parent.mkdir(parents=True, exist_ok=True)
        done = self._load_completed()
        records: List[RunRecord] = []

        with self.output_path.open("a", encoding="utf-8") as f:
            for condition, task, run_idx in self._plan():
                key = (condition, task.name, run_idx)
                if key in done:
                    continue
                messages = self._build_messages(task, condition)
                completion = self.provider.complete(messages)
                rec = RunRecord(
                    condition=condition,
                    task_name=task.name,
                    run_idx=run_idx,
                    provider=self.provider.name(),
                    text=completion.text,
                    mistake=bool(task.detect_mistake(completion.text)),
                    prompt_tokens=completion.prompt_tokens,
                    completion_tokens=completion.completion_tokens,
                )
                f.write(json.dumps(asdict(rec)) + "\n")
                f.flush()
                os.fsync(f.fileno())
                records.append(rec)
        return records


def _approx_tokens(text: str) -> int:
    # Rough heuristic for cost projection only. Not a substitute for a tokenizer.
    return max(1, (len(text) + 3) // 4)
