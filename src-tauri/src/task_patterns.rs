use rusqlite::{params, Connection};

/// Record a task's timing for pattern detection.
pub fn record_task_timing(conn: &Connection, agent_id: &str, domain: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now();
    let day = now.format("%u").to_string().parse::<i32>().unwrap_or(1); // 1=Monday, 7=Sunday
    let hour = now.format("%H").to_string().parse::<i32>().unwrap_or(0);
    let now_str = now.to_rfc3339();

    // Try to update existing pattern
    let updated = conn.execute(
        "UPDATE task_patterns SET frequency = frequency + 1, last_seen = ?1
         WHERE agent_id = ?2 AND domain = ?3 AND day_of_week = ?4 AND hour = ?5",
        params![now_str, agent_id, domain, day, hour],
    )?;

    if updated == 0 {
        conn.execute(
            "INSERT INTO task_patterns (id, agent_id, domain, day_of_week, hour, frequency, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
            params![uuid::Uuid::new_v4().to_string(), agent_id, domain, day, hour, now_str],
        )?;
    }
    Ok(())
}

/// Get strong patterns (5+ frequency) for an agent.
pub fn get_strong_patterns(conn: &Connection, agent_id: &str) -> anyhow::Result<Vec<(String, i32, i32, i64)>> {
    // Returns: (domain, day_of_week, hour, frequency)
    let mut stmt = conn.prepare(
        "SELECT domain, day_of_week, hour, frequency FROM task_patterns
         WHERE agent_id = ?1 AND frequency >= 5
         ORDER BY frequency DESC LIMIT 5"
    )?;
    let rows = stmt.query_map(params![agent_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?, row.get::<_, i32>(2)?, row.get::<_, i64>(3)?))
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Format patterns for the idle thinker prompt.
pub fn format_patterns_for_prompt(patterns: &[(String, i32, i32, i64)]) -> String {
    if patterns.is_empty() {
        return String::new();
    }
    let day_names = ["", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let lines: Vec<String> = patterns.iter().map(|(domain, day, hour, freq)| {
        let day_name = day_names.get(*day as usize).unwrap_or(&"unknown");
        format!("- {} tasks usually happen on {}s around {}:00 ({} times)", domain, day_name, hour, freq)
    }).collect();
    format!("\nTask patterns you've noticed:\n{}", lines.join("\n"))
}
