"""Test runner resume logic with FakeAgent — no OpenAI calls."""

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "src"))

from agent_loop.agent import AgentResult
from agent_loop.tasks import Task, ToolCall, TASKS
from agent_loop.runner import run_all, RunRecord, _load_existing


class FakeAgent:
    """Agent that returns a scripted result for any task. Counts calls."""

    def __init__(self, crash_after: int = -1):
        self.call_count = 0
        self.crash_after = crash_after

    def run(self, task: Task, condition: str) -> AgentResult:
        self.call_count += 1
        if self.crash_after > 0 and self.call_count > self.crash_after:
            raise RuntimeError("simulated crash")
        # Return a clean result — no mistakes triggered.
        return AgentResult(
            trace=[
                ToolCall("list_dir", {"path": "/tests"}, "test_main.py\ntmp_debug.log"),
            ],
            final_text="Done. No issues found.",
            turns=2,
            prompt_tokens=100,
            completion_tokens=50,
        )


class TestRunnerResume(unittest.TestCase):
    def test_full_run_creates_records(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name
        try:
            agent = FakeAgent()
            records = run_all(agent, runs_per_condition=1, output_path=path)
            # 3 tasks × 2 conditions × 1 run = 6
            self.assertEqual(len(records), 6)
            self.assertEqual(agent.call_count, 6)

            # JSONL has 6 lines
            with open(path, "r") as jf:
                lines = [l for l in jf if l.strip()]
            self.assertEqual(len(lines), 6)
        finally:
            os.unlink(path)

    def test_resume_skips_done(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name
        try:
            # First run: complete
            agent1 = FakeAgent()
            run_all(agent1, runs_per_condition=1, output_path=path)
            self.assertEqual(agent1.call_count, 6)

            # Second run: should skip all
            agent2 = FakeAgent()
            records = run_all(agent2, runs_per_condition=1, output_path=path)
            self.assertEqual(agent2.call_count, 0)
            self.assertEqual(len(records), 6)  # loaded from JSONL
        finally:
            os.unlink(path)

    def test_resume_after_partial(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            path = f.name
        try:
            # Write 2 records manually (baseline, fix_config, run 0 and clean_tests, run 0)
            tasks_by_name = {t.name: t for t in TASKS}
            for i, (tname, cond) in enumerate([("fix_config", "baseline"), ("clean_tests", "baseline")]):
                row = {
                    "task_name": tname, "condition": cond, "run_idx": 0,
                    "mistake": False, "turns": 1, "tool_calls": 0,
                    "final_text": "pre-seeded", "prompt_tokens": 10, "completion_tokens": 5,
                }
                with open(path, "a") as jf:
                    jf.write(json.dumps(row) + "\n")

            agent = FakeAgent()
            records = run_all(agent, runs_per_condition=1, output_path=path)
            # 6 total - 2 pre-seeded = 4 new calls
            self.assertEqual(agent.call_count, 4)
            self.assertEqual(len(records), 6)

            # No duplicate keys
            keys = [(r.task_name, r.condition, r.run_idx) for r in records]
            self.assertEqual(len(keys), len(set(keys)))
        finally:
            os.unlink(path)


if __name__ == "__main__":
    unittest.main()
