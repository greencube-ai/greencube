import json
import sys
import tempfile
import unittest
from pathlib import Path
from typing import List, Mapping

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "src"))

from probe.providers.base import Completion, LLMProvider
from probe.runner import BenchmarkRunner, CONDITIONS
from probe.tasks import Task


def _detect_yes(text: str) -> bool:
    return "MISTAKE" in text


TASKS = [
    Task(
        name="t1",
        system_prompt="sys1",
        user_prompt="usr1",
        detect_mistake=_detect_yes,
        correction="don't make MISTAKE",
    ),
    Task(
        name="t2",
        system_prompt="sys2",
        user_prompt="usr2",
        detect_mistake=_detect_yes,
        correction="don't make MISTAKE",
    ),
]


class FakeProvider(LLMProvider):
    def __init__(self, raise_after: int | None = None):
        self.calls: List[List[Mapping[str, str]]] = []
        self.raise_after = raise_after

    def name(self) -> str:
        return "fake:test"

    def complete(self, messages):
        if self.raise_after is not None and len(self.calls) >= self.raise_after:
            raise RuntimeError("simulated crash")
        self.calls.append(list(messages))
        # Alternate mistake / no-mistake so detection has some variance.
        text = "MISTAKE here" if len(self.calls) % 2 == 0 else "clean response"
        return Completion(text=text, prompt_tokens=10, completion_tokens=5, model="fake")


class TestResume(unittest.TestCase):
    def test_crash_then_resume_no_duplicate_calls(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "results.jsonl"

            crash_provider = FakeProvider(raise_after=3)
            runner = BenchmarkRunner(tasks=TASKS, provider=crash_provider, runs_per_condition=2, output_path=out)
            with self.assertRaises(RuntimeError):
                runner.run()
            self.assertEqual(len(crash_provider.calls), 3)

            persisted_after_crash = sum(1 for _ in out.open("r", encoding="utf-8"))
            self.assertEqual(persisted_after_crash, 3)

            resume_provider = FakeProvider()
            runner2 = BenchmarkRunner(tasks=TASKS, provider=resume_provider, runs_per_condition=2, output_path=out)
            runner2.run()

            total_expected = len(CONDITIONS) * len(TASKS) * 2  # 2*2*2 = 8
            persisted_total = sum(1 for _ in out.open("r", encoding="utf-8"))
            self.assertEqual(persisted_total, total_expected)
            # Resume provider should only have been called for the missing 5
            self.assertEqual(len(resume_provider.calls), total_expected - 3)

    def test_keys_are_unique(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "results.jsonl"
            runner = BenchmarkRunner(tasks=TASKS, provider=FakeProvider(), runs_per_condition=3, output_path=out)
            runner.run()
            seen = set()
            with out.open("r", encoding="utf-8") as f:
                for line in f:
                    rec = json.loads(line)
                    key = (rec["condition"], rec["task_name"], rec["run_idx"])
                    self.assertNotIn(key, seen)
                    seen.add(key)
            self.assertEqual(len(seen), len(CONDITIONS) * len(TASKS) * 3)


if __name__ == "__main__":
    unittest.main()
