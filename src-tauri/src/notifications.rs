use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub agent_id: String,
    pub content: String,
    pub notification_type: String, // insight, alert, question, achievement
    pub read: bool,
    pub created_at: String,
    pub source: Option<String>, // idle_thought, reflection, goal
}

pub fn create_notification(
    conn: &Connection,
    agent_id: &str,
    content: &str,
    notification_type: &str,
    source: &str,
) -> anyhow::Result<Notification> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO notifications (id, agent_id, content, notification_type, read, created_at, source)
         VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
        params![id, agent_id, content, notification_type, now, source],
    )?;
    Ok(Notification {
        id, agent_id: agent_id.into(), content: content.into(),
        notification_type: notification_type.into(), read: false,
        created_at: now, source: Some(source.into()),
    })
}

pub fn get_unread_count(conn: &Connection) -> anyhow::Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM notifications WHERE read = 0",
        [],
        |row| row.get(0),
    )?;
    Ok(count)
}

pub fn get_notifications(conn: &Connection, unread_only: bool, limit: i64) -> anyhow::Result<Vec<Notification>> {
    let sql = if unread_only {
        "SELECT id, agent_id, content, notification_type, read, created_at, source FROM notifications WHERE read = 0 ORDER BY created_at DESC LIMIT ?1"
    } else {
        "SELECT id, agent_id, content, notification_type, read, created_at, source FROM notifications ORDER BY created_at DESC LIMIT ?1"
    };
    let mut stmt = conn.prepare(sql)?;
    let entries = stmt.query_map(params![limit], |row| {
        Ok(Notification {
            id: row.get(0)?,
            agent_id: row.get(1)?,
            content: row.get(2)?,
            notification_type: row.get(3)?,
            read: row.get::<_, i64>(4)? == 1,
            created_at: row.get(5)?,
            source: row.get(6)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}

pub fn mark_read(conn: &Connection, id: &str) -> anyhow::Result<()> {
    conn.execute("UPDATE notifications SET read = 1 WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn dismiss_all(conn: &Connection) -> anyhow::Result<()> {
    conn.execute("UPDATE notifications SET read = 1 WHERE read = 0", [])?;
    Ok(())
}

/// Count notifications from idle thoughts for a specific agent today.
/// Used to enforce the max 3 notifications/day/agent from idle thoughts.
pub fn count_idle_notifications_today(conn: &Connection, agent_id: &str) -> anyhow::Result<i64> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM notifications WHERE agent_id = ?1 AND source = 'idle_thought' AND created_at LIKE ?2 || '%'",
        params![agent_id, today],
        |row| row.get(0),
    )?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_create_and_get_notifications() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        create_notification(&conn, &agent.id, "Found a pattern!", "insight", "idle_thought").expect("n1");
        create_notification(&conn, &agent.id, "API key expiring soon", "alert", "reflection").expect("n2");
        let all = get_notifications(&conn, false, 50).expect("get");
        assert_eq!(all.len(), 2);
        let unread = get_notifications(&conn, true, 50).expect("get");
        assert_eq!(unread.len(), 2);
    }

    #[test]
    fn test_unread_count() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        create_notification(&conn, &agent.id, "msg1", "insight", "idle_thought").expect("n1");
        create_notification(&conn, &agent.id, "msg2", "alert", "reflection").expect("n2");
        assert_eq!(get_unread_count(&conn).expect("count"), 2);
        mark_read(&conn, &get_notifications(&conn, false, 1).expect("get")[0].id).expect("mark");
        assert_eq!(get_unread_count(&conn).expect("count"), 1);
    }

    #[test]
    fn test_dismiss_all() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        create_notification(&conn, &agent.id, "msg1", "insight", "idle_thought").expect("n1");
        create_notification(&conn, &agent.id, "msg2", "alert", "reflection").expect("n2");
        dismiss_all(&conn).expect("dismiss");
        assert_eq!(get_unread_count(&conn).expect("count"), 0);
    }

    #[test]
    fn test_count_idle_notifications_today() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        create_notification(&conn, &agent.id, "thought1", "insight", "idle_thought").expect("n1");
        create_notification(&conn, &agent.id, "thought2", "question", "idle_thought").expect("n2");
        create_notification(&conn, &agent.id, "alert", "alert", "reflection").expect("n3"); // Not idle
        assert_eq!(count_idle_notifications_today(&conn, &agent.id).expect("count"), 2);
    }
}
