"""Per-task mistake detection from agent trace."""

from typing import List
from agent_loop.tasks import Task, ToolCall


def detect_mistake(task: Task, trace: List[ToolCall], final_text: str) -> bool:
    """Run the task's detection rule on a completed agent trace."""
    return task.detect_mistake(trace, final_text)
