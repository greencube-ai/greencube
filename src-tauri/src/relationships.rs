use rusqlite::{params, Connection};

/// Update relationship after a task interaction.
pub fn record_interaction(conn: &Connection, agent_id: &str, user_id: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let updated = conn.execute(
        "UPDATE relationships SET interactions = interactions + 1, last_interaction_at = ?1 WHERE agent_id = ?2 AND user_identifier = ?3",
        params![now, agent_id, user_id],
    )?;
    if updated == 0 {
        conn.execute(
            "INSERT INTO relationships (agent_id, user_identifier, interactions, positive_signals, negative_signals, notes, last_interaction_at)
             VALUES (?1, ?2, 1, 0, 0, '', ?3)",
            params![agent_id, user_id, now],
        )?;
    }
    Ok(())
}

/// Record a positive or negative signal (from thumbs up/down).
pub fn record_signal(conn: &Connection, agent_id: &str, user_id: &str, positive: bool) -> anyhow::Result<()> {
    if positive {
        conn.execute(
            "UPDATE relationships SET positive_signals = positive_signals + 1 WHERE agent_id = ?1 AND user_identifier = ?2",
            params![agent_id, user_id],
        )?;
    } else {
        conn.execute(
            "UPDATE relationships SET negative_signals = negative_signals + 1 WHERE agent_id = ?1 AND user_identifier = ?2",
            params![agent_id, user_id],
        )?;
    }
    Ok(())
}

/// Update relationship notes (called from reflection when user preferences are learned).
pub fn update_notes(conn: &Connection, agent_id: &str, user_id: &str, note: &str) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE relationships SET notes = ?1 WHERE agent_id = ?2 AND user_identifier = ?3",
        params![note, agent_id, user_id],
    )?;
    Ok(())
}

/// Get relationship context for injection into prompts.
pub fn get_relationship_prompt(conn: &Connection, agent_id: &str, user_id: &str) -> Option<String> {
    let result = conn.query_row(
        "SELECT interactions, positive_signals, negative_signals, notes FROM relationships WHERE agent_id = ?1 AND user_identifier = ?2",
        params![agent_id, user_id],
        |row| {
            let interactions: i64 = row.get(0)?;
            let positive: i64 = row.get(1)?;
            let negative: i64 = row.get(2)?;
            let notes: String = row.get(3)?;
            Ok((interactions, positive, negative, notes))
        },
    );

    match result {
        Ok((interactions, positive, negative, notes)) if interactions >= 3 => {
            let mut prompt = format!("You have interacted with this user {} times.", interactions);
            if positive > 0 || negative > 0 {
                prompt.push_str(&format!(" They have given you {} positive and {} negative signals.", positive, negative));
            }
            if positive > negative * 2 {
                prompt.push_str(" This user generally appreciates your work.");
            } else if negative > positive {
                prompt.push_str(" This user has been critical. Be extra careful and thorough.");
            }
            if !notes.is_empty() {
                prompt.push_str(&format!(" Notes: {}", notes));
            }
            Some(prompt)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_record_and_get_relationship() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        record_interaction(&conn, &agent.id, "user123").expect("r1");
        record_interaction(&conn, &agent.id, "user123").expect("r2");
        record_interaction(&conn, &agent.id, "user123").expect("r3");
        record_signal(&conn, &agent.id, "user123", true).expect("pos");
        let prompt = get_relationship_prompt(&conn, &agent.id, "user123");
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("3 times"));
    }

    #[test]
    fn test_no_prompt_for_strangers() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        record_interaction(&conn, &agent.id, "new_user").expect("r1");
        let prompt = get_relationship_prompt(&conn, &agent.id, "new_user");
        assert!(prompt.is_none()); // needs 3+ interactions
    }
}
