use rusqlite::{params, Connection};

/// Build a trajectory summary for the agent — its growth story.
pub fn build_trajectory_summary(conn: &Connection, agent_id: &str) -> String {
    let mut lines = Vec::new();

    // Total tasks
    let total_tasks: i64 = conn.query_row(
        "SELECT total_tasks FROM agents WHERE id = ?1", params![agent_id], |row| row.get(0),
    ).unwrap_or(0);

    if total_tasks == 0 {
        return "No tasks completed yet.".to_string();
    }

    let success_rate: f64 = conn.query_row(
        "SELECT CASE WHEN total_tasks > 0 THEN CAST(successful_tasks AS REAL) / total_tasks ELSE 0 END FROM agents WHERE id = ?1",
        params![agent_id], |row| row.get(0),
    ).unwrap_or(0.0);

    lines.push(format!("You have completed {} tasks (overall success: {:.0}%).", total_tasks, success_rate * 100.0));

    // Competence trajectory — compare current vs what it was
    let mut stmt = conn.prepare(
        "SELECT domain, confidence, task_count, trend FROM competence_map WHERE agent_id = ?1 AND task_count >= 3 ORDER BY task_count DESC"
    ).ok();

    if let Some(ref mut stmt) = stmt {
        let domains: Vec<(String, f64, i64, String)> = stmt.query_map(params![agent_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?))
        }).ok()
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();

        for (domain, confidence, task_count, trend) in &domains {
            let pct = (confidence * 100.0) as i64;
            let trend_word = match trend.as_str() {
                "improving" => "improving",
                "declining" => "declining",
                _ => "stable",
            };
            lines.push(format!("- {} {:.0}% ({} tasks, {})", domain, pct, task_count, trend_word));
        }
    }

    // Task timing patterns
    let mut stmt2 = conn.prepare(
        "SELECT domain, day_of_week, hour, frequency FROM task_patterns WHERE agent_id = ?1 AND frequency >= 5 ORDER BY frequency DESC LIMIT 3"
    ).ok();

    if let Some(ref mut stmt) = stmt2 {
        let day_names = ["", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        let patterns: Vec<String> = stmt.query_map(params![agent_id], |row| {
            let domain: String = row.get(0)?;
            let day: i32 = row.get(1)?;
            let hour: i32 = row.get(2)?;
            let freq: i64 = row.get(3)?;
            let day_name = day_names.get(day as usize).unwrap_or(&"?");
            Ok(format!("{} tasks usually happen on {}s around {}:00 ({} times)", domain, day_name, hour, freq))
        }).ok()
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();

        if !patterns.is_empty() {
            lines.push("Timing patterns:".to_string());
            for p in patterns {
                lines.push(format!("- {}", p));
            }
        }
    }

    // Curiosities count
    let curiosity_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM curiosities WHERE agent_id = ?1 AND explored = 0",
        params![agent_id], |row| row.get(0),
    ).unwrap_or(0);

    if curiosity_count > 0 {
        lines.push(format!("You have {} unexplored curiosities.", curiosity_count));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_empty_trajectory() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let summary = build_trajectory_summary(&conn, &agent.id);
        assert!(summary.contains("No tasks"));
    }

    #[test]
    fn test_trajectory_with_tasks() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        // Increment tasks manually
        conn.execute("UPDATE agents SET total_tasks = 10, successful_tasks = 8 WHERE id = ?1",
            rusqlite::params![agent.id]).expect("update");
        let summary = build_trajectory_summary(&conn, &agent.id);
        assert!(summary.contains("10 tasks"));
    }
}
