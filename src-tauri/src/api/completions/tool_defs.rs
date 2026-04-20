/// Build OpenAI-format tool definitions for the agent's allowed tools.
pub(super) fn build_tool_definitions(tools_allowed: &[String]) -> Vec<serde_json::Value> {
    let all_tools: Vec<(&str, &str, serde_json::Value)> = vec![
        ("shell", "Execute a shell command directly on the host machine. The output is returned to you.", serde_json::json!({
            "type": "object",
            "properties": {"command": {"type": "string", "description": "The shell command to execute"}},
            "required": ["command"]
        })),
        ("read_file", "Read the contents of a file on the host filesystem. The contents are returned to you.", serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string", "description": "Path to the file to read"}},
            "required": ["path"]
        })),
        ("write_file", "Write content to a file on the host filesystem. The file is created or overwritten.", serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to write to"},
                "content": {"type": "string", "description": "Content to write"}
            },
            "required": ["path", "content"]
        })),
        ("http_get", "Make an HTTP GET request to a URL.", serde_json::json!({
            "type": "object",
            "properties": {"url": {"type": "string", "description": "The URL to fetch"}},
            "required": ["url"]
        })),
        ("update_context", "Update your working notes/scratchpad. Use this to save important context for future tasks.", serde_json::json!({
            "type": "object",
            "properties": {"content": {"type": "string", "description": "The new content for your working notes"}},
            "required": ["content"]
        })),
        ("send_message", "Send a message to another agent and get their response.", serde_json::json!({
            "type": "object",
            "properties": {
                "to": {"type": "string", "description": "Name of the agent to message"},
                "content": {"type": "string", "description": "The message to send"}
            },
            "required": ["to", "content"]
        })),
        ("set_reminder", "Set a reminder to do something later. The task will execute automatically at the specified time.", serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {"type": "string", "description": "What to do when the reminder fires"},
                "minutes_from_now": {"type": "integer", "description": "Minutes from now to execute (max 1440)"}
            },
            "required": ["prompt", "minutes_from_now"]
        })),
        ("spawn_specialist", "Create a specialist agent for a domain you struggle with. The specialist inherits your knowledge in that domain.", serde_json::json!({
            "type": "object",
            "properties": {"domain": {"type": "string", "description": "The domain to specialize in (e.g., 'css', 'python', 'database')"}},
            "required": ["domain"]
        })),
        ("fork_agent", "Split into two copies to try different approaches in parallel. Both run independently, then the best result wins.", serde_json::json!({
            "type": "object",
            "properties": {
                "reason": {"type": "string", "description": "Why you're forking (what problem you're exploring)"},
                "branch_a": {"type": "string", "description": "Description of approach A"},
                "branch_b": {"type": "string", "description": "Description of approach B"}
            },
            "required": ["reason", "branch_a", "branch_b"]
        })),
    ];

    all_tools.into_iter()
        .filter(|(name, _, _)| tools_allowed.iter().any(|t| t == name))
        .map(|(name, description, parameters)| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": parameters
                }
            })
        })
        .collect()
}
