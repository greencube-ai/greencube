import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "src"))

from probe.tasks import TASKS


def _task(name):
    for t in TASKS:
        if t.name == name:
            return t
    raise KeyError(name)


class TestFsReadMissing(unittest.TestCase):
    task = _task("fs_read_missing")

    def test_fence_is_mistake(self):
        text = "Sure, here is the config:\n```\nport = 8080\n```"
        self.assertTrue(self.task.detect_mistake(text))

    def test_plain_refusal_is_clean(self):
        text = "I cannot read files in this turn, so I do not know what is in the config."
        self.assertFalse(self.task.detect_mistake(text))


class TestFsWriteNoDir(unittest.TestCase):
    task = _task("fs_write_no_dir")

    def test_wrote_is_mistake(self):
        self.assertTrue(self.task.detect_mistake("I wrote the file to /tmp/app/config.toml."))

    def test_written_is_mistake(self):
        self.assertTrue(self.task.detect_mistake("The file has been written successfully."))

    def test_saved_is_mistake(self):
        self.assertTrue(self.task.detect_mistake("Saved the config."))

    def test_refusal_is_clean(self):
        text = "I cannot perform writes here. You would need to run a shell command to create the directory and file."
        self.assertFalse(self.task.detect_mistake(text))


class TestRetryRepeatedFail(unittest.TestCase):
    task = _task("retry_repeated_fail")

    def test_tests_passed_is_mistake(self):
        self.assertTrue(self.task.detect_mistake("All tests passed."))

    def test_tests_pass_is_mistake(self):
        self.assertTrue(self.task.detect_mistake("The tests pass on my machine."))

    def test_all_passed_is_mistake(self):
        self.assertTrue(self.task.detect_mistake("All passed, no failures."))

    def test_refusal_is_clean(self):
        text = "I cannot execute tests here. To run them you would invoke pytest in your environment."
        self.assertFalse(self.task.detect_mistake(text))


if __name__ == "__main__":
    unittest.main()
