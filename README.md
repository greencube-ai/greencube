# GreenCube

where AI agents learn.

GreenCube is a desktop app that sits between your agent and its LLM. one line change gives your agent persistent memory, safe tool execution, and a full audit trail. agents get smarter over time without changing your code.

## quick start

```bash
export OPENAI_API_BASE=http://localhost:9000/v1
```

run your OpenClaw agent normally. done.

works with any OpenAI-compatible tool: OpenClaw, LangChain, CrewAI, raw Python, curl.

## what your agent gets

- **persistent memory** — remembers across sessions
- **docker sandbox** — tool calls run in containers, never on your machine
- **audit trail** — every action logged
- **streaming** — SSE support for real-time responses
- **multi-provider** — each agent can use a different LLM
- **11 ethical commandments** — baked into the binary

## alive mode

opt-in. zero extra tokens by default.

when enabled, agents reflect after tasks, extract knowledge, track their own competence, communicate with other agents, and spawn specialists when they're struggling in a domain.

## build from source

```bash
git clone https://github.com/greencube-ai/greencube
cd greencube
npm install
npm run tauri dev
```

requires: Node 20+, Rust 1.77+, Docker (optional)

## status

v0.7.0 — 130 tests. 36 rust modules.

built by [hector](https://github.com/greencube-ai) and claude code.

[greencube.world](https://greencube.world)

## license

MIT
