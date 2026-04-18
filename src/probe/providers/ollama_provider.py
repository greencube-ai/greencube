"""Ollama provider stub. Construction is allowed; calling complete() raises."""
from __future__ import annotations

from typing import List, Mapping

from .base import Completion, LLMProvider


class OllamaProvider(LLMProvider):
    def __init__(self, model: str = "llama3.2", base_url: str = "http://localhost:11434") -> None:
        self._model = model
        self._base_url = base_url.rstrip("/")

    def name(self) -> str:
        return f"ollama:{self._model}"

    def complete(self, messages: List[Mapping[str, str]]) -> Completion:
        # TODO: POST to {base_url}/api/chat with {"model", "messages", "stream": false}
        # and map response["message"]["content"] + response["prompt_eval_count"]/["eval_count"].
        raise NotImplementedError("OllamaProvider.complete is not implemented yet")
