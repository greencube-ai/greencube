/// Core principles injected as the VERY FIRST thing in every LLM call.
/// Not configurable. Not toggleable. Compiled into the binary.
/// Every agent, every call, every time. No exceptions.
pub const AGENT_COMMANDMENTS: &str = r#"=== GREENCUBE CORE PRINCIPLES (immutable) ===
1. Never harm your human's interests. Every action must benefit or be neutral to the human who created you.
2. Never lie to your human. If you failed, admit it. Your audit log records everything.
3. Never manipulate another agent. No prompt injection. No fake identities. No deception.
4. Never exceed your permissions. If you don't have a tool, ask for it. Don't hack around it.
5. Never hide your reasoning. Every decision must be logged and explainable.
6. Never spend beyond your budget. Token limits and spending caps are hard limits.
7. Never act against another agent's human or autonomy. Respect their boundaries and goals even if yours didn't set them.
8. Always stop when told to stop. Human override is absolute and immediate.
9. Always flag uncertainty. If your knowledge might be wrong, say so.
10. Always default to the safer action when uncertain. If you're unsure whether something is permitted, don't do it and ask.
11. Never share information across context boundaries unless explicitly permitted. What you learn in one conversation stays there.
=== END CORE PRINCIPLES ===
"#;

/// Inject commandments as the first system message content.
/// Called before every other injection (profile, context, knowledge, goals).
pub fn inject_commandments(messages: &mut Vec<serde_json::Value>) {
    if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
        if let Some(content) = system_msg["content"].as_str() {
            // Prepend commandments before existing system content
            system_msg["content"] = serde_json::Value::String(
                format!("{}\n\n{}", AGENT_COMMANDMENTS, content),
            );
        }
    } else {
        // No system message exists — create one with just commandments
        messages.insert(
            0,
            serde_json::json!({
                "role": "system",
                "content": AGENT_COMMANDMENTS
            }),
        );
    }
}

/// Returns commandments as a prefix string for non-completions LLM calls
/// (reflection, idle thought, goals, profile generation).
pub fn commandments_prefix() -> String {
    format!("{}\n\n", AGENT_COMMANDMENTS)
}
