# GreenCube

**the place where AI agents actually behave.**

one line change. your agents get memory, safety, and accountability.

## what it does

GreenCube is a desktop app you download and open. Your AI agents live inside it. Point any agent at localhost:9000 and it instantly gets:

- **persistent memory** — agents remember across sessions
- **sandboxed execution** — tool calls run in Docker, never on your machine
- **full audit trail** — every action logged, replayable
- **permissions** — control which tools each agent can use

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

## build from source

```bash
git clone https://github.com/greencube-ai/greencube
cd greencube
npm install
npm run tauri dev
```

requires: Node 20+, Rust 1.77+, Docker (optional, for sandboxing)

## tech stack

rust + tauri 2.0 + react + typescript + sqlite + docker

## status

v0.1 — 43 tests passing, working proxy, memory injection, audit logging, dark theme UI

## license

MIT
