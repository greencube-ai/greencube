use std::path::Path;

use rusqlite::{params, Connection};
use serde::Serialize;

pub struct Db {
    conn: Connection,
}

#[derive(Serialize, Clone, Debug)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub updated_at: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Memory {
    pub id: i64,
    pub content: String,
    pub created_at: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct StoredMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS conversations (
                 id         TEXT    PRIMARY KEY,
                 title      TEXT    NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS messages (
                 id              INTEGER PRIMARY KEY AUTOINCREMENT,
                 conversation_id TEXT    NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                 role            TEXT    NOT NULL,
                 content         TEXT    NOT NULL,
                 created_at      INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS memories (
                 id         INTEGER PRIMARY KEY AUTOINCREMENT,
                 content    TEXT    NOT NULL,
                 created_at INTEGER NOT NULL
             );",
        )?;
        Ok(Db { conn })
    }

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    pub fn create_conversation(&self, title: &str) -> anyhow::Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Self::now();
        self.conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![id, title, now],
        )?;
        Ok(id)
    }

    pub fn add_message(&self, conversation_id: &str, role: &str, content: &str) -> anyhow::Result<()> {
        let now = Self::now();
        self.conn.execute(
            "INSERT INTO messages (conversation_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![conversation_id, role, content, now],
        )?;
        self.conn.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            params![now, conversation_id],
        )?;
        Ok(())
    }

    pub fn list_conversations(&self) -> anyhow::Result<Vec<ConversationSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, updated_at FROM conversations ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ConversationSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn load_messages(&self, conversation_id: &str) -> anyhow::Result<Vec<StoredMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, role, content FROM messages WHERE conversation_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![conversation_id], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_conversation(&self, id: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM conversations WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_memories(&self) -> anyhow::Result<Vec<Memory>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, created_at FROM memories ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Memory {
                id: row.get(0)?,
                content: row.get(1)?,
                created_at: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn add_memory(&self, content: &str) -> anyhow::Result<Memory> {
        let now = Self::now();
        self.conn.execute(
            "INSERT INTO memories (content, created_at) VALUES (?1, ?2)",
            params![content, now],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Memory { id, content: content.to_string(), created_at: now })
    }

    pub fn delete_memory(&self, id: i64) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }
}
