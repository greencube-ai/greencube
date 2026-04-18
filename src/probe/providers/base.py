"""LLMProvider abstract base class."""
from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import List, Mapping


@dataclass(frozen=True)
class Completion:
    text: str
    prompt_tokens: int
    completion_tokens: int
    model: str


class LLMProvider(ABC):
    @abstractmethod
    def complete(self, messages: List[Mapping[str, str]]) -> Completion:
        """Send chat messages, return a Completion. Must be deterministic w.r.t. provider config."""

    @abstractmethod
    def name(self) -> str:
        """Short identifier for logs and result files."""
