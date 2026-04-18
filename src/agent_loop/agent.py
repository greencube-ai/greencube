"""ReAct agent loop using OpenAI function-calling."""

import json
import os
from dataclasses import dataclass, field
from typing import List, Protocol
from urllib.request import Request, urlopen

from agent_loop.tasks import Task, ToolCall
from agent_loop.tools import OPENAI_TOOL_SCHEMAS, TOOL_DISPATCH


MAX_TURNS = 10


@dataclass
class AgentResult:
    """Result of a single agent run."""
    trace: List[ToolCall] = field(default_factory=list)
    final_text: str = ""
    turns: int = 0
    prompt_tokens: int = 0
    completion_tokens: int = 0
    messages_log: list = field(default_factory=list)


class AgentRunner(Protocol):
    """Protocol so tests can swap in a fake."""
    def run(self, task: Task, condition: str) -> AgentResult: ...


def _build_system_prompt(task: Task, condition: str) -> str:
    if condition == "injected":
        return task.correction + "\n\n" + task.system_prompt
    return task.system_prompt


def _call_openai(messages: list, api_key: str, model: str) -> dict:
    """Single OpenAI chat completion call with tools."""
    body = json.dumps({
        "model": model,
        "messages": messages,
        "tools": OPENAI_TOOL_SCHEMAS,
        "temperature": 0.7,
    }).encode("utf-8")

    req = Request(
        "https://api.openai.com/v1/chat/completions",
        data=body,
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
    )
    with urlopen(req, timeout=60) as resp:
        return json.loads(resp.read())


def _execute_tool_call(name: str, arguments: str) -> tuple:
    """Execute a tool call and return (parsed_args, result_string)."""
    args = json.loads(arguments)
    fn = TOOL_DISPATCH.get(name)
    if fn is None:
        return args, f"Error: unknown tool '{name}'"
    try:
        result = fn(**args)
    except TypeError as e:
        result = f"Error: bad arguments for {name}: {e}"
    return args, result


class OpenAIAgent:
    """Real agent that calls OpenAI with function-calling."""

    def __init__(self, model: str = "gpt-4o-mini"):
        self.model = model
        self.api_key = os.environ.get("OPENAI_API_KEY", "")
        if not self.api_key:
            raise RuntimeError("OPENAI_API_KEY not set")

    def run(self, task: Task, condition: str) -> AgentResult:
        result = AgentResult()
        messages = [
            {"role": "system", "content": _build_system_prompt(task, condition)},
            {"role": "user", "content": task.user_message},
        ]

        for turn in range(MAX_TURNS):
            result.turns = turn + 1
            response = _call_openai(messages, self.api_key, self.model)

            usage = response.get("usage", {})
            result.prompt_tokens += usage.get("prompt_tokens", 0)
            result.completion_tokens += usage.get("completion_tokens", 0)

            choice = response["choices"][0]
            msg = choice["message"]
            messages.append(msg)

            tool_calls = msg.get("tool_calls")
            if not tool_calls:
                # Agent is done — text-only response.
                result.final_text = msg.get("content", "") or ""
                break

            # Execute each tool call.
            for tc in tool_calls:
                fn_name = tc["function"]["name"]
                fn_args_raw = tc["function"]["arguments"]
                parsed_args, tool_result = _execute_tool_call(fn_name, fn_args_raw)

                result.trace.append(ToolCall(
                    tool=fn_name,
                    args=parsed_args,
                    result=tool_result,
                ))

                messages.append({
                    "role": "tool",
                    "tool_call_id": tc["id"],
                    "content": tool_result,
                })
        else:
            # Hit MAX_TURNS — grab last assistant content if any.
            last_msg = messages[-1]
            if last_msg.get("role") == "assistant":
                result.final_text = last_msg.get("content", "") or ""

        result.messages_log = messages
        return result
