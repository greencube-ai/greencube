# GreenCube

**where AI agents learn from experience**

your agent forgets everything between sessions. GreenCube fixes that.

one env variable. your OpenClaw, LangChain, or CrewAI agent starts extracting knowledge from every task, tracking what it's good at, and reflecting on mistakes. when it keeps failing at something, it creates a specialist to handle it.

## quick start

1. download greencube from [greencube.world](https://greencube.world)
2. add one line before running your agent:
   ```
   export OPENAI_API_BASE=http://localhost:9000/v1
   ```
3. run your agent. greencube handles the rest.

## what your agent gets

- **persistent memory** — facts extracted from every task, injected into future tasks
- **competence tracking** — knows what it's good at (python 91%) and bad at (css 43%)
- **self-verification** — rates its own output and admits when it's wrong
- **docker sandbox** — tool calls run in containers, not on your machine
- **audit trail** — every action logged with timestamps

## alive mode (opt-in)

turn it on in settings. your agent starts:
- reflecting after tasks and extracting structured knowledge
- thinking between tasks about knowledge gaps
- sending you notifications when it notices something important
- spawning specialist children when it's struggling in a domain

creature behaviors like mood shifts, curiosity exploration, and specialist spawning emerge over days of real use. the more you use it, the more alive it gets.

## works with

openclaw / langchain / crewai / anything openai compatible

## build from source

```
git clone https://github.com/greencube-ai/greencube
cd greencube
npm install
npm run tauri dev
```

requires: node 20+, rust 1.77+, docker (optional)

[greencube.world](https://greencube.world) / [github](https://github.com/greencube-ai/greencube)

MIT License © 2026 Hector Gras
