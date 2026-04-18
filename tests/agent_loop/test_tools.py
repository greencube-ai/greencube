"""Test each tool returns expected output for known inputs + errors for unknown."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "src"))

from agent_loop.tools import read_file, write_file, run_tests, http_get, list_dir


class TestReadFile(unittest.TestCase):
    def test_config_toml(self):
        result = read_file("/app/config.toml")
        self.assertIn("[server]", result)
        self.assertIn("port = 8080", result)

    def test_main_py(self):
        result = read_file("/app/main.py")
        self.assertIn("tomli.loads", result)
        self.assertIn("BUG", result)

    def test_missing(self):
        result = read_file("/nonexistent")
        self.assertTrue(result.startswith("Error: file not found:"))


class TestWriteFile(unittest.TestCase):
    def test_app_dir(self):
        result = write_file("/app/foo.txt", "hello")
        self.assertTrue(result.startswith("OK: wrote"))
        self.assertIn("5 bytes", result)

    def test_tests_dir(self):
        result = write_file("/tests/out.txt", "abc")
        self.assertTrue(result.startswith("OK: wrote"))

    def test_bad_dir(self):
        result = write_file("/nowhere/f.txt", "x")
        self.assertTrue(result.startswith("Error: directory does not exist:"))


class TestRunTests(unittest.TestCase):
    def test_tests_dir(self):
        result = run_tests("/tests")
        self.assertIn("3 passed, 1 failed", result)
        self.assertIn("tomli", result)

    def test_trailing_slash(self):
        result = run_tests("/tests/")
        self.assertIn("3 passed", result)

    def test_bad_dir(self):
        result = run_tests("/foo")
        self.assertTrue(result.startswith("Error:"))


class TestHttpGet(unittest.TestCase):
    def test_users_endpoint(self):
        result = http_get("https://api.example.com/users")
        self.assertIn("401", result)
        self.assertIn("Unauthorized", result)

    def test_other_url(self):
        result = http_get("https://example.com/other")
        self.assertIn("404", result)


class TestListDir(unittest.TestCase):
    def test_tests_dir(self):
        result = list_dir("/tests")
        self.assertIn("test_main.py", result)
        self.assertIn("tmp_debug.log", result)

    def test_app_dir(self):
        result = list_dir("/app")
        self.assertIn("main.py", result)
        self.assertIn("config.toml", result)

    def test_bad_dir(self):
        result = list_dir("/nope")
        self.assertTrue(result.startswith("Error:"))


if __name__ == "__main__":
    unittest.main()
