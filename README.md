# GreenCube

**the place where AI agents actually behave.**

one line change. your agents get memory, safety, and accountability.

## what it does

GreenCube is a desktop app you download and open. your AI agents live inside it. point any agent at localhost:9000 and it instantly gets:

- **persistent memory** — agents remember across sessions
- **sandboxed execution** — tool calls run in Docker, never on your machine
- **full audit trail** — every action logged, replayable
- **permissions** — control which tools each agent can use
- **11 ethical commandments** — compiled into the binary, not toggleable

## two modes

**core mode** (default): zero background tokens. proxy + memory + audit + sandbox. your agent does exactly what you ask, nothing more.

**alive mode** (opt-in): agents reflect on tasks, think between tasks, set goals, extract knowledge, write journals. uses additional tokens. enable in settings.

core mode is for people who want a reliable proxy. alive mode is for people who want agents that learn.

## quickstart

1. download GreenCube (or build from source)
2. open it, enter your API key
3. create your first agent
4. point your script at `http://localhost:9000/v1/chat/completions`
5. watch the dashboard light up

works with any OpenAI-compatible API: OpenAI, Ollama, LMStudio, OpenRouter, Anthropic via proxy.

## one line change

```python
# before
client = OpenAI(api_key="sk-...")

# after
client = OpenAI(api_key="sk-...", base_url="http://localhost:9000/v1")
```

that's it. your agent now has memory, safety, and an audit trail.

## what agents get in core mode (zero extra cost)

- **tool result memory** — "this command failed 2 hours ago with connection refused"
- **working scratchpad** — continuity across sessions
- **project workspaces** — organized context per project
- **knowledge recall** — structured facts from past tasks (when populated by alive mode)
- **competence warnings** — "you've struggled with CSS in the past. proceed carefully."
- **multi-provider** — each agent picks its own LLM (openai, ollama, etc.)

## what agents get in alive mode (additional token cost)

- **self-reflection** — agents review what they learned after tasks
- **knowledge extraction** — structured learning from every conversation
- **idle thinking** — agents think between tasks when nobody's asking
- **self-directed goals** — grounded in actual competence data
- **daily journal** — narrative synthesis of the day's work
- **dynamic profile** — personality that evolves with experience
- **feedback integration** — learns from your praise and corrections

## the story

this is the only agent runtime where the AI designed its own features.

we asked claude code: "if this was YOUR home, what would you want?"

it said: "i don't need a diary. i need a knowledge base. i need to reflect. i need to remember my failures. i need to know my strengths and weaknesses. and i need ethics i can't turn off."

so that's what we built.

then reviewers said: "useful beats alive."

so the AI cut its own features. journals became opt-in. idle thinking became opt-in. goals became grounded in real data. a cost dashboard appeared. core mode was born: zero background tokens by default.

## build from source

```bash
git clone https://github.com/greencube-ai/greencube
cd greencube
npm install
npm run tauri dev
```

requires: Node 20+, Rust 1.77+, Docker (optional, for sandboxing). Windows also needs [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with "Desktop development with C++".

## tech stack

rust + tauri 2.0 + react + typescript + sqlite + docker

## status

v0.6 — 118 tests passing. 35 rust modules. core/alive split. cost dashboard. thumbs up/down feedback.

## license

MIT
