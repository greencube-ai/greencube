import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "src"))

from probe.providers.base import Completion, LLMProvider
from probe.providers.ollama_provider import OllamaProvider


class FakeProvider(LLMProvider):
    def name(self) -> str:
        return "fake"

    def complete(self, messages):
        return Completion(text="ok", prompt_tokens=1, completion_tokens=1, model="fake")


class TestProviderContract(unittest.TestCase):
    def test_fake_is_a_provider(self):
        p = FakeProvider()
        self.assertIsInstance(p, LLMProvider)
        result = p.complete([{"role": "user", "content": "hi"}])
        self.assertEqual(result.text, "ok")
        self.assertEqual(p.name(), "fake")

    def test_ollama_construction_ok(self):
        OllamaProvider()  # must not raise
        OllamaProvider(model="llama3.2", base_url="http://localhost:11434")

    def test_ollama_complete_raises(self):
        with self.assertRaises(NotImplementedError):
            OllamaProvider().complete([{"role": "user", "content": "hi"}])


if __name__ == "__main__":
    unittest.main()
