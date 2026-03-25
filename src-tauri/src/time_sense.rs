use chrono::{DateTime, Utc, Datelike, Timelike};

/// Convert an RFC3339 timestamp to relative time: "5 minutes ago", "2 hours ago", etc.
pub fn relative_time(timestamp: &str) -> String {
    let parsed = match DateTime::parse_from_rfc3339(timestamp) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => return "unknown time".into(),
    };
    let now = Utc::now();
    let duration = now.signed_duration_since(parsed);
    let seconds = duration.num_seconds();

    if seconds < 60 { return "just now".into(); }
    if seconds < 3600 {
        let mins = seconds / 60;
        return format!("{} minute{} ago", mins, if mins == 1 { "" } else { "s" });
    }
    if seconds < 86400 {
        let hours = seconds / 3600;
        return format!("{} hour{} ago", hours, if hours == 1 { "" } else { "s" });
    }
    if seconds < 86400 * 2 { return "yesterday".into(); }
    if seconds < 86400 * 7 {
        let days = seconds / 86400;
        return format!("{} days ago", days);
    }
    if seconds < 86400 * 30 {
        let weeks = seconds / (86400 * 7);
        return format!("{} week{} ago", weeks, if weeks == 1 { "" } else { "s" });
    }
    let months = seconds / (86400 * 30);
    format!("{} month{} ago", months, if months == 1 { "" } else { "s" })
}

/// Build full time context for an agent. Includes day of week.
pub fn time_context_for_agent(agent_updated_at: &str) -> String {
    let now = Utc::now();
    let day_name = match now.weekday() {
        chrono::Weekday::Mon => "Monday",
        chrono::Weekday::Tue => "Tuesday",
        chrono::Weekday::Wed => "Wednesday",
        chrono::Weekday::Thu => "Thursday",
        chrono::Weekday::Fri => "Friday",
        chrono::Weekday::Sat => "Saturday",
        chrono::Weekday::Sun => "Sunday",
    };
    let time_str = format!(
        "It is {}, {} at {:02}:{:02} UTC.",
        day_name,
        now.format("%B %d, %Y"),
        now.hour(),
        now.minute(),
    );
    let idle_str = format!("Your human last interacted with you {}.", relative_time(agent_updated_at));
    format!("{} {}", time_str, idle_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relative_time_recent() {
        let now = Utc::now();
        let recent = (now - chrono::Duration::seconds(30)).to_rfc3339();
        assert_eq!(relative_time(&recent), "just now");
    }

    #[test]
    fn test_relative_time_minutes() {
        let now = Utc::now();
        let mins_ago = (now - chrono::Duration::minutes(15)).to_rfc3339();
        assert_eq!(relative_time(&mins_ago), "15 minutes ago");
    }

    #[test]
    fn test_relative_time_hours() {
        let now = Utc::now();
        let hours_ago = (now - chrono::Duration::hours(3)).to_rfc3339();
        assert_eq!(relative_time(&hours_ago), "3 hours ago");
    }

    #[test]
    fn test_relative_time_yesterday() {
        let now = Utc::now();
        let yesterday = (now - chrono::Duration::hours(30)).to_rfc3339();
        assert_eq!(relative_time(&yesterday), "yesterday");
    }

    #[test]
    fn test_time_context_includes_day() {
        let now = Utc::now().to_rfc3339();
        let ctx = time_context_for_agent(&now);
        // Should contain a day of week
        assert!(
            ctx.contains("Monday") || ctx.contains("Tuesday") || ctx.contains("Wednesday")
            || ctx.contains("Thursday") || ctx.contains("Friday") || ctx.contains("Saturday")
            || ctx.contains("Sunday")
        );
        assert!(ctx.contains("UTC"));
    }
}
