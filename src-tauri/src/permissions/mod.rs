pub mod audit;

use crate::identity::Agent;

/// Available tools in v0.1
#[allow(dead_code)] // Used by identity::registry::VALID_TOOLS; kept here for API reference
pub const AVAILABLE_TOOLS: &[&str] = &["shell", "read_file", "write_file", "http_get"];

pub fn check_tool_permission(agent: &Agent, tool_name: &str) -> bool {
    agent.tools_allowed.iter().any(|t| t == tool_name)
}

#[allow(dead_code)] // Will be used when spending caps are enforced in completions proxy
pub fn check_spending_cap(agent: &Agent, additional_cents: i64) -> bool {
    if agent.max_spend_cents == 0 {
        return true; // 0 = unlimited
    }
    agent.total_spend_cents + additional_cents <= agent.max_spend_cents
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Agent;

    fn make_agent(tools: Vec<String>, max_spend: i64, total_spend: i64) -> Agent {
        Agent {
            id: "test".into(),
            name: "test".into(),
            created_at: "".into(),
            updated_at: "".into(),
            status: "idle".into(),
            system_prompt: "".into(),
            public_key: vec![],
            private_key: vec![],
            tools_allowed: tools,
            max_spend_cents: max_spend,
            total_tasks: 0,
            successful_tasks: 0,
            total_spend_cents: total_spend,
            provider_id: None,
            dynamic_profile: String::new(),
        }
    }

    #[test]
    fn test_check_tool_allowed() {
        let agent = make_agent(vec!["shell".into()], 0, 0);
        assert!(check_tool_permission(&agent, "shell"));
    }

    #[test]
    fn test_check_tool_denied() {
        let agent = make_agent(vec!["shell".into()], 0, 0);
        assert!(!check_tool_permission(&agent, "http_get"));
    }

    #[test]
    fn test_spending_cap_unlimited() {
        let agent = make_agent(vec![], 0, 500);
        assert!(check_spending_cap(&agent, 99999));
    }

    #[test]
    fn test_spending_cap_within() {
        let agent = make_agent(vec![], 100, 50);
        assert!(check_spending_cap(&agent, 30));
    }

    #[test]
    fn test_spending_cap_exceeded() {
        let agent = make_agent(vec![], 100, 80);
        assert!(!check_spending_cap(&agent, 30));
    }
}
