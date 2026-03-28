use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drive {
    pub drive_name: String,
    pub energy: f64,
    pub threshold: f64,
    pub last_discharged_at: String,
}

#[derive(Debug, Clone)]
pub enum DriveAction {
    ExploreCuriosity,      // curiosity_energy exceeded
    ReachOutToAgent,       // social_energy exceeded
    ForceSelfVerify,       // verification_energy exceeded
}

/// Initialize drives for an agent if they don't exist.
pub fn ensure_drives(conn: &Connection, agent_id: &str) -> anyhow::Result<()> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM drives WHERE agent_id = ?1", params![agent_id], |r| r.get(0),
    ).unwrap_or(0);

    if count == 0 {
        let now = chrono::Utc::now().to_rfc3339();
        for (name, threshold) in &[("curiosity", 1.0), ("social", 1.5), ("verification", 1.0)] {
            conn.execute(
                "INSERT INTO drives (agent_id, drive_name, energy, threshold, last_discharged_at) VALUES (?1, ?2, 0.0, ?3, ?4)",
                params![agent_id, name, threshold, now],
            )?;
        }
    }
    Ok(())
}

/// Increase a drive's energy.
pub fn charge_drive(conn: &Connection, agent_id: &str, drive_name: &str, amount: f64) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE drives SET energy = MIN(energy + ?1, threshold * 2.0) WHERE agent_id = ?2 AND drive_name = ?3",
        params![amount, agent_id, drive_name],
    )?;
    Ok(())
}

/// Discharge a drive (reset energy to 0).
pub fn discharge_drive(conn: &Connection, agent_id: &str, drive_name: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE drives SET energy = 0.0, last_discharged_at = ?1 WHERE agent_id = ?2 AND drive_name = ?3",
        params![now, agent_id, drive_name],
    )?;
    Ok(())
}

/// Check which drives have exceeded their thresholds.
pub fn check_drives(conn: &Connection, agent_id: &str) -> Vec<DriveAction> {
    let mut actions = Vec::new();

    let mut stmt = conn.prepare(
        "SELECT drive_name, energy, threshold FROM drives WHERE agent_id = ?1 AND energy >= threshold"
    ).ok();

    if let Some(ref mut stmt) = stmt {
        let rows: Vec<(String, f64, f64)> = stmt.query_map(params![agent_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?, row.get::<_, f64>(2)?))
        }).ok()
            .map(|r| r.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();

        for (name, energy, _threshold) in rows {
            match name.as_str() {
                "curiosity" => {
                    actions.push(DriveAction::ExploreCuriosity);
                    tracing::info!("Drive fired: curiosity (energy: {:.1})", energy);
                }
                "social" => {
                    actions.push(DriveAction::ReachOutToAgent);
                    tracing::info!("Drive fired: social (energy: {:.1})", energy);
                }
                "verification" => {
                    actions.push(DriveAction::ForceSelfVerify);
                    tracing::info!("Drive fired: verification (energy: {:.1})", energy);
                }
                _ => {}
            }
        }
    }

    actions
}

/// Get all drives for display.
pub fn get_drives(conn: &Connection, agent_id: &str) -> Vec<Drive> {
    let mut stmt = conn.prepare(
        "SELECT drive_name, energy, threshold, last_discharged_at FROM drives WHERE agent_id = ?1"
    ).ok();

    stmt.as_mut().map(|s| {
        s.query_map(params![agent_id], |row| {
            Ok(Drive {
                drive_name: row.get(0)?, energy: row.get(1)?,
                threshold: row.get(2)?, last_discharged_at: row.get(3)?,
            })
        }).ok()
            .map(|r| r.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_ensure_drives_creates_three() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        ensure_drives(&conn, &agent.id).expect("ensure");
        let drives = get_drives(&conn, &agent.id);
        assert_eq!(drives.len(), 3);
        assert!(drives.iter().any(|d| d.drive_name == "curiosity"));
        assert!(drives.iter().any(|d| d.drive_name == "social"));
        assert!(drives.iter().any(|d| d.drive_name == "verification"));
    }

    #[test]
    fn test_charge_and_check_drives() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        ensure_drives(&conn, &agent.id).expect("ensure");
        // Charge curiosity past threshold (0.5 * 3 = 1.5 > 1.0)
        for _ in 0..3 { charge_drive(&conn, &agent.id, "curiosity", 0.5).expect("charge"); }
        let actions = check_drives(&conn, &agent.id);
        assert!(actions.iter().any(|a| matches!(a, DriveAction::ExploreCuriosity)));
    }

    #[test]
    fn test_discharge_resets_energy() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        ensure_drives(&conn, &agent.id).expect("ensure");
        charge_drive(&conn, &agent.id, "curiosity", 2.0).expect("charge");
        discharge_drive(&conn, &agent.id, "curiosity").expect("discharge");
        let drives = get_drives(&conn, &agent.id);
        let curiosity = drives.iter().find(|d| d.drive_name == "curiosity").unwrap();
        assert_eq!(curiosity.energy, 0.0);
    }

    #[test]
    fn test_no_actions_below_threshold() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        ensure_drives(&conn, &agent.id).expect("ensure");
        charge_drive(&conn, &agent.id, "curiosity", 0.5).expect("charge");
        let actions = check_drives(&conn, &agent.id);
        assert!(actions.is_empty());
    }
}
