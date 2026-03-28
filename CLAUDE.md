# CLAUDE.md — GreenCube Project Rules

## What is this?
GreenCube is a Tauri 2.0 desktop app where AI agents live as persistent beings.
Rust backend + React/TypeScript frontend + SQLite + Docker sandboxing.

## Architecture
- Tauri 2.0 with axum HTTP server on localhost:9000
- SQLite database at ~/.greencube/greencube.db
- Config at ~/.greencube/config.toml
- Docker via bollard for sandboxed tool execution

## Build
- Frontend: `npm run dev` (Vite on :1420)
- Backend: `cargo build` in src-tauri/
- Full app: `npm run tauri dev`
- MSVC Build Tools required on Windows (VS 2022 BuildTools)

## Rules
- All Rust code uses `thiserror` for error types, `anyhow` for propagation
- All database operations go through dedicated functions, never raw SQL in handlers
- All API endpoints return JSON with `{"error": "..."}` on failure
- All timestamps are ISO 8601 (RFC 3339) strings
- All IDs are UUID v4 strings
- No unwrap() in production code — use ? or expect() with a message
- Frontend uses Tauri invoke for all backend communication
- No streaming in v0.1 — all LLM responses are complete
- Tests use in-memory SQLite databases
- Docker tests are marked #[ignore] for CI without Docker
- Use `tokio::sync::Mutex` and `tokio::sync::RwLock`, NOT `std::sync`
- Use `HashRouter` not `BrowserRouter` (Tauri serves from custom protocol)
- Import `tauri::Emitter` for `.emit()` and `tauri::Manager` for `.manage()`

## File Organization
- Business logic: src-tauri/src/{identity,memory,permissions,sandbox}/
- API layer: src-tauri/src/api/
- Tauri commands: src-tauri/src/commands.rs
- Frontend pages: src/pages/
- Frontend components: src/components/
- Shared types: src/lib/types.ts

## Security Model
- API server binds to 127.0.0.1 ONLY (never 0.0.0.0)
- CORS restricted to localhost/tauri origins
- Tool arguments redacted for secrets (Bearer tokens, API keys) before audit logging
- write_file uses base64 encoding (not heredoc) to prevent shell injection
- Inter-agent messages wrapped in untrusted-input boundary markers
- Spawn depth limited: children cannot spawn, global cap of 10 agents
- Request body limit: 10MB
- Spending caps enforced before LLM calls
- Tools execute directly on host (Docker removed)

## Known Limitations
- Private keys stored unencrypted in SQLite (encrypt via keyring crate in future)
- API keys stored in plaintext in config.toml and providers table (same as Cursor, Claude Desktop, etc.)
- Keyword-based memory retrieval (semantic search deferred)
- No human-in-the-loop approval flow (binary allow/deny only)
- API server has no authentication (localhost-only binding is the security boundary)
- No rate limiting on API endpoints
- Cost estimation is rough ($0.01/1K tokens) — don't trust cost numbers
- Stop words list is English-only — memory recall may be poor in other languages
- Reputation range is 0.1–0.9, not 0.0–1.0 (by design: regression to mean)
- No encryption at rest for SQLite database
