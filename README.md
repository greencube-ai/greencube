# GreenCube

turn any agent into a persistent agent.

stateless agents forget everything. persistent agents remember, learn, and improve.

## install

mac / linux:
```
curl -sL greencube.world/install.sh | sh
```

windows (powershell):
```
irm greencube.world/install.ps1 | iex
```

## connect your agent

add one line before running your agent:

```
export OPENAI_API_BASE=http://localhost:9000/v1
```

works with openclaw, langchain, crewai, or anything openai compatible.

## what your agent gets

- **remembers** — facts from past tasks persist and get injected into future conversations
- **learns** — extracts structured knowledge from every task automatically
- **self-checks** — reviews its own output and flags when it got something wrong
- **tracks competence** — knows what its good at and where it struggles, per domain
- **adapts** — changes behavior based on learned preferences

## how it works

greencube runs a local server on your machine (localhost:9000). your agent talks to greencube instead of directly to openai. greencube forwards the request, but also remembers, learns, and improves your agent behind the scenes. the response comes back unchanged. your agent has no idea anything happened. it just gets smarter.

everything stays on your machine. nothing goes to the cloud except the AI model calls themselves.

## the numbers

- 143 tests passing
- 45 rust modules
- works on windows + mac
- MIT license
- free forever

[greencube.world](https://greencube.world)

MIT License © 2026 Hector Gras
