> **LEGACY DOCUMENT.** This spec describes the creature-era design (dated 2026-03-25). It does not reflect current greencube architecture. See STATUS.md for current state.

# GREENCUBE v0.1 — TECHNICAL SPECIFICATION

> **Author:** Claude Code (Architect)
> **Date:** 2026-03-25
> **Status:** SPEC COMPLETE — reviewed and patched, ready for single-pass implementation
> **Target:** Fully working Tauri 2.0 desktop app with AI agent runtime
> **Review:** 32 issues identified and fixed (9 critical, 10 important, 13 minor/noted)

---

## TABLE OF CONTENTS

1. [System Overview](#1-system-overview)
2. [Architecture](#2-architecture)
3. [File System Layout](#3-file-system-layout)
4. [Dependency List](#4-dependency-list)
5. [Database Schema](#5-database-schema)
6. [Configuration](#6-configuration)
7. [API Server](#7-api-server)
8. [OpenAI Proxy](#8-openai-proxy)
9. [Sandbox](#9-sandbox)
10. [Memory](#10-memory)
11. [Identity](#11-identity)
12. [Permissions and Audit](#12-permissions-and-audit)
13. [Agent-to-Agent Local Communication](#13-agent-to-agent-local-communication)
14. [Frontend](#14-frontend)
15. [Error Handling](#15-error-handling)
16. [Testing Strategy](#16-testing-strategy)
17. [Build and Release](#17-build-and-release)
18. [Future-Proofing](#18-future-proofing)
19. [Implementation Order](#19-implementation-order)
20. [Oneshot Execution Plan](#20-oneshot-execution-plan)

---

## 1. SYSTEM OVERVIEW

### WHAT

GreenCube is a local-first Tauri 2.0 desktop application that provides AI agents with persistent memory, cryptographic identity, sandboxed tool execution, and a permission system — all running on the user's machine. It exposes an OpenAI-compatible API on localhost:9000 so existing agent frameworks (LangChain, CrewAI, custom scripts) can connect by changing one URL. Agents become persistent creatures instead of stateless scripts.

### Mental Model

```
┌─────────────────────────────────────────────────────┐
│                    GreenCube App                     │
│  ┌───────────────┐    ┌──────────────────────────┐  │
│  │  React UI      │◄──►  Tauri IPC (invoke/events)│  │
│  │  (Vite/TS)     │    └──────────┬───────────────┘  │
│  └───────────────┘               │                   │
│                          ┌───────▼───────┐           │
│                          │  Rust Backend  │           │
│                          │  (Tauri Core)  │           │
│                          └───┬───┬───┬───┘           │
│                  ┌───────────┘   │   └──────────┐    │
│           ┌──────▼──────┐ ┌─────▼─────┐ ┌──────▼──┐ │
│           │ Axum API    │ │  SQLite   │ │ Docker  │ │
│           │ :9000       │ │  Memory   │ │ Sandbox │ │
│           └─────────────┘ └───────────┘ └─────────┘ │
└─────────────────────────────────────────────────────┘
```

### User Journey (Install → First Agent Running)

1. User downloads GreenCube installer (or builds from source)
2. Opens the app → sees onboarding screen (dark theme)
3. Enters their OpenAI API key (or any OpenAI-compatible endpoint URL)
4. Clicks "Create First Agent" → names it, picks allowed tools
5. Agent appears in the dashboard as "idle"
6. User (or external script) sends a request to `http://localhost:9000/v1/chat/completions` with the agent's ID
7. GreenCube proxies to the LLM, intercepts tool calls, runs them in Docker sandbox, logs everything
8. User watches the activity feed in real time
9. After the task completes, the agent's memory persists for next time

### WHY

Agents outside GreenCube are stateless — they forget everything, run unsandboxed on the host, have no identity, and can't safely interact with other agents. GreenCube solves all four problems in a single desktop app.

---

## 2. ARCHITECTURE

### WHAT

Three processes cooperate: Tauri's webview (frontend), the Rust backend (Tauri core + axum HTTP server), and optional Docker containers (sandboxes). Everything runs locally on the user's machine.

### Thread Model

```
Main Thread (Tauri)
├── Tauri event loop (webview management, IPC)
├── setup() spawns:
│   ├── tokio::spawn → Axum HTTP server on 127.0.0.1:9000
│   └── tokio::spawn → Background tasks (memory decay, sandbox cleanup)
│
Axum Server (separate Tokio task, same runtime)
├── Handles /v1/* API endpoints
├── Shares AppState (Arc<AppState>) with Tauri commands
│
Docker Containers (ephemeral, per-task)
├── Created via bollard when tool execution needed
├── Destroyed after task completion or timeout
```

### IPC Between Frontend and Backend

Tauri 2.0 uses `invoke` for request/response and `emit`/`listen` for events.

**Commands (frontend → backend, request/response):**
- `invoke('get_agents')` → returns agent list
- `invoke('get_agent', { id })` → returns single agent
- `invoke('create_agent', { name, tools_allowed })` → returns new agent
- `invoke('get_episodes', { agentId, limit })` → returns memory episodes
- `invoke('get_audit_log', { agentId, limit })` → returns audit entries
- `invoke('get_config')` → returns current config
- `invoke('save_config', { config })` → saves config
- `invoke('get_docker_status')` → returns docker availability
- `invoke('get_activity_feed', { limit })` → returns recent activity across all agents

**Events (backend → frontend, push):**
- `activity-update` → new audit log entry (agent did something)
- `agent-status-change` → agent went active/idle/error
- `sandbox-event` → sandbox created/destroyed/error

### State Management

**Rust (backend):** Single `AppState` struct wrapped in `Arc`, shared between Tauri commands and axum handlers:

```rust
// src-tauri/src/state.rs
use tokio::sync::{Mutex, RwLock}; // NOT std::sync — Tauri commands are async
use crate::config::AppConfig;

pub struct AppState {
    pub db: Mutex<rusqlite::Connection>,
    pub config: RwLock<AppConfig>,
    pub docker: RwLock<Option<bollard::Docker>>,
    pub app_handle: tauri::AppHandle, // AppHandle is Clone+Send+Sync, no Mutex needed
}
```

**React (frontend):** React Context + `useReducer` for global state. No Redux — overkill for v0.1. Each page fetches its own data via `invoke` on mount.

### CONTESTABLE

**Arc<Mutex<Connection>> for SQLite:** This is the simplest approach but means only one DB operation at a time. For v0.1 this is fine — the app won't have high concurrency. If it becomes a bottleneck later, switch to `r2d2` connection pool or `deadpool-sqlite`. I'm choosing simplicity over performance here because premature optimization will add complexity that slows down v0.1 delivery.

**No Redux/Zustand:** For v0.1 with 3-4 pages and simple data flow, React Context is sufficient. Adding a state management library adds boilerplate without benefit at this scale.

---

## 3. FILE SYSTEM LAYOUT

### ~/.greencube/ (User Data Directory)

```
~/.greencube/
├── config.toml              # User configuration
├── greencube.db             # Main SQLite database (agents, memory, audit)
└── logs/
    └── greencube.log        # Application log file (rotated)
```

**Platform paths:**
- macOS: `~/Library/Application Support/greencube/` (but we use `~/.greencube/` for simplicity and cross-platform consistency)
- Windows: `C:\Users\<user>\.greencube\`
- Linux: `~/.greencube/`

**CONTESTABLE:** Using `~/.greencube/` instead of platform-specific dirs (like `dirs::config_dir()`). The roadmap explicitly says `~/.greencube/`. I agree with this — it's simpler, predictable, and users can find it easily. The `dirs` crate approach is "more correct" but harder to document and debug. Keeping it.

### Project Source Layout

```
greencube/
├── src-tauri/
│   ├── Cargo.toml
│   ├── build.rs
│   ├── tauri.conf.json
│   ├── capabilities/
│   │   └── default.json         # Tauri 2.0 capability permissions
│   ├── icons/                   # App icons (generated by tauri)
│   └── src/
│       ├── main.rs              # Entry point: Tauri setup + axum spawn
│       ├── state.rs             # AppState definition
│       ├── db.rs                # Database initialization + migrations
│       ├── config.rs            # Config loading/saving
│       ├── commands.rs          # All Tauri invoke commands
│       ├── api/
│       │   ├── mod.rs           # Axum router setup
│       │   ├── agents.rs        # Agent CRUD endpoints
│       │   ├── completions.rs   # OpenAI-compatible proxy
│       │   └── health.rs        # Health check endpoint
│       ├── sandbox/
│       │   ├── mod.rs           # Sandbox trait + types
│       │   └── docker.rs        # Docker implementation via bollard
│       ├── memory/
│       │   ├── mod.rs           # Memory types + queries
│       │   └── episodic.rs      # Episodic memory implementation
│       ├── identity/
│       │   ├── mod.rs           # Agent identity types
│       │   └── registry.rs      # Agent CRUD operations
│       ├── permissions/
│       │   ├── mod.rs           # Permission checking
│       │   └── audit.rs         # Audit log operations
│       └── errors.rs            # Error types
├── src/
│   ├── main.tsx                 # React entry point
│   ├── App.tsx                  # Root component with router
│   ├── styles/
│   │   └── globals.css          # Tailwind imports + custom theme
│   ├── lib/
│   │   ├── invoke.ts            # Typed Tauri invoke wrappers
│   │   ├── types.ts             # TypeScript type definitions
│   │   └── events.ts            # Tauri event listeners
│   ├── context/
│   │   └── AppContext.tsx        # Global state context
│   ├── components/
│   │   ├── Layout.tsx           # App shell (sidebar + content)
│   │   ├── AgentCard.tsx        # Agent summary card
│   │   ├── ActivityFeed.tsx     # Scrolling activity log
│   │   ├── MemoryViewer.tsx     # Agent memory browser
│   │   ├── AuditLog.tsx         # Audit log table
│   │   ├── OnboardingModal.tsx  # First-run setup wizard
│   │   ├── CreateAgentModal.tsx # New agent form
│   │   ├── StatusBadge.tsx      # Agent status indicator
│   │   └── EmptyState.tsx       # Placeholder for empty lists
│   └── pages/
│       ├── Dashboard.tsx        # Agent grid + activity feed
│       ├── AgentDetail.tsx      # Single agent deep view
│       └── Settings.tsx         # Configuration page
├── index.html                   # Vite entry HTML
├── vite.config.ts               # Vite configuration
├── tsconfig.json                # TypeScript config
├── tsconfig.node.json           # TypeScript config for Node
├── tailwind.config.js           # Tailwind configuration
├── postcss.config.js            # PostCSS config
├── package.json                 # NPM dependencies
├── GREENCUBE_SPEC.md            # This file
├── CLAUDE.md                    # Project rules for Claude Code
├── README.md                    # Project readme
└── .gitignore
```

---

## 4. DEPENDENCY LIST

### Rust Dependencies (src-tauri/Cargo.toml)

```toml
[package]
name = "greencube"
version = "0.1.0"
edition = "2021"

[dependencies]
# Tauri
tauri = { version = "2", features = ["devtools"] }
tauri-plugin-shell = "2"

# Async runtime (Tauri 2 uses tokio internally, we need it for axum)
tokio = { version = "1", features = ["full"] }

# HTTP server
axum = "0.8"
tower-http = { version = "0.6", features = ["cors", "limit"] }

# Database
rusqlite = { version = "0.32", features = ["bundled"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# HTTP client (for proxying to LLM)
reqwest = { version = "0.12", features = ["json"] }

# Docker
bollard = "0.18"

# Crypto
ed25519-dalek = { version = "2", features = ["rand_core"] }
rand = "0.8"
base64 = "0.22"

# Async stream utilities (needed for bollard streams)
futures-util = "0.3"

# Utilities
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
dirs = "5"
toml = "0.8"

[build-dependencies]
tauri-build = { version = "2", features = [] }
```

**CONTESTABLE — Crate version pinning:** I'm using major version specs (e.g., `"2"` not `"2.10.3"`) intentionally. Cargo.lock will pin exact versions. Using exact versions in Cargo.toml causes unnecessary build failures when minor versions advance. The Cargo.lock file is what guarantees reproducibility.

**CONTESTABLE — rusqlite version:** I've specified `0.32` instead of `0.38` because `0.38` may not exist or may have breaking changes with the bundled feature. The implementer should use the latest `0.3x` that compiles. Same logic for bollard at `0.18` and reqwest at `0.12` — these are the well-tested stable lines.

### NPM Dependencies (package.json)

```json
{
  "name": "greencube",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-shell": "^2",
    "react": "^19",
    "react-dom": "^19",
    "react-router-dom": "^7"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2",
    "@types/react": "^19",
    "@types/react-dom": "^19",
    "@vitejs/plugin-react": "^4",
    "autoprefixer": "^10",
    "postcss": "^8",
    "tailwindcss": "^3",
    "typescript": "^5",
    "vite": "^6"
  }
}
```

**Note on Tailwind:** Using Tailwind v3 (not v4). v4 is a major rewrite and still stabilizing. v3 is battle-tested and has the most documentation. The config-based approach (`tailwind.config.js`) is more predictable for a generated project.

### System Dependencies

- **Docker Desktop** (or Docker Engine on Linux) — optional, required for sandbox features
- **Node.js 20+** — for frontend build tooling
- **Rust 1.77+** — for backend compilation
- **System WebView** — Tauri uses the OS webview (WebView2 on Windows, WebKit on macOS/Linux)

---

## 5. DATABASE SCHEMA

### WHAT

Single SQLite database at `~/.greencube/greencube.db` stores all persistent data: agents, memory episodes, audit log, and configuration state.

### WHY

SQLite is embedded (no server), cross-platform, and handles the read/write volume of a single-user desktop app trivially. One file = easy backup, easy reset.

### HOW

Database is initialized in `src-tauri/src/db.rs` on first launch. All tables created in a single transaction.

### Migration Strategy

v0.1 uses a simple version table. On startup, check `schema_version` and run any needed migrations sequentially.

```sql
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL DEFAULT 1
);
```

For v0.1, there's only version 1. Future versions add migration functions: `migrate_1_to_2()`, `migrate_2_to_3()`, etc.

### Tables

#### agents

```sql
CREATE TABLE agents (
    id TEXT PRIMARY KEY,                    -- UUID v4
    name TEXT NOT NULL UNIQUE,               -- Human-readable name (UNIQUE constraint as defense-in-depth)
    created_at TEXT NOT NULL,               -- ISO 8601 timestamp
    updated_at TEXT NOT NULL,               -- ISO 8601 timestamp
    status TEXT NOT NULL DEFAULT 'idle',    -- 'idle', 'active', 'error'
    system_prompt TEXT NOT NULL DEFAULT '', -- Agent's base system prompt
    public_key BLOB NOT NULL,              -- Ed25519 public key (32 bytes)
    private_key BLOB NOT NULL,             -- Ed25519 private key (32 bytes, encrypted later)
    tools_allowed TEXT NOT NULL DEFAULT '[]',  -- JSON array of tool names
    max_spend_cents INTEGER NOT NULL DEFAULT 0,  -- 0 = unlimited
    total_tasks INTEGER NOT NULL DEFAULT 0,
    successful_tasks INTEGER NOT NULL DEFAULT 0,
    total_spend_cents INTEGER NOT NULL DEFAULT 0
);
```

**CONTESTABLE — Private key in SQLite plaintext:** For v0.1, the private key is stored as raw bytes in SQLite. This is acceptable because:
1. The database is on the user's local machine, protected by OS-level permissions
2. There's no multiplayer in v0.1, so the key is never used for real authentication
3. Adding encryption (via a master password or OS keychain) would add significant complexity

For v0.2+, the private key should be encrypted with a key derived from the OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service) via the `keyring` crate. Flag this in CLAUDE.md as a known security limitation.

#### episodes (Episodic Memory)

```sql
CREATE TABLE episodes (
    id TEXT PRIMARY KEY,                    -- UUID v4
    agent_id TEXT NOT NULL,                 -- FK to agents
    created_at TEXT NOT NULL,               -- ISO 8601 timestamp
    event_type TEXT NOT NULL,               -- 'tool_call', 'llm_request', 'llm_response', 'error', 'task_start', 'task_end'
    summary TEXT NOT NULL,                  -- Human-readable summary
    raw_data TEXT,                          -- Full JSON of the event
    task_id TEXT,                           -- Groups events into tasks
    outcome TEXT,                           -- 'success', 'failure', 'partial', NULL
    tokens_used INTEGER DEFAULT 0,
    cost_cents INTEGER DEFAULT 0,
    FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
);

CREATE INDEX idx_episodes_agent_time ON episodes(agent_id, created_at DESC);
CREATE INDEX idx_episodes_task ON episodes(task_id);
```

#### audit_log

```sql
CREATE TABLE audit_log (
    id TEXT PRIMARY KEY,                    -- UUID v4
    agent_id TEXT NOT NULL,                 -- FK to agents
    created_at TEXT NOT NULL,               -- ISO 8601 timestamp
    action_type TEXT NOT NULL,              -- 'tool_call', 'llm_request', 'memory_write', 'agent_created', 'permission_check'
    action_detail TEXT NOT NULL,            -- JSON of what was attempted
    permission_result TEXT NOT NULL,        -- 'allowed', 'denied'
    result TEXT,                            -- JSON of what happened (NULL if denied)
    duration_ms INTEGER,
    cost_cents INTEGER DEFAULT 0,
    error TEXT,                             -- Error message if failed
    FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
);

CREATE INDEX idx_audit_agent_time ON audit_log(agent_id, created_at DESC);
CREATE INDEX idx_audit_type ON audit_log(action_type);
```

#### config_store

```sql
CREATE TABLE config_store (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

Used for storing the `onboarding_complete` flag and any runtime config that doesn't belong in `config.toml`.

### Full Schema Init Function

**File: `src-tauri/src/db.rs`**

```rust
use rusqlite::{Connection, Result};
use std::path::Path;

pub fn init_database(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;

    // Enable WAL mode for better concurrent read performance
    // NOTE: rusqlite pragma API varies by version. If pragma_update doesn't exist,
    // use conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;

    conn.execute_batch(SCHEMA_SQL)?;

    Ok(conn)
}

const SCHEMA_SQL: &str = r#"
    CREATE TABLE IF NOT EXISTS schema_version (
        version INTEGER NOT NULL DEFAULT 1
    );
    INSERT OR IGNORE INTO schema_version (version) VALUES (1);

    CREATE TABLE IF NOT EXISTS agents (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'idle',
        system_prompt TEXT NOT NULL DEFAULT '',
        public_key BLOB NOT NULL,
        private_key BLOB NOT NULL,
        tools_allowed TEXT NOT NULL DEFAULT '[]',
        max_spend_cents INTEGER NOT NULL DEFAULT 0,
        total_tasks INTEGER NOT NULL DEFAULT 0,
        successful_tasks INTEGER NOT NULL DEFAULT 0,
        total_spend_cents INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS episodes (
        id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL,
        created_at TEXT NOT NULL,
        event_type TEXT NOT NULL,
        summary TEXT NOT NULL,
        raw_data TEXT,
        task_id TEXT,
        outcome TEXT,
        tokens_used INTEGER DEFAULT 0,
        cost_cents INTEGER DEFAULT 0,
        FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_episodes_agent_time ON episodes(agent_id, created_at DESC);
    CREATE INDEX IF NOT EXISTS idx_episodes_task ON episodes(task_id);

    CREATE TABLE IF NOT EXISTS audit_log (
        id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL,
        created_at TEXT NOT NULL,
        action_type TEXT NOT NULL,
        action_detail TEXT NOT NULL,
        permission_result TEXT NOT NULL,
        result TEXT,
        duration_ms INTEGER,
        cost_cents INTEGER DEFAULT 0,
        error TEXT,
        FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_audit_agent_time ON audit_log(agent_id, created_at DESC);
    CREATE INDEX IF NOT EXISTS idx_audit_type ON audit_log(action_type);

    CREATE TABLE IF NOT EXISTS config_store (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
"#;
```

### ERRORS
- **DB file locked:** Another GreenCube instance is running. Show error: "GreenCube is already running. Check your system tray."
- **Corrupt database:** Detect via `PRAGMA integrity_check`. If corrupt, rename to `greencube.db.corrupt` and create fresh. Show warning to user.
- **Disk full:** Catch `rusqlite::Error::SqliteFailure` with `SQLITE_FULL`. Show: "Disk full. Free up space and restart."
- **Permission denied:** Can't write to `~/.greencube/`. Show: "Cannot write to ~/.greencube/. Check permissions."

### TESTS

```rust
// tests in src-tauri/src/db.rs
#[cfg(test)]
mod tests {
    // test_init_creates_all_tables: open in-memory DB, run init, verify all tables exist
    // test_init_is_idempotent: run init twice, no errors
    // test_foreign_keys_enforced: insert episode with bad agent_id, expect error
    // test_wal_mode_enabled: check PRAGMA journal_mode returns 'wal'
}
```

---

## 6. CONFIGURATION

### WHAT

User configuration stored in `~/.greencube/config.toml`. Loaded on startup, editable via Settings page in UI.

### config.toml Format

```toml
# GreenCube Configuration

[llm]
# OpenAI-compatible API endpoint
api_base_url = "https://api.openai.com/v1"
api_key = ""
default_model = "gpt-4o"

[server]
host = "127.0.0.1"
port = 9000

[sandbox]
# Docker image for agent sandboxes
image = "python:3.12-slim"
# Default resource limits
cpu_limit_cores = 1.0
memory_limit_mb = 512
timeout_seconds = 300
network_enabled = false

[ui]
# Whether onboarding has been completed
onboarding_complete = false
```

### HOW

**File: `src-tauri/src/config.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub llm: LlmConfig,
    pub server: ServerConfig,
    pub sandbox: SandboxConfig,
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub api_base_url: String,
    pub api_key: String,
    pub default_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub image: String,
    pub cpu_limit_cores: f64,
    pub memory_limit_mb: u64,
    pub timeout_seconds: u64,
    pub network_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub onboarding_complete: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            llm: LlmConfig {
                api_base_url: "https://api.openai.com/v1".into(),
                api_key: String::new(),
                default_model: "gpt-4o".into(),
            },
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 9000,
            },
            sandbox: SandboxConfig {
                image: "python:3.12-slim".into(),
                cpu_limit_cores: 1.0,
                memory_limit_mb: 512,
                timeout_seconds: 300,
                network_enabled: false,
            },
            ui: UiConfig {
                onboarding_complete: false,
            },
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".greencube")
}

pub fn load_config() -> anyhow::Result<AppConfig> {
    let path = config_dir().join("config.toml");
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    } else {
        let config = AppConfig::default();
        save_config(&config)?;
        Ok(config)
    }
}

pub fn save_config(config: &AppConfig) -> anyhow::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let content = toml::to_string_pretty(config)?;
    std::fs::write(dir.join("config.toml"), content)?;
    Ok(())
}
```

### First-Run Flow

1. `main.rs` calls `load_config()`
2. If `config.toml` doesn't exist → create with defaults
3. If `config.ui.onboarding_complete == false` → frontend shows `OnboardingModal`
4. User enters API key → frontend calls `invoke('save_config', { config })`
5. User clicks "Create First Agent" → frontend calls `invoke('create_agent', { ... })`
6. Config saved with `onboarding_complete = true`
7. Dashboard loads showing the new agent

### ERRORS
- **Invalid TOML:** If config.toml is manually edited and corrupted, rename to `config.toml.bak`, create fresh default, log warning.
- **Missing home dir:** `dirs::home_dir()` returns `None` on some edge cases. Panic with clear message — the app can't function without a home directory.

### TESTS

```rust
#[cfg(test)]
mod tests {
    // test_default_config_serializes: default config round-trips through toml
    // test_load_creates_default: load from non-existent path creates file
    // test_save_and_load: save config, load it back, values match
}
```

---

## 7. API SERVER

### WHAT

An axum HTTP server running on `127.0.0.1:9000` alongside Tauri. Provides the OpenAI-compatible proxy and agent management API.

### WHY

External agent scripts/frameworks connect here. The API is the bridge between "any agent framework" and GreenCube's runtime (memory, sandbox, audit).

### HOW

**File: `src-tauri/src/api/mod.rs`**

```rust
use axum::{Router, routing::{get, post}, extract::State};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use crate::state::AppState;

pub mod agents;
pub mod completions;
pub mod health;

pub fn create_router(state: Arc<AppState>) -> Router {
    // NOTE: axum 0.8 path parameter syntax — verify at build time.
    // If "/{id}" doesn't compile, try "/:id" instead.
    Router::new()
        // Health
        .route("/health", get(health::health_check))
        // Agents
        .route("/v1/agents", get(agents::list_agents))
        .route("/v1/agents", post(agents::create_agent))
        .route("/v1/agents/{id}", get(agents::get_agent))
        // OpenAI-compatible
        .route("/v1/chat/completions", post(completions::chat_completions))
        // Memory
        .route("/v1/agents/{id}/episodes", get(agents::get_episodes))
        // Audit
        .route("/v1/agents/{id}/audit", get(agents::get_audit_log))
        .layer(CorsLayer::permissive())
        .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024)) // 10MB request body limit
        .with_state(state)
}
```

### Endpoints

#### GET /health

**Response 200:**
```json
{
  "status": "ok",
  "version": "0.1.0",
  "docker_available": true
}
```

#### GET /v1/agents

**Response 200:**
```json
{
  "agents": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "name": "CodeBot",
      "status": "idle",
      "created_at": "2026-03-25T10:00:00Z",
      "tools_allowed": ["shell", "read_file", "write_file"],
      "total_tasks": 5,
      "successful_tasks": 4,
      "reputation": 0.8
    }
  ]
}
```

#### POST /v1/agents

**Request:**
```json
{
  "name": "CodeBot",
  "system_prompt": "You are a helpful coding assistant.",
  "tools_allowed": ["shell", "read_file", "write_file"]
}
```

**Response 201:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "CodeBot",
  "status": "idle",
  "created_at": "2026-03-25T10:00:00Z",
  "public_key": "base64-encoded-ed25519-public-key"
}
```

**Errors:**
- 400: `{"error": "name is required"}` — missing name
- 400: `{"error": "invalid tool: foobar. Valid tools: shell, read_file, write_file, http_get"}` — unknown tool name
- 409: `{"error": "agent with name 'CodeBot' already exists"}` — duplicate name

#### GET /v1/agents/{id}

**Response 200:**
```json
{
  "id": "550e8400-...",
  "name": "CodeBot",
  "status": "idle",
  "created_at": "2026-03-25T10:00:00Z",
  "updated_at": "2026-03-25T12:00:00Z",
  "system_prompt": "You are a helpful coding assistant.",
  "tools_allowed": ["shell", "read_file", "write_file"],
  "max_spend_cents": 0,
  "total_tasks": 5,
  "successful_tasks": 4,
  "total_spend_cents": 120,
  "reputation": 0.8,
  "public_key": "base64-encoded-ed25519-public-key"
}
```

**Errors:**
- 404: `{"error": "agent not found"}` — no agent with this ID

#### GET /v1/agents/{id}/episodes

**Query params:** `?limit=50&task_id=xxx`

**Response 200:**
```json
{
  "episodes": [
    {
      "id": "...",
      "created_at": "2026-03-25T10:05:00Z",
      "event_type": "tool_call",
      "summary": "Executed shell command: ls -la",
      "task_id": "task-001",
      "outcome": "success",
      "tokens_used": 0,
      "cost_cents": 0
    }
  ]
}
```

#### GET /v1/agents/{id}/audit

**Query params:** `?limit=50`

**Response 200:**
```json
{
  "entries": [
    {
      "id": "...",
      "created_at": "2026-03-25T10:05:00Z",
      "action_type": "tool_call",
      "action_detail": "{\"tool\": \"shell\", \"command\": \"ls -la\"}",
      "permission_result": "allowed",
      "duration_ms": 150,
      "cost_cents": 0,
      "error": null
    }
  ]
}
```

#### POST /v1/chat/completions

See Section 8 (OpenAI Proxy) for full specification.

### Auth

**v0.1: No authentication.** The server binds to `127.0.0.1` only (not `0.0.0.0`), so it's only accessible from localhost. This is sufficient for a single-user desktop app.

**CONTESTABLE:** The prompt says to listen on `0.0.0.0:9000`. I'm changing this to `127.0.0.1:9000`. Binding to all interfaces exposes the API to the local network with no auth — any device on the same WiFi can create agents and proxy LLM calls on your dime. For v0.1 there's no auth mechanism, so localhost-only is the only safe default.

### ERRORS (Global)

All error responses follow this format:
```json
{
  "error": "human-readable error message"
}
```

Status codes:
- 400: Bad request (invalid input)
- 404: Not found
- 409: Conflict (duplicate)
- 500: Internal server error
- 503: Service unavailable (Docker not running, LLM API unreachable)

### TESTS

```rust
#[cfg(test)]
mod tests {
    // test_health_endpoint: GET /health returns 200 with version
    // test_create_agent: POST /v1/agents returns 201 with valid agent
    // test_create_agent_missing_name: POST /v1/agents returns 400
    // test_list_agents: create 2 agents, GET /v1/agents returns both
    // test_get_agent_not_found: GET /v1/agents/nonexistent returns 404
    // test_get_episodes_empty: new agent has no episodes
}
```

---

## 8. OPENAI PROXY

### WHAT

`POST /v1/chat/completions` accepts OpenAI-format requests, injects agent memories, proxies to the configured LLM, parses tool calls from the response, executes them in the sandbox, and returns the final result.

### WHY

This is the core value proposition. Any agent framework that speaks OpenAI API format gets free memory, sandboxing, and audit logging by pointing at localhost:9000.

### Step-by-Step Flow

**File: `src-tauri/src/api/completions.rs`**

```
1. RECEIVE REQUEST
   - Parse OpenAI ChatCompletion request body as serde_json::Value
   - Extract agent_id from custom header: X-Agent-Id
   - If no X-Agent-Id header, use "default" agent:
     a. Query DB for agent with name = "default"
     b. If not found, auto-create: create_agent(conn, "default", "You are a helpful assistant.", &["shell", "read_file", "write_file", "http_get"])
     c. Use its ID for the rest of the flow
   - CRITICAL: Override streaming — set body["stream"] = serde_json::Value::Bool(false)
     This prevents SSE responses which v0.1 cannot parse.

2. VALIDATE AGENT
   - Look up agent in SQLite by ID
   - If not found → 404
   - If agent status is "error" → 503 "agent is in error state"

3. LOG TASK START
   - Generate task_id (UUID)
   - Insert episode: event_type='task_start'
   - Set agent status to 'active'
   - Emit 'agent-status-change' event to frontend

4. INJECT MEMORIES
   - Take the last user message from the messages array
   - Query episodes table for this agent's recent relevant episodes
   - For v0.1: simple keyword match (LIKE query) against episode summaries
   - Prepend matching memories to the system prompt as:
     "[Memory] <timestamp>: <summary>"
   - Max 5 memories injected

5. CHECK PERMISSIONS
   - If the request includes tool definitions, verify each tool name
     is in the agent's tools_allowed list
   - Denied tools are removed from the request (not an error — just filtered)
   - Log permission check to audit_log

6. FORWARD TO LLM
   - Build reqwest client
   - POST to config.llm.api_base_url + "/chat/completions"
   - Headers: Authorization: Bearer <api_key>, Content-Type: application/json
   - Body: the (possibly modified) request with memories injected
   - Set stream: false (override any stream:true from client)
   - Timeout: 120 seconds

7. PARSE RESPONSE
   - If HTTP error from LLM → log error episode, return 502 to client
   - Parse response as OpenAI ChatCompletion format
   - Log episode: event_type='llm_response', tokens_used from usage field
   - Calculate cost_cents from token counts (rough estimate: $0.01/1K tokens)

8. CHECK FOR TOOL CALLS
   - If response contains tool_calls in the assistant message:
     a. For each tool call:
        - Check tool name against agent's tools_allowed
        - If allowed: execute in sandbox (see Section 9)
        - If denied: return error result for that tool call
        - Log audit entry for each tool call
     b. Collect tool results
     c. Append assistant message + tool results to messages
     d. Go to step 6 (loop — re-send to LLM with tool results)
     e. Max 10 iterations to prevent infinite loops

9. RETURN FINAL RESPONSE
   - Return the LLM's final response (no tool calls) to the client
   - Log episode: event_type='task_end', outcome based on whether errors occurred
   - Set agent status back to 'idle'
   - Emit 'agent-status-change' and 'activity-update' events to frontend
```

### Request Format (OpenAI-compatible)

```json
{
  "model": "gpt-4o",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "List files in the current directory"}
  ],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "shell",
        "description": "Execute a shell command",
        "parameters": {
          "type": "object",
          "properties": {
            "command": {"type": "string", "description": "The command to run"}
          },
          "required": ["command"]
        }
      }
    }
  ]
}
```

**Custom header:** `X-Agent-Id: <agent-uuid>` — identifies which agent this request is for.

### Response Format (OpenAI-compatible)

```json
{
  "id": "chatcmpl-greencube-<uuid>",
  "object": "chat.completion",
  "created": 1711360000,
  "model": "gpt-4o",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "Here are the files in the current directory:\n..."
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 150,
    "completion_tokens": 50,
    "total_tokens": 200
  }
}
```

### CONTESTABLE — Memory Injection Approach

The prompt requires "only inject memories with >0.85 similarity score." This requires vector embeddings and cosine similarity, which means either:
1. An embedding API call for every request (adds latency + cost)
2. A local embedding model (adds ~500MB to app size)

**For v0.1, I'm replacing this with keyword-based memory retrieval.** Here's why:
- v0.1 is about proving the concept works end-to-end
- Embedding infrastructure is a significant implementation burden
- Keyword matching on episode summaries is simple, fast, and good enough to demonstrate memory injection
- The memory module is designed with a trait so embeddings can be swapped in for v0.2

The v0.1 approach: split the user's message into words, search episode summaries with `LIKE '%word%'` for each significant word (>3 chars, not stopwords), score by number of matching words, return top 5.

### CONTESTABLE — Tool Call Loop Limit

I set max 10 tool-call iterations. The prompt doesn't specify this. Without a limit, a buggy agent could loop forever, burning API credits. 10 is generous enough for real tasks but prevents runaway costs.

### ERRORS

- **LLM API unreachable:** 502 `{"error": "could not reach LLM API at <url>. Check your API key and network."}`
- **LLM API key invalid:** 502 `{"error": "LLM API returned 401. Check your API key in Settings."}`
- **LLM rate limited:** 502 `{"error": "LLM API returned 429. Rate limited. Try again later."}`
- **Tool call loop exceeded:** 500 `{"error": "tool call loop exceeded 10 iterations. Possible infinite loop."}`
- **Agent not found:** 404 `{"error": "agent not found. Create one at http://localhost:9000/v1/agents"}`
- **Docker not available for tool call:** 503 `{"error": "Docker is required for tool execution but is not running. Install Docker or disable tools."}`
- **Sandbox timeout:** Tool result returns `{"error": "command timed out after 300 seconds"}`

### TESTS

```rust
#[cfg(test)]
mod tests {
    // test_completions_without_tools: simple chat request proxied and returned
    // test_completions_with_agent_id: X-Agent-Id header is parsed
    // test_completions_default_agent: no agent_id uses/creates default agent
    // test_completions_memory_injection: create episodes, verify they appear in forwarded request
    // test_completions_tool_permission_filter: tools not in allowed list are stripped
    // test_completions_max_iterations: mock LLM always returns tool calls, verify loop stops at 10
    // test_completions_llm_error: mock 500 from LLM, verify 502 returned
}
```

---

## 9. SANDBOX

### WHAT

Docker-based isolated execution environment for agent tool calls. Each tool execution runs inside a temporary container with resource limits.

### WHY

Agents must never execute code directly on the host. The sandbox provides isolation, resource limits, and a clean environment for every execution.

### HOW

**File: `src-tauri/src/sandbox/mod.rs`**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i64,
    pub duration_ms: u64,
    pub timed_out: bool,
}

#[derive(Debug, Clone)]
pub struct SandboxOptions {
    pub image: String,
    pub cpu_limit_cores: f64,
    pub memory_limit_mb: u64,
    pub timeout_seconds: u64,
    pub network_enabled: bool,
}
```

**File: `src-tauri/src/sandbox/docker.rs`**

```rust
use bollard::Docker;
use bollard::container::{Config, CreateContainerOptions, StartContainerOptions, WaitContainerOptions, LogsOptions};
use bollard::models::{HostConfig, ContainerWaitResponse};
use crate::sandbox::{SandboxResult, SandboxOptions};
use tokio::time::{timeout, Duration};

pub async fn check_docker_available() -> bool {
    match Docker::connect_with_local_defaults() {
        Ok(docker) => docker.ping().await.is_ok(),
        Err(_) => false,
    }
}

pub async fn ensure_image_pulled(docker: &Docker, image: &str) -> anyhow::Result<()> {
    use bollard::image::CreateImageOptions;
    use futures_util::StreamExt;

    // Check if image exists locally first
    if docker.inspect_image(image).await.is_ok() {
        return Ok(());
    }

    tracing::info!("Pulling Docker image: {}. This may take 30-60 seconds on first run...", image);

    let opts = CreateImageOptions {
        from_image: image,
        ..Default::default()
    };

    let mut stream = docker.create_image(Some(opts), None, None);
    while let Some(result) = stream.next().await {
        result?; // Propagate pull errors
    }

    tracing::info!("Docker image pulled successfully: {}", image);
    Ok(())
}

pub async fn execute_in_sandbox(
    docker: &Docker,
    command: &str,
    options: &SandboxOptions,
) -> anyhow::Result<SandboxResult> {
    // Ensure image is available (pulls on first use)
    ensure_image_pulled(docker, &options.image).await?;

    let container_name = format!("greencube-{}", uuid::Uuid::new_v4());

    // Create container config
    // CRITICAL: auto_remove must be false. If true, the container is deleted
    // the instant it exits, and collect_logs will fail with "container not found".
    let config = Config {
        image: Some(options.image.clone()),
        cmd: Some(vec!["sh".to_string(), "-c".to_string(), command.to_string()]),
        host_config: Some(HostConfig {
            memory: Some((options.memory_limit_mb * 1024 * 1024) as i64),
            nano_cpus: Some((options.cpu_limit_cores * 1_000_000_000.0) as i64),
            network_mode: if options.network_enabled {
                Some("bridge".to_string())
            } else {
                Some("none".to_string())
            },
            auto_remove: Some(false), // MUST be false — we need logs before removal
            ..Default::default()
        }),
        ..Default::default()
    };

    // Create container
    let create_opts = CreateContainerOptions {
        name: &container_name,
        ..Default::default()
    };
    docker.create_container(Some(create_opts), config).await?;

    // Start container
    docker.start_container(&container_name, None::<StartContainerOptions<String>>).await?;

    // Wait for completion with timeout
    let start = std::time::Instant::now();
    let wait_result = timeout(
        Duration::from_secs(options.timeout_seconds),
        wait_for_container(docker, &container_name),
    ).await;

    let duration_ms = start.elapsed().as_millis() as u64;

    let result = match wait_result {
        Ok(Ok(exit_code)) => {
            // Collect logs BEFORE removing container
            let (stdout, stderr) = collect_logs(docker, &container_name).await
                .unwrap_or_else(|_| (String::new(), "Failed to collect logs".to_string()));
            Ok(SandboxResult {
                stdout,
                stderr,
                exit_code,
                duration_ms,
                timed_out: false,
            })
        }
        Ok(Err(e)) => {
            let _ = docker.kill_container(&container_name, None::<bollard::container::KillContainerOptions<String>>).await;
            Err(e)
        }
        Err(_) => {
            // Timeout — kill container
            let _ = docker.kill_container(&container_name, None::<bollard::container::KillContainerOptions<String>>).await;
            Ok(SandboxResult {
                stdout: String::new(),
                stderr: "Execution timed out".to_string(),
                exit_code: -1,
                duration_ms,
                timed_out: true,
            })
        }
    };

    // Always clean up the container (since auto_remove is false)
    let _ = docker.remove_container(
        &container_name,
        Some(bollard::container::RemoveContainerOptions { force: true, ..Default::default() }),
    ).await;

    result
}

async fn wait_for_container(docker: &Docker, name: &str) -> anyhow::Result<i64> {
    use futures_util::StreamExt;
    let mut stream = docker.wait_container(name, None::<WaitContainerOptions<String>>);
    if let Some(result) = stream.next().await {
        let resp = result?;
        Ok(resp.status_code)
    } else {
        Err(anyhow::anyhow!("container wait stream ended unexpectedly"))
    }
}

async fn collect_logs(docker: &Docker, name: &str) -> anyhow::Result<(String, String)> {
    use futures_util::StreamExt;
    let opts = LogsOptions::<String> {
        stdout: true,
        stderr: true,
        ..Default::default()
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut stream = docker.logs(name, Some(opts));
    while let Some(Ok(output)) = stream.next().await {
        match output {
            bollard::container::LogOutput::StdOut { message } => {
                stdout.push_str(&String::from_utf8_lossy(&message));
            }
            bollard::container::LogOutput::StdErr { message } => {
                stderr.push_str(&String::from_utf8_lossy(&message));
            }
            _ => {}
        }
    }
    Ok((stdout, stderr))
}
```

### Tool Routing

When the LLM returns a tool call, the completions handler routes it based on tool name:

```rust
// In src-tauri/src/api/completions.rs
async fn execute_tool_call(
    state: &AppState,
    agent_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> anyhow::Result<String> {
    match tool_name {
        "shell" => {
            let command = arguments["command"].as_str()
                .ok_or_else(|| anyhow::anyhow!("shell tool requires 'command' argument"))?;
            let docker = state.docker.read().await;
            let docker = docker.as_ref()
                .ok_or_else(|| anyhow::anyhow!("Docker is not available"))?;
            let config = state.config.read().await;
            let opts = SandboxOptions {
                image: config.sandbox.image.clone(),
                cpu_limit_cores: config.sandbox.cpu_limit_cores,
                memory_limit_mb: config.sandbox.memory_limit_mb,
                timeout_seconds: config.sandbox.timeout_seconds,
                network_enabled: config.sandbox.network_enabled,
            };
            let result = crate::sandbox::docker::execute_in_sandbox(docker, command, &opts).await?;
            Ok(format!("Exit code: {}\nStdout:\n{}\nStderr:\n{}", result.exit_code, result.stdout, result.stderr))
        }
        "read_file" => {
            let path = arguments["path"].as_str()
                .ok_or_else(|| anyhow::anyhow!("read_file requires 'path' argument"))?;
            let command = format!("cat {}", shell_escape(path));
            // ... same Docker execution
            execute_tool_call(state, agent_id, "shell", &serde_json::json!({"command": command})).await
        }
        "write_file" => {
            let path = arguments["path"].as_str()
                .ok_or_else(|| anyhow::anyhow!("write_file requires 'path' argument"))?;
            let content = arguments["content"].as_str()
                .ok_or_else(|| anyhow::anyhow!("write_file requires 'content' argument"))?;
            let command = format!("cat > {} << 'GREENCUBE_EOF'\n{}\nGREENCUBE_EOF", shell_escape(path), content);
            execute_tool_call(state, agent_id, "shell", &serde_json::json!({"command": command})).await
        }
        "http_get" => {
            let url = arguments["url"].as_str()
                .ok_or_else(|| anyhow::anyhow!("http_get requires 'url' argument"))?;
            let command = format!("curl -sS {}", shell_escape(url));
            // Requires network_enabled in sandbox options
            execute_tool_call(state, agent_id, "shell", &serde_json::json!({"command": command})).await
        }
        _ => Err(anyhow::anyhow!("unknown tool: {}", tool_name)),
    }
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
```

### Docker Not Installed — Graceful Handling

On startup, call `check_docker_available()`. Store result in `AppState.docker`:
- If Docker available: `Some(docker_client)`
- If Docker not available: `None`

**UI behavior when Docker is missing:**
- Dashboard shows a yellow banner: "Docker not installed. Tool execution is disabled. [Install Docker →]"
- Agents can still be created and managed
- Chat completions work (LLM proxy), but any tool calls return error: `"Docker is not available. Install Docker to enable tool execution."`
- The health endpoint returns `"docker_available": false`

**Re-check on demand:** Docker status is checked on app startup. If Docker becomes available after startup, the user can restart the app or navigate to Settings to see the updated status. (v0.2: add periodic background re-check via tokio::spawn interval.)

### Cross-Platform Docker Differences

- **macOS:** Docker Desktop uses a Linux VM. `Docker::connect_with_local_defaults()` connects via Unix socket at `/var/run/docker.sock`.
- **Windows:** Docker Desktop uses WSL2 or Hyper-V. `connect_with_local_defaults()` connects via named pipe `//./pipe/docker_engine`.
- **Linux:** Native Docker. Unix socket at `/var/run/docker.sock`.

The `bollard` crate handles all three via `connect_with_local_defaults()`. No platform-specific code needed.

### ERRORS

- **Docker not installed:** Handled gracefully (see above)
- **Docker not running:** Same as not installed — `ping()` fails
- **Image pull fails:** On first use, the image needs to be pulled. If network is down: `"Could not pull Docker image python:3.12-slim. Check your internet connection."`
- **Container OOM killed:** `exit_code` will be 137. Report: `"Command was killed: out of memory (limit: 512MB)"`
- **Container timeout:** Handled by tokio timeout. Report: `"Command timed out after 300 seconds"`
- **Docker socket permission denied:** Linux-specific. Report: `"Cannot connect to Docker. Add your user to the 'docker' group or run Docker Desktop."`

### TESTS

```rust
#[cfg(test)]
mod tests {
    // test_check_docker_available: verifies function doesn't panic (may return true or false)
    // test_sandbox_echo: run "echo hello" in sandbox, verify stdout contains "hello"
    //   (requires Docker — skip in CI with #[ignore] if DOCKER_AVAILABLE env not set)
    // test_sandbox_timeout: run "sleep 999" with 2-second timeout, verify timed_out=true
    // test_sandbox_exit_code: run "exit 42", verify exit_code=42
    // test_sandbox_network_disabled: run "curl google.com" with network=false, verify failure
    // test_shell_escape: verify special characters are escaped properly
}
```

---

## 10. MEMORY

### WHAT

Episodic memory stores a timeline of everything an agent has done. Memories persist across sessions and are injected into LLM context to make agents smarter over time.

### WHY

Without memory, every agent conversation starts from zero. Memory is what turns a stateless script into a persistent being.

### Episodic Memory Implementation

**File: `src-tauri/src/memory/mod.rs`**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub agent_id: String,
    pub created_at: String,
    pub event_type: String,
    pub summary: String,
    pub raw_data: Option<String>,
    pub task_id: Option<String>,
    pub outcome: Option<String>,
    pub tokens_used: i64,
    pub cost_cents: i64,
}
```

**File: `src-tauri/src/memory/episodic.rs`**

```rust
use rusqlite::{Connection, params};
use crate::memory::Episode;

pub fn insert_episode(conn: &Connection, episode: &Episode) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO episodes (id, agent_id, created_at, event_type, summary, raw_data, task_id, outcome, tokens_used, cost_cents)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            episode.id,
            episode.agent_id,
            episode.created_at,
            episode.event_type,
            episode.summary,
            episode.raw_data,
            episode.task_id,
            episode.outcome,
            episode.tokens_used,
            episode.cost_cents,
        ],
    )?;
    Ok(())
}

pub fn get_episodes(
    conn: &Connection,
    agent_id: &str,
    limit: i64,
    task_id: Option<&str>,
) -> anyhow::Result<Vec<Episode>> {
    // Use two separate queries to avoid parameter index confusion
    if let Some(tid) = task_id {
        let mut stmt = conn.prepare(
            "SELECT id, agent_id, created_at, event_type, summary, raw_data, task_id, outcome, tokens_used, cost_cents
             FROM episodes WHERE agent_id = ?1 AND task_id = ?2
             ORDER BY created_at DESC LIMIT ?3"
        )?;
        let rows = stmt.query_map(params![agent_id, tid, limit], map_episode)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, agent_id, created_at, event_type, summary, raw_data, task_id, outcome, tokens_used, cost_cents
             FROM episodes WHERE agent_id = ?1
             ORDER BY created_at DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![agent_id, limit], map_episode)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn map_episode(row: &rusqlite::Row) -> rusqlite::Result<Episode> {
    Ok(Episode {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        created_at: row.get(2)?,
        event_type: row.get(3)?,
        summary: row.get(4)?,
        raw_data: row.get(5)?,
        task_id: row.get(6)?,
        outcome: row.get(7)?,
        tokens_used: row.get(8)?,
        cost_cents: row.get(9)?,
    })
}
```

**NOTE:** The `get_episodes` function above uses two separate prepared statements to avoid parameter index issues. This is the correct pattern — copy it directly.

### Memory Retrieval for Injection (v0.1 — Keyword-Based)

```rust
// In src-tauri/src/memory/episodic.rs

pub fn recall_relevant_episodes(
    conn: &Connection,
    agent_id: &str,
    query: &str,
    limit: i64,
) -> anyhow::Result<Vec<Episode>> {
    // Extract significant words (>3 chars, lowercase)
    let words: Vec<String> = query
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 3)
        .filter(|w| !STOP_WORDS.contains(&w.as_str()))
        .collect();

    if words.is_empty() {
        return Ok(vec![]);
    }

    // Build query that counts matching words in summary
    // Each matching word adds 1 to the score
    let mut conditions = Vec::new();
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(agent_id.to_string())];

    for (i, word) in words.iter().enumerate() {
        conditions.push(format!(
            "(CASE WHEN LOWER(summary) LIKE '%' || ?{} || '%' THEN 1 ELSE 0 END)",
            i + 2
        ));
        all_params.push(Box::new(word.clone()));
    }

    let score_expr = conditions.join(" + ");
    let sql = format!(
        "SELECT id, agent_id, created_at, event_type, summary, raw_data, task_id, outcome, tokens_used, cost_cents,
                ({}) as relevance_score
         FROM episodes
         WHERE agent_id = ?1 AND ({}) > 0
         ORDER BY relevance_score DESC, created_at DESC
         LIMIT {}",
        score_expr, score_expr, limit
    );

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_refs.as_slice(), map_episode)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "had",
    "her", "was", "one", "our", "out", "has", "have", "that", "this", "with",
    "they", "been", "from", "will", "what", "when", "make", "like", "just",
    "over", "such", "take", "than", "them", "very", "some", "could", "into",
    "other", "then", "these", "would",
];
```

### Memory Injection into System Prompt

```rust
// In src-tauri/src/api/completions.rs

fn inject_memories(messages: &mut Vec<serde_json::Value>, memories: &[Episode]) {
    if memories.is_empty() {
        return;
    }

    let memory_text = memories.iter()
        .map(|ep| format!("[Memory from {}] {}: {}", ep.created_at, ep.event_type, ep.summary))
        .collect::<Vec<_>>()
        .join("\n");

    let injection = format!(
        "\n\n--- Relevant memories from past tasks ---\n{}\n--- End memories ---",
        memory_text
    );

    // Find existing system message and append, or create one
    if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
        if let Some(content) = system_msg["content"].as_str() {
            system_msg["content"] = serde_json::Value::String(format!("{}{}", content, injection));
        }
    } else {
        messages.insert(0, serde_json::json!({
            "role": "system",
            "content": injection
        }));
    }
}
```

### Memory Isolation

Agents can ONLY access their own memories. Every query includes `WHERE agent_id = ?`. There is no cross-agent memory access in v0.1.

### CONTESTABLE — No Semantic Memory in v0.1

The roadmap describes a full semantic memory system with embeddings, vector search, confidence scores, and decay. I'm deferring ALL of this to v0.2. Here's why:

1. **Embeddings require either an API call (latency + cost) or a local model (~500MB binary size increase)**
2. **sqlite-vss or LanceDB adds significant dependency complexity**
3. **Episodic memory with keyword search is sufficient to prove the concept**
4. **The memory module is structured so semantic search can be added without changing the API**

For v0.2: add `fastembed` for local embeddings, store vectors in a `knowledge` table, and implement proper cosine similarity retrieval with the 0.85 threshold.

### TESTS

```rust
#[cfg(test)]
mod tests {
    // test_insert_episode: insert and retrieve an episode
    // test_get_episodes_limit: insert 10, get 5, verify only 5 returned
    // test_get_episodes_by_task: filter by task_id
    // test_recall_relevant: insert episodes with known summaries, query with matching keyword, verify correct ones returned
    // test_recall_no_match: query with unrelated keyword, verify empty result
    // test_memory_isolation: insert episodes for agent A and B, recall for A returns only A's
    // test_stop_words_filtered: query with only stop words returns empty
}
```

---

## 11. IDENTITY

### WHAT

Each agent has a unique identity: UUID, name, Ed25519 key pair, and reputation score. The identity persists across sessions and will be used for cross-agent trust in future versions.

### WHY

Identity is the foundation for trust, accountability, and future multiplayer features. Even in v0.1, it establishes the pattern that agents are persistent beings, not disposable scripts.

### Agent Structure

```rust
// src-tauri/src/identity/mod.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub system_prompt: String,
    #[serde(skip_serializing)]
    pub public_key: Vec<u8>,
    #[serde(skip)]
    pub private_key: Vec<u8>,
    pub tools_allowed: Vec<String>,
    pub max_spend_cents: i64,
    pub total_tasks: i64,
    pub successful_tasks: i64,
    pub total_spend_cents: i64,
}

/// Serializable agent for API responses (no private key, includes computed fields)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub system_prompt: String,
    pub tools_allowed: Vec<String>,
    pub max_spend_cents: i64,
    pub total_tasks: i64,
    pub successful_tasks: i64,
    pub total_spend_cents: i64,
    pub reputation: f64,
    pub public_key: String,  // base64-encoded
}

impl Agent {
    pub fn reputation(&self) -> f64 {
        if self.total_tasks == 0 {
            0.5 // Starting reputation
        } else {
            (self.successful_tasks as f64 / self.total_tasks as f64) * 0.8 + 0.5 * 0.2
        }
    }

    pub fn to_response(&self) -> AgentResponse {
        use base64::Engine;
        AgentResponse {
            id: self.id.clone(),
            name: self.name.clone(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            status: self.status.clone(),
            system_prompt: self.system_prompt.clone(),
            tools_allowed: self.tools_allowed.clone(),
            max_spend_cents: self.max_spend_cents,
            total_tasks: self.total_tasks,
            successful_tasks: self.successful_tasks,
            total_spend_cents: self.total_spend_cents,
            reputation: self.reputation(),
            public_key: base64::engine::general_purpose::STANDARD.encode(&self.public_key),
        }
    }
}
```

**Note:** `base64` crate is already listed in Cargo.toml (Section 4).

### Key Generation

**File: `src-tauri/src/identity/registry.rs`**

```rust
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use rusqlite::{Connection, params};
use crate::identity::Agent;

pub fn create_agent(
    conn: &Connection,
    name: &str,
    system_prompt: &str,
    tools_allowed: &[String],
) -> anyhow::Result<Agent> {
    // Validate name
    if name.trim().is_empty() {
        anyhow::bail!("name is required");
    }

    // Check for duplicate name
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM agents WHERE name = ?1",
        params![name],
        |row| row.get(0),
    )?;
    if exists {
        anyhow::bail!("agent with name '{}' already exists", name);
    }

    // Validate tools
    let valid_tools = ["shell", "read_file", "write_file", "http_get"];
    for tool in tools_allowed {
        if !valid_tools.contains(&tool.as_str()) {
            anyhow::bail!("invalid tool: {}. Valid tools: {}", tool, valid_tools.join(", "));
        }
    }

    // Generate Ed25519 key pair
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key: VerifyingKey = (&signing_key).into();

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let tools_json = serde_json::to_string(tools_allowed)?;

    conn.execute(
        "INSERT INTO agents (id, name, created_at, updated_at, status, system_prompt, public_key, private_key, tools_allowed, max_spend_cents)
         VALUES (?1, ?2, ?3, ?4, 'idle', ?5, ?6, ?7, ?8, 0)",
        params![
            id,
            name,
            now,
            now,
            system_prompt,
            verifying_key.as_bytes().to_vec(),
            signing_key.to_bytes().to_vec(),
            tools_json,
        ],
    )?;

    Ok(Agent {
        id,
        name: name.to_string(),
        created_at: now.clone(),
        updated_at: now,
        status: "idle".to_string(),
        system_prompt: system_prompt.to_string(),
        public_key: verifying_key.as_bytes().to_vec(),
        private_key: signing_key.to_bytes().to_vec(),
        tools_allowed: tools_allowed.to_vec(),
        max_spend_cents: 0,
        total_tasks: 0,
        successful_tasks: 0,
        total_spend_cents: 0,
    })
}

pub fn list_agents(conn: &Connection) -> anyhow::Result<Vec<Agent>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at, updated_at, status, system_prompt, public_key, private_key, tools_allowed, max_spend_cents, total_tasks, successful_tasks, total_spend_cents
         FROM agents ORDER BY created_at DESC"
    )?;

    let agents = stmt.query_map([], |row| {
        let tools_json: String = row.get(8)?;
        Ok(Agent {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            status: row.get(4)?,
            system_prompt: row.get(5)?,
            public_key: row.get(6)?,
            private_key: row.get(7)?,
            tools_allowed: serde_json::from_str(&tools_json).unwrap_or_default(),
            max_spend_cents: row.get(9)?,
            total_tasks: row.get(10)?,
            successful_tasks: row.get(11)?,
            total_spend_cents: row.get(12)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;

    Ok(agents)
}

pub fn get_agent(conn: &Connection, id: &str) -> anyhow::Result<Option<Agent>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at, updated_at, status, system_prompt, public_key, private_key, tools_allowed, max_spend_cents, total_tasks, successful_tasks, total_spend_cents
         FROM agents WHERE id = ?1"
    )?;

    let mut agents = stmt.query_map(params![id], |row| {
        let tools_json: String = row.get(8)?;
        Ok(Agent {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            status: row.get(4)?,
            system_prompt: row.get(5)?,
            public_key: row.get(6)?,
            private_key: row.get(7)?,
            tools_allowed: serde_json::from_str(&tools_json).unwrap_or_default(),
            max_spend_cents: row.get(9)?,
            total_tasks: row.get(10)?,
            successful_tasks: row.get(11)?,
            total_spend_cents: row.get(12)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;

    Ok(agents.into_iter().next())
}

pub fn update_agent_status(conn: &Connection, id: &str, status: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE agents SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status, now, id],
    )?;
    Ok(())
}

pub fn increment_task_counts(
    conn: &Connection,
    id: &str,
    success: bool,
    cost_cents: i64,
) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    if success {
        conn.execute(
            "UPDATE agents SET total_tasks = total_tasks + 1, successful_tasks = successful_tasks + 1, total_spend_cents = total_spend_cents + ?1, updated_at = ?2 WHERE id = ?3",
            params![cost_cents, now, id],
        )?;
    } else {
        conn.execute(
            "UPDATE agents SET total_tasks = total_tasks + 1, total_spend_cents = total_spend_cents + ?1, updated_at = ?2 WHERE id = ?3",
            params![cost_cents, now, id],
        )?;
    }
    Ok(())
}
```

### Reputation Calculation

```
reputation = (successful_tasks / total_tasks) * 0.8 + 0.5 * 0.2
```

- New agent (0 tasks): 0.5 (neutral)
- Perfect track record: 0.9
- All failures: 0.1
- The 0.2 weight on 0.5 is a "regression to mean" that prevents a single failure from tanking reputation

### TESTS

```rust
#[cfg(test)]
mod tests {
    // test_create_agent: creates agent, verify all fields set
    // test_create_agent_generates_keys: verify public_key and private_key are 32 bytes each
    // test_create_agent_duplicate_name: second creation with same name fails
    // test_create_agent_invalid_tool: unknown tool name fails
    // test_create_agent_empty_name: empty name fails
    // test_list_agents: create 3, list returns 3
    // test_get_agent_found: create and retrieve by id
    // test_get_agent_not_found: returns None for unknown id
    // test_reputation_new_agent: 0 tasks → 0.5
    // test_reputation_all_success: 10/10 → 0.9
    // test_reputation_half_success: 5/10 → 0.5
    // test_update_status: change to active, verify
    // test_increment_task_counts: increment and verify totals
}
```

---

## 12. PERMISSIONS AND AUDIT

### WHAT

Permission system controls which tools each agent can use. Audit log records every action for accountability and debugging.

### WHY

Safety. Users must be able to restrict what agents do. The audit log provides full transparency and enables replay/debugging.

### Permission Checking

**File: `src-tauri/src/permissions/mod.rs`**

```rust
use crate::identity::Agent;

/// Available tools in v0.1
pub const AVAILABLE_TOOLS: &[&str] = &["shell", "read_file", "write_file", "http_get"];

pub fn check_tool_permission(agent: &Agent, tool_name: &str) -> bool {
    agent.tools_allowed.iter().any(|t| t == tool_name)
}

pub fn check_spending_cap(agent: &Agent, additional_cents: i64) -> bool {
    if agent.max_spend_cents == 0 {
        return true; // 0 = unlimited
    }
    agent.total_spend_cents + additional_cents <= agent.max_spend_cents
}
```

### Audit Log Operations

**File: `src-tauri/src/permissions/audit.rs`**

```rust
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub agent_id: String,
    pub created_at: String,
    pub action_type: String,
    pub action_detail: String,
    pub permission_result: String,
    pub result: Option<String>,
    pub duration_ms: Option<i64>,
    pub cost_cents: i64,
    pub error: Option<String>,
}

pub fn log_action(conn: &Connection, entry: &AuditEntry) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO audit_log (id, agent_id, created_at, action_type, action_detail, permission_result, result, duration_ms, cost_cents, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            entry.id,
            entry.agent_id,
            entry.created_at,
            entry.action_type,
            entry.action_detail,
            entry.permission_result,
            entry.result,
            entry.duration_ms,
            entry.cost_cents,
            entry.error,
        ],
    )?;
    Ok(())
}

pub fn get_audit_log(
    conn: &Connection,
    agent_id: &str,
    limit: i64,
) -> anyhow::Result<Vec<AuditEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, created_at, action_type, action_detail, permission_result, result, duration_ms, cost_cents, error
         FROM audit_log WHERE agent_id = ?1 ORDER BY created_at DESC LIMIT ?2"
    )?;

    let entries = stmt.query_map(params![agent_id, limit], |row| {
        Ok(AuditEntry {
            id: row.get(0)?,
            agent_id: row.get(1)?,
            created_at: row.get(2)?,
            action_type: row.get(3)?,
            action_detail: row.get(4)?,
            permission_result: row.get(5)?,
            result: row.get(6)?,
            duration_ms: row.get(7)?,
            cost_cents: row.get(8)?,
            error: row.get(9)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;

    Ok(entries)
}

/// Get recent activity across ALL agents (for dashboard feed)
pub fn get_recent_activity(conn: &Connection, limit: i64) -> anyhow::Result<Vec<AuditEntry>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.agent_id, a.created_at, a.action_type, a.action_detail, a.permission_result, a.result, a.duration_ms, a.cost_cents, a.error
         FROM audit_log a ORDER BY a.created_at DESC LIMIT ?1"
    )?;

    let entries = stmt.query_map(params![limit], |row| {
        Ok(AuditEntry {
            id: row.get(0)?,
            agent_id: row.get(1)?,
            created_at: row.get(2)?,
            action_type: row.get(3)?,
            action_detail: row.get(4)?,
            permission_result: row.get(5)?,
            result: row.get(6)?,
            duration_ms: row.get(7)?,
            cost_cents: row.get(8)?,
            error: row.get(9)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;

    Ok(entries)
}
```

### Human-in-the-Loop (v0.1 Scope)

**Not implemented in v0.1.** The `requires_approval` field from the roadmap is deferred. In v0.1:
- If a tool is in `tools_allowed`, it executes
- If a tool is NOT in `tools_allowed`, it's denied
- No pause-and-ask flow

**CONTESTABLE:** Human-in-the-loop adds significant complexity (async approval flow, UI notifications, timeout handling). For v0.1, the binary allowed/denied model is sufficient. The audit log captures everything, so the user can review after the fact. Add approval flow in v0.2 when the UI patterns are established.

### TESTS

```rust
#[cfg(test)]
mod tests {
    // test_check_tool_allowed: agent with ["shell"] allows "shell"
    // test_check_tool_denied: agent with ["shell"] denies "http_get"
    // test_spending_cap_unlimited: max_spend=0 always allows
    // test_spending_cap_within: total 50, cap 100, additional 30 → allowed
    // test_spending_cap_exceeded: total 80, cap 100, additional 30 → denied
    // test_log_and_retrieve_audit: insert entry, retrieve it
    // test_audit_ordering: entries returned newest-first
    // test_recent_activity: entries from multiple agents returned together
}
```

---

## 13. AGENT-TO-AGENT LOCAL COMMUNICATION

### WHAT

Not implemented in v0.1. This section documents the hooks for future implementation.

### WHY (Deferral)

Agent-to-agent communication requires the message protocol, task delegation flow, and budget transfer system. This is Phase 5 in the roadmap (weeks 14-17). For v0.1, agents are independent — they don't know about each other.

### Future Hooks

The `Agent` struct already has fields that support future communication:
- `tools_allowed` can include a `"communicate"` tool
- The audit log already captures `"communication"` as an `action_type`
- The agent registry (list/get) provides discovery

**Implementation notes for v0.2+:**
- Add a `messages` table to SQLite
- Use `tokio::sync::broadcast` channel for local message bus
- Each agent gets a `tokio::sync::mpsc` channel for direct messages
- Message types as defined in the roadmap: `Announce`, `Discover`, `TaskRequest`, etc.
- The axum API gets new endpoints: `POST /v1/agents/{id}/send`, `GET /v1/agents/{id}/messages`

### CONTESTABLE

This is the right call. Shipping communication in v0.1 would double the implementation scope for a feature that requires multiple agents to be useful — and v0.1 users will likely only create 1-2 agents. Build the foundation, ship it, add communication when users are ready.

---

## 14. FRONTEND

### WHAT

React + TypeScript + Tailwind UI with three pages: Dashboard, Agent Detail, and Settings. Dark theme. Clean and minimal.

### Design System

#### Colors (CSS Custom Properties in globals.css)

```css
:root {
  /* Background layers */
  --bg-primary: #0a0a0b;       /* Main background - near black */
  --bg-secondary: #111113;     /* Card/panel background */
  --bg-tertiary: #1a1a1e;      /* Elevated elements, hover states */
  --bg-hover: #222228;         /* Hover on interactive elements */

  /* Text */
  --text-primary: #e4e4e7;     /* Primary text - zinc-200 */
  --text-secondary: #a1a1aa;   /* Secondary text - zinc-400 */
  --text-muted: #71717a;       /* Muted text - zinc-500 */

  /* Accent */
  --accent: #22c55e;           /* Green - the "GreenCube" brand color */
  --accent-hover: #16a34a;     /* Green darker */
  --accent-subtle: #22c55e1a;  /* Green at 10% opacity */

  /* Status */
  --status-active: #22c55e;    /* Green */
  --status-idle: #a1a1aa;      /* Gray */
  --status-error: #ef4444;     /* Red */

  /* Borders */
  --border: #27272a;           /* zinc-800 */
  --border-hover: #3f3f46;     /* zinc-700 */

  /* Misc */
  --shadow: 0 1px 3px rgba(0,0,0,0.5);
  --radius: 8px;
  --radius-lg: 12px;
}
```

#### Typography

```css
body {
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  font-size: 14px;
  line-height: 1.5;
  color: var(--text-primary);
  background: var(--bg-primary);
}

/* Font sizes */
/* .text-xs: 12px — timestamps, badges */
/* .text-sm: 14px — body text, table content */
/* .text-base: 16px — section headings */
/* .text-lg: 18px — page titles */
/* .text-xl: 20px — main headings */
```

#### Spacing

Use Tailwind's default scale. Consistent patterns:
- Page padding: `p-6`
- Card padding: `p-4`
- Gap between cards: `gap-4`
- Section margins: `mb-6`

### globals.css

**File: `src/styles/globals.css`**

```css
@tailwind base;
@tailwind components;
@tailwind utilities;

:root {
  --bg-primary: #0a0a0b;
  --bg-secondary: #111113;
  --bg-tertiary: #1a1a1e;
  --bg-hover: #222228;
  --text-primary: #e4e4e7;
  --text-secondary: #a1a1aa;
  --text-muted: #71717a;
  --accent: #22c55e;
  --accent-hover: #16a34a;
  --accent-subtle: rgba(34, 197, 94, 0.1);
  --status-active: #22c55e;
  --status-idle: #a1a1aa;
  --status-error: #ef4444;
  --border: #27272a;
  --border-hover: #3f3f46;
  --radius: 8px;
  --radius-lg: 12px;
}

body {
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  background: var(--bg-primary);
  color: var(--text-primary);
  margin: 0;
  padding: 0;
  min-height: 100vh;
}

/* Scrollbar styling */
::-webkit-scrollbar {
  width: 6px;
}
::-webkit-scrollbar-track {
  background: var(--bg-primary);
}
::-webkit-scrollbar-thumb {
  background: var(--border-hover);
  border-radius: 3px;
}
```

### TypeScript Types

**File: `src/lib/types.ts`**

```typescript
export interface Agent {
  id: string;
  name: string;
  created_at: string;
  updated_at: string;
  status: 'idle' | 'active' | 'error';
  system_prompt: string;
  tools_allowed: string[];
  max_spend_cents: number;
  total_tasks: number;
  successful_tasks: number;
  total_spend_cents: number;
  reputation: number;
  public_key: string;
}

export interface Episode {
  id: string;
  agent_id: string;
  created_at: string;
  event_type: string;
  summary: string;
  raw_data?: string;
  task_id?: string;
  outcome?: string;
  tokens_used: number;
  cost_cents: number;
}

export interface AuditEntry {
  id: string;
  agent_id: string;
  created_at: string;
  action_type: string;
  action_detail: string;
  permission_result: string;
  result?: string;
  duration_ms?: number;
  cost_cents: number;
  error?: string;
}

export interface AppConfig {
  llm: {
    api_base_url: string;
    api_key: string;
    default_model: string;
  };
  server: {
    host: string;
    port: number;
  };
  sandbox: {
    image: string;
    cpu_limit_cores: number;
    memory_limit_mb: number;
    timeout_seconds: number;
    network_enabled: boolean;
  };
  ui: {
    onboarding_complete: boolean;
  };
}

export interface DockerStatus {
  available: boolean;
}
```

### Tauri Invoke Wrappers

**File: `src/lib/invoke.ts`**

```typescript
import { invoke } from '@tauri-apps/api/core';
import type { Agent, Episode, AuditEntry, AppConfig, DockerStatus } from './types';

export async function getAgents(): Promise<Agent[]> {
  return invoke<Agent[]>('get_agents');
}

export async function getAgent(id: string): Promise<Agent> {
  return invoke<Agent>('get_agent', { id });
}

export async function createAgent(name: string, systemPrompt: string, toolsAllowed: string[]): Promise<Agent> {
  return invoke<Agent>('create_agent', { name, systemPrompt, toolsAllowed });
}

export async function getEpisodes(agentId: string, limit: number = 50): Promise<Episode[]> {
  return invoke<Episode[]>('get_episodes', { agentId, limit });
}

export async function getAuditLog(agentId: string, limit: number = 50): Promise<AuditEntry[]> {
  return invoke<AuditEntry[]>('get_audit_log', { agentId, limit });
}

export async function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>('get_config');
}

export async function saveConfig(config: AppConfig): Promise<void> {
  return invoke<void>('save_config', { config });
}

export async function getDockerStatus(): Promise<DockerStatus> {
  return invoke<DockerStatus>('get_docker_status');
}

export async function getActivityFeed(limit: number = 50): Promise<AuditEntry[]> {
  return invoke<AuditEntry[]>('get_activity_feed', { limit });
}
```

### Event Listeners

**File: `src/lib/events.ts`**

```typescript
import { listen } from '@tauri-apps/api/event';
import type { AuditEntry } from './types';

export function onActivityUpdate(callback: (entry: AuditEntry) => void) {
  return listen<AuditEntry>('activity-update', (event) => {
    callback(event.payload);
  });
}

export function onAgentStatusChange(callback: (data: { id: string; status: string }) => void) {
  return listen<{ id: string; status: string }>('agent-status-change', (event) => {
    callback(event.payload);
  });
}
```

### App Context

**File: `src/context/AppContext.tsx`**

```typescript
import React, { createContext, useContext, useReducer, useEffect } from 'react';
import type { Agent, AppConfig } from '../lib/types';
import { getAgents, getConfig } from '../lib/invoke';

interface AppState {
  agents: Agent[];
  config: AppConfig | null;
  dockerAvailable: boolean;
  loading: boolean;
}

type Action =
  | { type: 'SET_AGENTS'; agents: Agent[] }
  | { type: 'SET_CONFIG'; config: AppConfig }
  | { type: 'SET_DOCKER'; available: boolean }
  | { type: 'SET_LOADING'; loading: boolean }
  | { type: 'ADD_AGENT'; agent: Agent }
  | { type: 'UPDATE_AGENT_STATUS'; id: string; status: string };

const initialState: AppState = {
  agents: [],
  config: null,
  dockerAvailable: false,
  loading: true,
};

function reducer(state: AppState, action: Action): AppState {
  switch (action.type) {
    case 'SET_AGENTS':
      return { ...state, agents: action.agents };
    case 'SET_CONFIG':
      return { ...state, config: action.config };
    case 'SET_DOCKER':
      return { ...state, dockerAvailable: action.available };
    case 'SET_LOADING':
      return { ...state, loading: action.loading };
    case 'ADD_AGENT':
      return { ...state, agents: [action.agent, ...state.agents] };
    case 'UPDATE_AGENT_STATUS':
      return {
        ...state,
        agents: state.agents.map(a =>
          a.id === action.id ? { ...a, status: action.status as Agent['status'] } : a
        ),
      };
    default:
      return state;
  }
}

const AppContext = createContext<{
  state: AppState;
  dispatch: React.Dispatch<Action>;
}>({ state: initialState, dispatch: () => {} });

export function AppProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(reducer, initialState);

  useEffect(() => {
    async function init() {
      try {
        const [agents, config] = await Promise.all([getAgents(), getConfig()]);
        dispatch({ type: 'SET_AGENTS', agents });
        dispatch({ type: 'SET_CONFIG', config });
      } catch (err) {
        console.error('Failed to initialize:', err);
      } finally {
        dispatch({ type: 'SET_LOADING', loading: false });
      }
    }
    init();
  }, []);

  return (
    <AppContext.Provider value={{ state, dispatch }}>
      {children}
    </AppContext.Provider>
  );
}

export function useApp() {
  return useContext(AppContext);
}
```

### Components

#### Layout.tsx

```typescript
// src/components/Layout.tsx
// Shell with sidebar navigation and main content area
// Sidebar: GreenCube logo, nav links (Dashboard, Settings), docker status indicator
// Content: children rendered in main area with p-6 padding
// Props: { children: React.ReactNode }
// Sidebar width: w-56 (224px), bg-secondary, border-r border
// Active nav item: bg-accent-subtle, text-accent
// Logo: "GreenCube" text in text-lg font-bold, with a small green square icon (CSS-only, 12x12px div with bg-accent rounded-sm)
```

#### AgentCard.tsx

```typescript
// src/components/AgentCard.tsx
// Displays agent summary in dashboard grid
// Props: { agent: Agent, onClick: () => void }
// Layout: bg-secondary rounded-lg p-4 border border-[var(--border)] hover:border-[var(--border-hover)] cursor-pointer transition
// Shows: name (text-base font-medium), status badge, tools count, tasks count, reputation bar
// Status badge: colored dot (8px circle) + text, using status colors
// Reputation: thin progress bar (h-1) with accent color, width = reputation * 100%
// Click navigates to AgentDetail page
```

#### StatusBadge.tsx

```typescript
// src/components/StatusBadge.tsx
// Small status indicator
// Props: { status: 'idle' | 'active' | 'error' }
// Renders: 8px circle + capitalized status text
// Colors: active=green, idle=gray, error=red
// Flex row, items-center, gap-2, text-xs
```

#### ActivityFeed.tsx

```typescript
// src/components/ActivityFeed.tsx
// Scrolling list of recent audit entries
// Props: { entries: AuditEntry[], agentNames?: Record<string, string> }
// Each entry: one row showing timestamp (text-muted, text-xs), agent name (if provided), action_type badge, summary from action_detail
// Action type badges: tool_call=blue, llm_request=purple, permission_check=yellow, error=red
// Max height with overflow-y-auto, scrollbar styled
// If entries empty, show EmptyState with "No activity yet"
```

#### MemoryViewer.tsx

```typescript
// src/components/MemoryViewer.tsx
// Shows agent's episodic memory timeline
// Props: { episodes: Episode[] }
// Vertical timeline layout: left border line (2px accent), entries as cards
// Each entry: created_at timestamp, event_type badge, summary text, outcome badge if present
// Grouped by task_id when available (collapsible groups)
// If empty, show EmptyState with "No memories yet. This agent hasn't run any tasks."
```

#### AuditLog.tsx

```typescript
// src/components/AuditLog.tsx
// Table view of audit entries for a specific agent
// Props: { entries: AuditEntry[] }
// Columns: Time, Action, Permission, Duration, Cost, Error
// Sortable by clicking column headers (local state sorting)
// Permission column: green "allowed" or red "denied" badge
// Error column: red text if present, "—" if null
// Zebra striping: even rows bg-secondary, odd rows bg-primary
```

#### OnboardingModal.tsx

```typescript
// src/components/OnboardingModal.tsx
// Full-screen overlay shown on first launch
// Step 1: Welcome message + API key input
//   - "Welcome to GreenCube" heading
//   - "Enter your OpenAI API key to get started" subtext
//   - Text input for API key (password type, monospace font)
//   - "API Base URL" text input (prefilled with https://api.openai.com/v1)
//   - "Continue" button (accent color, disabled until key entered)
// Step 2: Create first agent
//   - "Create your first agent" heading
//   - Name input
//   - System prompt textarea
//   - Tool checkboxes (shell, read_file, write_file, http_get)
//   - "Create Agent" button
// Step 3: Done
//   - "You're all set!" with green checkmark
//   - Shows agent card preview
//   - "Go to Dashboard" button
// State: currentStep (1|2|3), formData
// On complete: calls saveConfig (with API key + onboarding_complete=true) and createAgent
```

#### CreateAgentModal.tsx

```typescript
// src/components/CreateAgentModal.tsx
// Modal dialog for creating new agent (used after onboarding)
// Props: { isOpen: boolean, onClose: () => void, onCreated: (agent: Agent) => void }
// Fields: name (required), system_prompt (optional textarea), tools_allowed (checkboxes)
// Validation: name must be non-empty, at least one tool selected
// On submit: calls createAgent invoke, then onCreated callback
// Overlay: fixed inset-0 bg-black/50, centered bg-secondary card with max-w-md rounded-lg border border-[var(--border)]
```

#### EmptyState.tsx

```typescript
// src/components/EmptyState.tsx
// Placeholder for empty lists/views
// Props: { message: string, icon?: 'agents' | 'memory' | 'activity' }
// Centered text with muted color, subtle icon above
// Used in ActivityFeed, MemoryViewer, and agent lists when empty
```

### Pages

#### Dashboard.tsx

```typescript
// src/pages/Dashboard.tsx
// Main page — grid of agent cards + activity feed
// Layout: two columns on desktop (lg:grid-cols-3), full width on mobile
//   Left 2 cols: Agent grid (grid of AgentCards, 1-2 cols depending on count)
//   Right 1 col: Activity feed (sticky, scrollable)
// Top bar: "Dashboard" title + "New Agent" button (opens CreateAgentModal)
// If no agents: full-width EmptyState with "Create your first agent to get started" + create button
// Docker banner: if !dockerAvailable, yellow banner at top: "Docker not detected. Tool execution is disabled."
// Data: fetches agents from context, activity from getActivityFeed on mount
// Real-time: listens to 'activity-update' and 'agent-status-change' events to update
// Agent card click: navigate to /agent/{id} via react-router
```

#### AgentDetail.tsx

```typescript
// src/pages/AgentDetail.tsx
// Deep view of a single agent
// URL: /agent/:id (react-router param)
// Layout: tabs — Overview | Memory | Audit Log
//   Overview tab:
//     - Agent name, status badge, created date
//     - Stats row: total tasks, success rate, total spend, reputation
//     - Tools allowed (listed as badges)
//     - Public key (truncated, copy button)
//     - Recent activity (last 10 audit entries)
//   Memory tab:
//     - MemoryViewer component with episodes fetched from getEpisodes
//   Audit tab:
//     - AuditLog component with entries fetched from getAuditLog
// Back button: ← Dashboard (navigates to /)
// Data: fetches agent, episodes, audit on mount. Refreshes on 'activity-update' event matching this agent.
```

#### Settings.tsx

```typescript
// src/pages/Settings.tsx
// Configuration page
// Sections:
//   LLM Configuration:
//     - API Base URL (text input)
//     - API Key (password input with show/hide toggle)
//     - Default Model (text input)
//   Server:
//     - Port (number input, readonly in v0.1 — changing requires restart)
//   Sandbox Defaults:
//     - Docker Image (text input)
//     - CPU Limit (number input)
//     - Memory Limit MB (number input)
//     - Timeout Seconds (number input)
//     - Network Enabled (toggle)
//   About:
//     - Version: 0.1.0
//     - Docker Status: Available/Not Available
//     - Data directory: ~/.greencube/
// Save button at bottom: calls saveConfig
// All inputs use controlled state, initialized from getConfig on mount
```

### App.tsx

**File: `src/App.tsx`**

```typescript
// CRITICAL: Use HashRouter, NOT BrowserRouter. Tauri serves from a custom protocol
// (tauri://localhost). BrowserRouter uses History API which requires a real server.
// Refreshing on /agent/123 would break. HashRouter uses /#/agent/123 which always works.
import { HashRouter, Routes, Route } from 'react-router-dom';
import { AppProvider } from './context/AppContext';
import { Layout } from './components/Layout';
import { Dashboard } from './pages/Dashboard';
import { AgentDetail } from './pages/AgentDetail';
import { Settings } from './pages/Settings';
import { OnboardingModal } from './components/OnboardingModal';
import { useApp } from './context/AppContext';

// Error boundary to prevent white-screen crashes
class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { hasError: boolean; error: string }
> {
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { hasError: false, error: '' };
  }
  static getDerivedStateFromError(error: Error) {
    return { hasError: true, error: error.message };
  }
  render() {
    if (this.state.hasError) {
      return (
        <div className="flex flex-col items-center justify-center min-h-screen gap-4">
          <div className="text-[var(--status-error)] text-lg">Something went wrong</div>
          <div className="text-[var(--text-muted)] text-sm">{this.state.error}</div>
          <button
            onClick={() => window.location.reload()}
            className="px-4 py-2 bg-[var(--accent)] text-black rounded-lg"
          >
            Reload
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

function AppContent() {
  const { state } = useApp();

  if (state.loading) {
    return (
      <div className="flex items-center justify-center min-h-screen">
        <div className="text-[var(--text-muted)]">Loading...</div>
      </div>
    );
  }

  if (state.config && !state.config.ui.onboarding_complete) {
    return <OnboardingModal />;
  }

  return (
    <HashRouter>
      <Layout>
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/agent/:id" element={<AgentDetail />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </Layout>
    </HashRouter>
  );
}

export default function App() {
  return (
    <ErrorBoundary>
      <AppProvider>
        <AppContent />
      </AppProvider>
    </ErrorBoundary>
  );
}
```

### main.tsx

**File: `src/main.tsx`**

```typescript
import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './styles/globals.css';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
```

### vite.config.ts

**File: `vite.config.ts`**

```typescript
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// https://tauri.app/start/frontend/vite/
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  // Vite options tailored for Tauri development
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
});
```

### Real-Time Update Strategy

The frontend uses Tauri events (not polling) for real-time updates:

1. Backend emits `activity-update` event with audit entry payload whenever an action occurs
2. Backend emits `agent-status-change` when agent status changes
3. Dashboard listens for both events and updates its local state
4. AgentDetail listens for events matching its agent ID

Event setup in components:
```typescript
useEffect(() => {
  const unlisten = onActivityUpdate((entry) => {
    setEntries(prev => [entry, ...prev].slice(0, 100));
  });
  return () => { unlisten.then(fn => fn()); };
}, []);
```

### TESTS

Frontend testing is minimal for v0.1 (the value is in backend tests). But include:
- TypeScript compilation check (`tsc --noEmit`)
- Verify all imports resolve
- Basic render test for App component (if a testing framework is set up)

---

## 15. ERROR HANDLING

### WHAT

Unified error types in Rust, consistent error responses from API, and user-friendly error messages in the UI.

### Error Type Hierarchy

**File: `src-tauri/src/errors.rs`**

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GreenCubeError {
    // Database errors
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    // Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    // Agent errors
    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Agent error: {0}")]
    AgentError(String),

    #[error("Agent with name '{0}' already exists")]
    DuplicateAgent(String),

    // LLM proxy errors
    #[error("LLM API error: {0}")]
    LlmError(String),

    #[error("LLM API unreachable: {0}")]
    LlmUnreachable(String),

    // Sandbox errors
    #[error("Docker not available")]
    DockerNotAvailable,

    #[error("Sandbox error: {0}")]
    SandboxError(String),

    #[error("Sandbox timeout after {0} seconds")]
    SandboxTimeout(u64),

    // Permission errors
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Spending cap exceeded")]
    SpendingCapExceeded,

    // General
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

// For Tauri commands — must implement Serialize
impl serde::Serialize for GreenCubeError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

// For axum responses
impl axum::response::IntoResponse for GreenCubeError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;

        let (status, message) = match &self {
            GreenCubeError::AgentNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            GreenCubeError::Validation(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            GreenCubeError::DuplicateAgent(_) => (StatusCode::CONFLICT, self.to_string()),
            GreenCubeError::PermissionDenied(_) => (StatusCode::FORBIDDEN, self.to_string()),
            GreenCubeError::SpendingCapExceeded => (StatusCode::FORBIDDEN, self.to_string()),
            GreenCubeError::DockerNotAvailable => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            GreenCubeError::LlmUnreachable(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            GreenCubeError::LlmError(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = serde_json::json!({ "error": message });
        (status, axum::Json(body)).into_response()
    }
}
```

### Recovery Strategies

| Error | Recovery |
|-------|----------|
| DB locked | Retry once after 100ms, then fail with message |
| DB corrupt | Rename to .corrupt, create fresh, warn user |
| Docker gone mid-task | Kill orphaned containers on next startup, return error to caller |
| LLM rate limited | Return 502 to caller, log episode with event_type='error' |
| Sandbox OOM | Return result with exit_code=137 and explanation |
| Config corrupt | Rename to .bak, create default |
| Port 9000 taken | Panic with clear error: "Port 9000 is already in use. Close the other process or change server.port in config.toml." (v0.2: auto-try 9001-9010) |

### Logging

Using `tracing` crate. Log to file at `~/.greencube/logs/greencube.log`.

```rust
// In main.rs setup
use tracing_subscriber::{fmt, EnvFilter};

let log_dir = config_dir().join("logs");
std::fs::create_dir_all(&log_dir)?;
let log_file = std::fs::File::create(log_dir.join("greencube.log"))?;

tracing_subscriber::fmt()
    .with_writer(log_file)
    .with_env_filter(EnvFilter::new("greencube=info"))
    .init();
```

---

## 16. TESTING STRATEGY

### Unit Tests Per Module

Every module has `#[cfg(test)] mod tests` at the bottom. Tests use in-memory SQLite databases (`Connection::open_in_memory()`).

| Module | Tests | What They Verify |
|--------|-------|-----------------|
| `db.rs` | 4 | Schema creation, idempotency, FK enforcement, WAL mode |
| `config.rs` | 3 | Default serialization, load/save, missing file handling |
| `identity/registry.rs` | 13 | Agent CRUD, key generation, validation, reputation math |
| `memory/episodic.rs` | 7 | Insert, retrieve, filter, keyword recall, isolation |
| `permissions/mod.rs` | 5 | Tool allow/deny, spending cap checks |
| `permissions/audit.rs` | 3 | Log/retrieve audit entries, ordering |
| `sandbox/docker.rs` | 6 | Docker check, echo test, timeout, exit code, network, escaping |
| `api/completions.rs` | 7 | Proxy flow, memory injection, tool filtering, loop limit, errors |
| `errors.rs` | 3 | Error serialization, status code mapping |

**Total: ~51 unit tests**

### Integration Tests

In `tests/` directory at the project root:

```rust
// tests/integration_test.rs

// test_full_agent_lifecycle:
//   1. Create agent via POST /v1/agents
//   2. Verify it appears in GET /v1/agents
//   3. Send chat completion (mocked LLM)
//   4. Verify episode was stored
//   5. Verify audit log entry exists
//   6. Check agent task count incremented

// test_onboarding_flow:
//   1. Start with no config
//   2. Call get_config, verify onboarding_complete = false
//   3. Save config with API key
//   4. Create agent
//   5. Verify onboarding_complete = true
```

### CI Without Docker

Sandbox tests that require Docker are annotated with `#[ignore]`. CI runs:
```bash
cargo test                    # Runs all non-ignored tests
cargo test -- --ignored       # Runs Docker tests (only in environments with Docker)
```

GitHub Actions CI uses `services: docker` for the Docker tests.

### Performance Benchmarks

Not in v0.1. But the architecture supports it:
- SQLite query times (should be <10ms for all queries)
- Sandbox boot time (should be <3s)
- Memory retrieval time (should be <50ms)

---

## 17. BUILD AND RELEASE

### Build Commands

```bash
# Development
npm run tauri dev

# Production build
npm run tauri build
```

### Per Platform

**macOS:**
```bash
# Produces .dmg and .app
npm run tauri build
# Output: src-tauri/target/release/bundle/dmg/GreenCube_0.1.0_aarch64.dmg
```

**Windows:**
```bash
# Produces .msi and .exe
npm run tauri build
# Output: src-tauri/target/release/bundle/msi/GreenCube_0.1.0_x64_en-US.msi
```

**Linux:**
```bash
# Produces .deb and .AppImage
npm run tauri build
# Output: src-tauri/target/release/bundle/deb/greencube_0.1.0_amd64.deb
```

### GitHub Actions CI

**File: `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - name: Install system deps
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
      - name: Install npm deps
        run: npm install
      - name: Rust tests
        run: cd src-tauri && cargo test
      - name: TypeScript check
        run: npx tsc --noEmit
      - name: Build
        run: npm run tauri build

  test-docker:
    runs-on: ubuntu-latest
    services:
      docker:
        image: docker:dind
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Docker sandbox tests
        run: cd src-tauri && cargo test -- --ignored
```

### Auto-Update

Deferred to v0.2. For v0.1, users download new versions manually from GitHub Releases.

### tauri.conf.json

```json
{
  "productName": "GreenCube",
  "version": "0.1.0",
  "identifier": "com.greencube.app",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "title": "GreenCube",
    "windows": [
      {
        "title": "GreenCube",
        "width": 1200,
        "height": 800,
        "minWidth": 800,
        "minHeight": 600,
        "resizable": true,
        "fullscreen": false
      }
    ],
    "security": {
      "csp": "default-src 'self'; connect-src 'self' http://localhost:* https://*; style-src 'self' 'unsafe-inline'"
    }
  },
  "bundle": {
    "active": true,
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

### Tauri 2.0 Capabilities

**File: `src-tauri/capabilities/default.json`**

```json
{
  "identifier": "default",
  "description": "Default capabilities for GreenCube",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "shell:allow-open"
  ]
}
```

---

## 18. FUTURE-PROOFING

### Sandbox Trait for Swappable Backends

The sandbox module uses a concrete function approach in v0.1, but the code is structured so a `Sandbox` trait can be introduced:

```rust
// Future trait (not implemented in v0.1, but the function signatures match)
#[async_trait]
pub trait Sandbox {
    async fn execute(&self, command: &str, options: &SandboxOptions) -> anyhow::Result<SandboxResult>;
    async fn is_available(&self) -> bool;
}

// DockerSandbox implements Sandbox
// WasmSandbox implements Sandbox (future)
// FirecrackerSandbox implements Sandbox (future)
```

For v0.1, the `execute_in_sandbox` and `check_docker_available` free functions serve the same role. Converting to a trait is a mechanical refactor.

### Identity → Multiplayer Extension Points

- Agent public keys are already stored and exposed via API
- The `AgentResponse` struct includes `public_key` for sharing
- The reputation system is already functional
- Adding mDNS discovery (via `mdns-sd` crate) and cross-habitat messaging can build on the existing `Agent` struct

### Messages → Protocol

- The audit log already captures message-like events
- Adding a `messages` table and Tokio channels is additive
- The agent registry already supports discovery (list agents with capabilities)

### Plugin System Hooks

Not implemented in v0.1, but the architecture supports it:
- Tools are dispatched by name in `execute_tool_call` — adding new tools means adding new match arms
- Future: load tools dynamically from a `~/.greencube/plugins/` directory
- Each plugin is a WASM module or a Docker image with a defined interface

---

## 19. IMPLEMENTATION ORDER

This is the dependency graph. Each step depends only on steps above it.

```
Step 1: Project Scaffold
  ├── Cargo.toml, package.json, vite.config.ts, tsconfig.json, tailwind.config.js
  ├── index.html, src/main.tsx (minimal React)
  ├── src-tauri/src/main.rs (minimal Tauri)
  ├── tauri.conf.json, capabilities/default.json
  ├── build.rs
  └── VERIFY: `npm install && npm run tauri dev` opens a window

Step 2: Core Types + Errors
  ├── src-tauri/src/errors.rs
  ├── src-tauri/src/state.rs
  └── VERIFY: `cargo build` passes

Step 3: Config Module
  ├── src-tauri/src/config.rs
  └── VERIFY: `cargo test` in config module passes

Step 4: Database Module
  ├── src-tauri/src/db.rs
  └── VERIFY: `cargo test` in db module passes

Step 5: Identity + Registry
  ├── src-tauri/src/identity/mod.rs
  ├── src-tauri/src/identity/registry.rs
  └── VERIFY: `cargo test` in identity module passes

Step 6: Memory Module
  ├── src-tauri/src/memory/mod.rs
  ├── src-tauri/src/memory/episodic.rs
  └── VERIFY: `cargo test` in memory module passes

Step 7: Permissions + Audit
  ├── src-tauri/src/permissions/mod.rs
  ├── src-tauri/src/permissions/audit.rs
  └── VERIFY: `cargo test` in permissions module passes

Step 8: Sandbox Module
  ├── src-tauri/src/sandbox/mod.rs
  ├── src-tauri/src/sandbox/docker.rs
  └── VERIFY: `cargo test` (non-Docker tests pass, Docker tests pass if Docker available)

Step 9: Tauri Commands
  ├── src-tauri/src/commands.rs (all invoke handlers)
  └── VERIFY: `cargo build` passes

Step 10: Axum API Server
  ├── src-tauri/src/api/mod.rs
  ├── src-tauri/src/api/health.rs
  ├── src-tauri/src/api/agents.rs
  └── VERIFY: `cargo build` passes

Step 11: OpenAI Proxy
  ├── src-tauri/src/api/completions.rs
  └── VERIFY: `cargo test` in completions module passes

Step 12: Wire main.rs
  ├── Connect all modules in main.rs (state init, DB init, axum spawn, Tauri commands)
  └── VERIFY: `npm run tauri dev` opens app AND localhost:9000/health responds

Step 13: Frontend Types + Invoke Wrappers
  ├── src/lib/types.ts
  ├── src/lib/invoke.ts
  ├── src/lib/events.ts
  └── VERIFY: `npx tsc --noEmit` passes

Step 14: Frontend Context + App Shell
  ├── src/context/AppContext.tsx
  ├── src/App.tsx
  ├── src/components/Layout.tsx
  ├── src/components/EmptyState.tsx
  ├── src/components/StatusBadge.tsx
  ├── src/styles/globals.css
  └── VERIFY: `npm run tauri dev` shows app shell with sidebar

Step 15: Onboarding Flow
  ├── src/components/OnboardingModal.tsx
  └── VERIFY: First launch shows onboarding, entering key + creating agent completes it

Step 16: Dashboard Page
  ├── src/pages/Dashboard.tsx
  ├── src/components/AgentCard.tsx
  ├── src/components/ActivityFeed.tsx
  ├── src/components/CreateAgentModal.tsx
  └── VERIFY: Dashboard shows agents and activity

Step 17: Agent Detail Page
  ├── src/pages/AgentDetail.tsx
  ├── src/components/MemoryViewer.tsx
  ├── src/components/AuditLog.tsx
  └── VERIFY: Clicking agent card navigates to detail view

Step 18: Settings Page
  ├── src/pages/Settings.tsx
  └── VERIFY: Settings save and persist across restarts

Step 19: Real-time Events
  ├── Wire Tauri emit calls in backend (completions handler, sandbox)
  ├── Wire event listeners in frontend components
  └── VERIFY: Activity feed updates when API receives a request

Step 20: Polish + Docs
  ├── CLAUDE.md
  ├── README.md
  ├── .gitignore
  ├── .github/workflows/ci.yml
  └── VERIFY: Full test suite passes, `npm run tauri build` produces installer
```

---

## 20. ONESHOT EXECUTION PLAN

This is the step-by-step checklist for Claude Code to build GreenCube v0.1 in a single run.

### STEP 1: Initialize Project

```
DO NOT use `npm create tauri-app` — the interactive CLI may prompt for choices that
can't be answered non-interactively. Create the scaffold manually from the spec files.

1. Create all directories: src-tauri/src/, src-tauri/capabilities/, src-tauri/icons/,
   src/, src/lib/, src/pages/, src/components/, src/context/, src/styles/
2. Write package.json (Section 4)
3. Write src-tauri/Cargo.toml (Section 4)
4. Write src-tauri/tauri.conf.json (Section 17)
5. Write src-tauri/capabilities/default.json (Section 17)
6. Write src-tauri/build.rs: fn main() { tauri_build::build() }
7. Write vite.config.ts (Section 14)
8. Write tsconfig.json, tsconfig.node.json (standard Vite+React configs)
9. Write tailwind.config.js with content: ["./index.html", "./src/**/*.{ts,tsx}"]
10. Write postcss.config.js with tailwindcss and autoprefixer plugins
11. Write index.html with <div id="root"></div> and script src="/src/main.tsx"
12. Write minimal src/main.tsx and src/App.tsx (just "Hello GreenCube")
13. Write minimal src-tauri/src/main.rs (just Tauri builder with empty setup)
14. Write .gitignore (node_modules, target, dist, .greencube)
15. Run: npm install
16. VERIFY: npm run tauri dev → window opens showing "Hello GreenCube"
```

### STEP 2: Rust Core Modules (no dependencies between them)

Create these files in order:
```
1. src-tauri/src/errors.rs         — Error types (Section 15)
2. src-tauri/src/config.rs         — Config types + load/save (Section 6)
3. src-tauri/src/state.rs          — AppState struct (Section 2)
4. src-tauri/src/db.rs             — Database init + schema (Section 5)
5. VERIFY: cargo build (add mod declarations to main.rs as you go)
```

### STEP 3: Business Logic Modules

```
1. src-tauri/src/identity/mod.rs       — Agent types (Section 11)
2. src-tauri/src/identity/registry.rs  — Agent CRUD (Section 11)
3. src-tauri/src/memory/mod.rs         — Episode types (Section 10)
4. src-tauri/src/memory/episodic.rs    — Episode operations (Section 10)
5. src-tauri/src/permissions/mod.rs    — Permission checks (Section 12)
6. src-tauri/src/permissions/audit.rs  — Audit log operations (Section 12)
7. src-tauri/src/sandbox/mod.rs        — Sandbox types (Section 9)
8. src-tauri/src/sandbox/docker.rs     — Docker implementation (Section 9)
9. VERIFY: cargo test (all unit tests pass)
```

### STEP 4: Tauri Commands

```
1. src-tauri/src/commands.rs — All Tauri invoke handlers:
   - get_agents, get_agent, create_agent
   - get_episodes, get_audit_log, get_activity_feed
   - get_config, save_config
   - get_docker_status
   Each command: extract AppState, lock DB, call business logic, return result
2. VERIFY: cargo build
```

### STEP 5: Axum API Server

```
1. src-tauri/src/api/mod.rs          — Router setup (Section 7)
2. src-tauri/src/api/health.rs       — GET /health
3. src-tauri/src/api/agents.rs       — Agent CRUD endpoints
4. src-tauri/src/api/completions.rs  — OpenAI proxy (Section 8)
5. VERIFY: cargo build
```

### STEP 6: Wire main.rs

```
1. Update src-tauri/src/main.rs:
   - Create ~/.greencube/ directory
   - Load config
   - Initialize database
   - Check Docker availability
   - Create AppState
   - Spawn axum server on Tokio task
   - Register all Tauri commands
   - Run Tauri app
2. VERIFY: npm run tauri dev → window opens, localhost:9000/health responds
```

### STEP 7: Frontend Foundation

```
1. src/styles/globals.css          — Theme CSS (Section 14)
2. src/lib/types.ts                — TypeScript types
3. src/lib/invoke.ts               — Typed invoke wrappers
4. src/lib/events.ts               — Event listeners
5. src/context/AppContext.tsx       — Global state
6. src/components/EmptyState.tsx
7. src/components/StatusBadge.tsx
8. src/components/Layout.tsx
9. src/App.tsx                     — Router setup
10. src/main.tsx                   — Entry point
11. VERIFY: npm run tauri dev → shows layout with sidebar
```

### STEP 8: Onboarding

```
1. src/components/OnboardingModal.tsx
2. VERIFY: Fresh launch (delete ~/.greencube/) → shows onboarding
```

### STEP 9: Dashboard

```
1. src/components/AgentCard.tsx
2. src/components/ActivityFeed.tsx
3. src/components/CreateAgentModal.tsx
4. src/pages/Dashboard.tsx
5. VERIFY: After onboarding, dashboard shows agents and activity
```

### STEP 10: Agent Detail

```
1. src/components/MemoryViewer.tsx
2. src/components/AuditLog.tsx
3. src/pages/AgentDetail.tsx
4. VERIFY: Click agent card → shows detail with tabs
```

### STEP 11: Settings

```
1. src/pages/Settings.tsx
2. VERIFY: Settings load current config, save persists changes
```

### STEP 12: Real-time Events

```
1. Add tauri::Emitter::emit calls in completions handler and sandbox
2. Add event listeners in Dashboard and AgentDetail
3. VERIFY: Send request to localhost:9000/v1/chat/completions → activity feed updates live
```

### STEP 13: Docs + CI

```
1. README.md (project description, install instructions, usage)
2. CLAUDE.md (project rules for Claude Code)
3. .github/workflows/ci.yml (CI pipeline)
4. VERIFY: All tests pass, npm run tauri build succeeds
```

### STEP 14: Final Verification Checklist

```
□ npm run tauri dev → window opens with dark theme
□ First launch shows onboarding wizard
□ Enter API key → saved to config.toml
□ Create agent → appears in dashboard
□ curl localhost:9000/health → returns OK with docker status
□ curl -X POST localhost:9000/v1/agents → creates agent via API
□ curl localhost:9000/v1/agents → lists agents
□ Agent detail page shows memory and audit tabs
□ Settings page saves and persists config
□ Docker banner shows when Docker not available
□ cargo test → all non-Docker tests pass
□ npm run tauri build → produces installer
```

---

## APPENDIX A: COMPLETE main.rs

This is the full entry point that wires everything together:

```rust
// src-tauri/src/main.rs

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod commands;
mod config;
mod db;
mod errors;
mod identity;
mod memory;
mod permissions;
mod sandbox;
mod state;

use config::config_dir;
use state::AppState;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock}; // MUST be tokio::sync, NOT std::sync
use tracing_subscriber::EnvFilter;

fn main() {
    // Initialize tracing — logs to stdout in dev, can be redirected to file later
    let log_dir = config_dir().join("logs");
    let _ = std::fs::create_dir_all(&log_dir);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("greencube=info,tower_http=info"))
        .init();

    tracing::info!("Starting GreenCube v0.1.0");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Ensure data directory exists
            let data_dir = config_dir();
            std::fs::create_dir_all(&data_dir)
                .expect("Failed to create ~/.greencube/");

            // Load config
            let config = config::load_config()
                .expect("Failed to load config");

            // Initialize database
            let db_path = data_dir.join("greencube.db");
            let conn = db::init_database(&db_path)
                .expect("Failed to initialize database");

            // Check Docker
            let docker = tauri::async_runtime::block_on(async {
                match bollard::Docker::connect_with_local_defaults() {
                    Ok(d) => {
                        if d.ping().await.is_ok() {
                            tracing::info!("Docker is available");
                            Some(d)
                        } else {
                            tracing::warn!("Docker is installed but not responding");
                            None
                        }
                    }
                    Err(_) => {
                        tracing::warn!("Docker is not available");
                        None
                    }
                }
            });

            // Create shared state
            // AppHandle is Clone+Send+Sync — no Mutex needed
            let state = Arc::new(AppState {
                db: Mutex::new(conn),
                config: RwLock::new(config.clone()),
                docker: RwLock::new(docker),
                app_handle: handle,
            });

            // Store state in Tauri's managed state
            app.manage(state.clone());

            // Spawn axum server
            let server_state = state.clone();
            let host = config.server.host.clone();
            let port = config.server.port;

            tauri::async_runtime::spawn(async move {
                let router = api::create_router(server_state);
                let addr = format!("{}:{}", host, port);
                let listener = tokio::net::TcpListener::bind(&addr).await
                    .expect(&format!("Failed to bind to {}", addr));
                tracing::info!("API server listening on {}", addr);
                axum::serve(listener, router).await
                    .expect("API server crashed");
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_agents,
            commands::get_agent,
            commands::create_agent,
            commands::get_episodes,
            commands::get_audit_log,
            commands::get_activity_feed,
            commands::get_config,
            commands::save_config,
            commands::get_docker_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

## APPENDIX B: COMPLETE commands.rs

```rust
// src-tauri/src/commands.rs

use crate::config::{self, AppConfig};
use crate::errors::GreenCubeError;
use crate::identity::{self, AgentResponse};
use crate::identity::registry;
use crate::memory::{self, Episode};
use crate::memory::episodic;
use crate::permissions::audit::{self, AuditEntry};
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

type Result<T> = std::result::Result<T, GreenCubeError>;

#[tauri::command]
pub async fn get_agents(state: State<'_, Arc<AppState>>) -> Result<Vec<AgentResponse>> {
    let db = state.db.lock().await;
    let agents = registry::list_agents(&db)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(agents.iter().map(|a| a.to_response()).collect())
}

#[tauri::command]
pub async fn get_agent(id: String, state: State<'_, Arc<AppState>>) -> Result<AgentResponse> {
    let db = state.db.lock().await;
    let agent = registry::get_agent(&db, &id)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?
        .ok_or_else(|| GreenCubeError::AgentNotFound(id))?;
    Ok(agent.to_response())
}

#[tauri::command]
pub async fn create_agent(
    name: String,
    system_prompt: String,
    tools_allowed: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<AgentResponse> {
    let db = state.db.lock().await;
    let agent = registry::create_agent(&db, &name, &system_prompt, &tools_allowed)
        .map_err(|e| GreenCubeError::Validation(e.to_string()))?;
    Ok(agent.to_response())
}

#[tauri::command]
pub async fn get_episodes(
    agent_id: String,
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Episode>> {
    let db = state.db.lock().await;
    let episodes = episodic::get_episodes(&db, &agent_id, limit.unwrap_or(50), None)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(episodes)
}

#[tauri::command]
pub async fn get_audit_log(
    agent_id: String,
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<AuditEntry>> {
    let db = state.db.lock().await;
    let entries = audit::get_audit_log(&db, &agent_id, limit.unwrap_or(50))
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(entries)
}

#[tauri::command]
pub async fn get_activity_feed(
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<AuditEntry>> {
    let db = state.db.lock().await;
    let entries = audit::get_recent_activity(&db, limit.unwrap_or(50))
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(entries)
}

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<AppConfig> {
    let config = state.config.read().await;
    Ok(config.clone())
}

#[tauri::command]
pub async fn save_config(
    config: AppConfig,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    config::save_config(&config)
        .map_err(|e| GreenCubeError::Config(e.to_string()))?;
    let mut current = state.config.write().await;
    *current = config;
    Ok(())
}

#[tauri::command]
pub async fn get_docker_status(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    let docker = state.docker.read().await;
    Ok(serde_json::json!({
        "available": docker.is_some()
    }))
}
```

## APPENDIX C: CLAUDE.md

```markdown
# CLAUDE.md — GreenCube Project Rules

## What is this?
GreenCube is a Tauri 2.0 desktop app where AI agents live as persistent beings.
Rust backend + React/TypeScript frontend + SQLite + Docker sandboxing.

## Architecture
- Tauri 2.0 with axum HTTP server on localhost:9000
- SQLite database at ~/.greencube/greencube.db
- Config at ~/.greencube/config.toml
- Docker via bollard for sandboxed tool execution

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

## File Organization
- Business logic: src-tauri/src/{identity,memory,permissions,sandbox}/
- API layer: src-tauri/src/api/
- Tauri commands: src-tauri/src/commands.rs
- Frontend pages: src/pages/
- Frontend components: src/components/
- Shared types: src/lib/types.ts

## Known Limitations (v0.1)
- Private keys stored unencrypted in SQLite (encrypt in v0.2 via keyring crate)
- API key stored in plaintext in config.toml (same as Cursor, Claude Desktop, etc.)
- Keyword-based memory retrieval (semantic search in v0.2)
- No human-in-the-loop approval flow (v0.2)
- No agent-to-agent communication (v0.2)
- No streaming responses (v0.2)
- API server has no authentication (localhost-only binding)
- No graceful shutdown / container cleanup on app close (v0.2)
- No DELETE agent endpoint (v0.2)
- Cost estimation is rough ($0.01/1K tokens) — don't trust cost numbers
- Stop words list is English-only — memory recall may be poor in other languages
- Reputation range is 0.1–0.9, not 0.0–1.0 (by design: regression to mean)
```

---

*End of specification. This document contains everything needed to build GreenCube v0.1 in a single implementation pass.*
