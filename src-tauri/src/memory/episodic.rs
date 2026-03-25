use rusqlite::{params, Connection};

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
    if let Some(tid) = task_id {
        let mut stmt = conn.prepare(
            "SELECT id, agent_id, created_at, event_type, summary, raw_data, task_id, outcome, tokens_used, cost_cents
             FROM episodes WHERE agent_id = ?1 AND task_id = ?2
             ORDER BY created_at DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![agent_id, tid, limit], map_episode)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, agent_id, created_at, event_type, summary, raw_data, task_id, outcome, tokens_used, cost_cents
             FROM episodes WHERE agent_id = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![agent_id, limit], map_episode)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

pub fn recall_relevant_episodes(
    conn: &Connection,
    agent_id: &str,
    query: &str,
    limit: i64,
) -> anyhow::Result<Vec<Episode>> {
    // Extract significant words (>3 chars, lowercase, no stop words)
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
    let mut conditions = Vec::new();
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(agent_id.to_string())];

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
                ({score}) as relevance_score
         FROM episodes
         WHERE agent_id = ?1 AND ({score}) > 0
         ORDER BY relevance_score DESC, created_at DESC
         LIMIT {limit}",
        score = score_expr,
        limit = limit
    );

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        all_params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_refs.as_slice(), map_episode)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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

const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "had", "her", "was", "one",
    "our", "out", "has", "have", "that", "this", "with", "they", "been", "from", "will", "what",
    "when", "make", "like", "just", "over", "such", "take", "than", "them", "very", "some",
    "could", "into", "other", "then", "these", "would",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    fn setup() -> (Connection, String) {
        let conn = init_memory_database().expect("init");
        let agent =
            create_agent(&conn, "MemBot", "", &["shell".into()]).expect("create");
        (conn, agent.id)
    }

    fn make_episode(agent_id: &str, id: &str, summary: &str) -> Episode {
        Episode {
            id: id.into(),
            agent_id: agent_id.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            event_type: "tool_call".into(),
            summary: summary.into(),
            raw_data: None,
            task_id: Some("task-1".into()),
            outcome: Some("success".into()),
            tokens_used: 0,
            cost_cents: 0,
        }
    }

    #[test]
    fn test_insert_episode() {
        let (conn, agent_id) = setup();
        let ep = make_episode(&agent_id, "ep1", "Ran ls command");
        insert_episode(&conn, &ep).expect("insert");
        let episodes = get_episodes(&conn, &agent_id, 50, None).expect("get");
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].summary, "Ran ls command");
    }

    #[test]
    fn test_get_episodes_limit() {
        let (conn, agent_id) = setup();
        for i in 0..10 {
            let ep = make_episode(&agent_id, &format!("ep{}", i), &format!("Event {}", i));
            insert_episode(&conn, &ep).expect("insert");
        }
        let episodes = get_episodes(&conn, &agent_id, 5, None).expect("get");
        assert_eq!(episodes.len(), 5);
    }

    #[test]
    fn test_get_episodes_by_task() {
        let (conn, agent_id) = setup();
        let mut ep1 = make_episode(&agent_id, "ep1", "Task A event");
        ep1.task_id = Some("task-a".into());
        insert_episode(&conn, &ep1).expect("insert");

        let mut ep2 = make_episode(&agent_id, "ep2", "Task B event");
        ep2.task_id = Some("task-b".into());
        insert_episode(&conn, &ep2).expect("insert");

        let results = get_episodes(&conn, &agent_id, 50, Some("task-a")).expect("get");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].summary, "Task A event");
    }

    #[test]
    fn test_recall_relevant() {
        let (conn, agent_id) = setup();
        insert_episode(&conn, &make_episode(&agent_id, "ep1", "Called the Stripe API with bearer token")).expect("insert");
        insert_episode(&conn, &make_episode(&agent_id, "ep2", "Read a CSV file from disk")).expect("insert");
        insert_episode(&conn, &make_episode(&agent_id, "ep3", "Stripe payment processing failed")).expect("insert");

        let results = recall_relevant_episodes(&conn, &agent_id, "How to use Stripe API?", 5).expect("recall");
        assert!(!results.is_empty());
        // Both Stripe-related episodes should match
        assert!(results.iter().any(|e| e.summary.contains("Stripe API")));
    }

    #[test]
    fn test_recall_no_match() {
        let (conn, agent_id) = setup();
        insert_episode(&conn, &make_episode(&agent_id, "ep1", "Read a file")).expect("insert");
        let results = recall_relevant_episodes(&conn, &agent_id, "quantum computing theory", 5).expect("recall");
        assert!(results.is_empty());
    }

    #[test]
    fn test_memory_isolation() {
        let conn = init_memory_database().expect("init");
        let agent_a = create_agent(&conn, "AgentA", "", &["shell".into()]).expect("create");
        let agent_b = create_agent(&conn, "AgentB", "", &["shell".into()]).expect("create");

        insert_episode(&conn, &make_episode(&agent_a.id, "ep1", "A's memory")).expect("insert");
        insert_episode(&conn, &make_episode(&agent_b.id, "ep2", "B's memory")).expect("insert");

        let a_episodes = get_episodes(&conn, &agent_a.id, 50, None).expect("get");
        assert_eq!(a_episodes.len(), 1);
        assert_eq!(a_episodes[0].summary, "A's memory");

        let b_episodes = get_episodes(&conn, &agent_b.id, 50, None).expect("get");
        assert_eq!(b_episodes.len(), 1);
        assert_eq!(b_episodes[0].summary, "B's memory");
    }

    #[test]
    fn test_stop_words_filtered() {
        let (conn, agent_id) = setup();
        insert_episode(&conn, &make_episode(&agent_id, "ep1", "Something relevant")).expect("insert");
        // Query with only stop words (all <=3 chars or in stop list)
        let results = recall_relevant_episodes(&conn, &agent_id, "the and for are", 5).expect("recall");
        assert!(results.is_empty());
    }
}
