# GreenCube

turn any agent into a persistent agent.

stateless agents forget everything. persistent agents remember, learn, and improve.

## install

mac / linux:
```
curl -fsSL https://greencube.world/install.sh | bash
```

windows (powershell):
```
irm greencube.world/install.ps1 | iex
```

## connect your agent

```
export OPENAI_API_BASE=http://localhost:9000/v1
```

works with any OpenAI-compatible API: OpenAI, Ollama, LM Studio, OpenRouter, and any tool that uses the OpenAI SDK format. does not support Anthropic API or Azure OpenAI directly.

## check your agent's brain

```
curl localhost:9000/brain
```

```
🧠 Dev | state: confident | 47 tasks | 84% success
learned:
  • Stripe API needs Bearer auth
  • user prefers short answers
  • pytest over unittest
good at:
  • python       ████░ 87%
  • api          ███░░ 71%
  • css          ██░░░ 43%
improvements:
  • 3 mistakes prevented
  • 47 facts used in tasks
  • 2 corrections applied
recent:
  ✓ 2m ago   "add tests for webhook handlers"
  ✓ 8m ago   "fix stripe signature validation"
  • 15m ago  "refactor settings page layout"
today: ~2400 tokens (~$0.024)
```

other terminal commands:
```
curl localhost:9000/status    # one-line summary
curl localhost:9000/log       # last 20 activities
```

## what it does

- **remembers** — facts from past tasks persist and get injected into future conversations
- **learns** — extracts structured knowledge from every task automatically
- **self-checks** — reviews its own output and flags when it got something wrong
- **prevents mistakes** — catches known error patterns before they reach you
- **tracks competence** — knows what its good at and where it struggles, per domain
- **adapts** — changes behavior based on your feedback (thumbs up/down)

## how it works

greencube runs a local proxy on your machine (localhost:9000). your agent talks to greencube instead of directly to your LLM provider. greencube forwards the request, but also remembers, learns, and improves your agent behind the scenes. everything stays on your machine.

## the numbers

- 142 tests passing
- 45+ rust modules
- works on windows + mac + linux
- MIT license
- free forever, no account, no telemetry

## app

greencube includes a settings app for changing your provider, API key, and model. it runs in the system tray. close the window anytime — the proxy keeps running.

## support

- [GitHub Discussions](https://github.com/greencube-ai/greencube/discussions) — questions, feedback, feature requests
- [GitHub Issues](https://github.com/greencube-ai/greencube/issues) — bug reports

[greencube.world](https://greencube.world)

MIT License © 2026 Hector Gras
