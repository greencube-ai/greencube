use std::sync::Arc;

use crate::identity::Agent;
use crate::memory::episodic;
use crate::memory::Episode;
use crate::permissions;
use crate::state::AppState;

/// 2c. COMPETENCE WARNING: If agent is weak in detected domain but no specialist, warn
pub(super) async fn inject_competence_warning(
    state: &Arc<AppState>,
    agent: &Agent,
    body: &mut serde_json::Value,
) {
    let user_msg_lower = body["messages"]
        .as_array()
        .and_then(|msgs| msgs.iter().rev().find(|m| m["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("")
        .to_lowercase();

    let db = state.db.lock().await;
    let competence_map = crate::competence::get_competence_map(&db, &agent.id).unwrap_or_default();

    for entry in &competence_map {
        if entry.task_count >= 5 && entry.confidence < 0.50 && user_msg_lower.contains(&entry.domain) {
            let children = crate::spawn::get_children(&db, &agent.id);
            let has_specialist = children.iter().any(|c| c.2 == entry.domain);

            if !has_specialist {
                // Inject warning with specific past failures from knowledge
                let all_knowledge = crate::knowledge::list_knowledge(&db, &agent.id, 100).unwrap_or_default();
                let domain_warnings: Vec<String> = all_knowledge.iter()
                    .filter(|k| k.category == "warning" && k.content.to_lowercase().contains(&entry.domain))
                    .take(3)
                    .map(|k| format!("  - {}", k.content))
                    .collect();

                if let Some(messages) = body["messages"].as_array_mut() {
                    let mut warning = format!(
                        "\n\nWARNING: You have a {:.0}% success rate in {} ({} tasks).",
                        entry.confidence * 100.0, entry.domain, entry.task_count
                    );
                    if !domain_warnings.is_empty() {
                        warning.push_str(&format!("\nPast issues in {}:\n{}\nAvoid these specific mistakes.", entry.domain, domain_warnings.join("\n")));
                    } else {
                        warning.push_str(" Your response may need extra verification. Flag any uncertainty.");
                    }
                    if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                        if let Some(content) = system_msg["content"].as_str() {
                            system_msg["content"] = serde_json::Value::String(format!("{}{}", content, warning));
                        }
                    }
                }
            }
            break;
        }
    }
}

/// Inject relationship context if enough interactions
pub(super) async fn inject_relationship(
    state: &Arc<AppState>,
    agent: &Agent,
    user_id: &str,
    body: &mut serde_json::Value,
) {
    if let Some(messages) = body["messages"].as_array_mut() {
        let rel_prompt = {
            let db = state.db.lock().await;
            crate::relationships::get_relationship_prompt(&db, &agent.id, user_id)
        };
        if let Some(rel_text) = rel_prompt {
            if let Some(sys) = messages.iter_mut().find(|m| m["role"] == "system") {
                if let Some(c) = sys["content"].as_str() {
                    sys["content"] = serde_json::Value::String(format!("{}\n\n{}", c, rel_text));
                }
            }
        }
    }
}

/// 3b. INJECT LEARNED PREFERENCES + MISTAKES TO AVOID
pub(super) async fn inject_preferences_and_corrections(
    state: &Arc<AppState>,
    agent: &Agent,
    body: &mut serde_json::Value,
) {
    let db = state.db.lock().await;
    let all_knowledge = crate::knowledge::list_knowledge(&db, &agent.id, 100).unwrap_or_default();
    let preferences: Vec<String> = all_knowledge.iter()
        .filter(|k| k.category == "preference")
        .take(5)
        .map(|k| format!("- {}", k.content))
        .collect();
    let correction_entries: Vec<_> = all_knowledge.iter()
        .filter(|k| k.category == "correction")
        .take(3)
        .collect();
    if !correction_entries.is_empty() {
        crate::api::brain::increment_counter(&db, &agent.id, "corrections_applied");
    }
    let corrections: Vec<String> = correction_entries.iter()
        .map(|k| format!("- {}", k.content))
        .collect();
    if !preferences.is_empty() || !corrections.is_empty() {
        if let Some(messages) = body["messages"].as_array_mut() {
            let mut injection = String::new();
            if !preferences.is_empty() {
                injection.push_str(&format!(
                    "\n\n--- Apply these learned preferences ---\n{}\n--- End preferences ---",
                    preferences.join("\n")
                ));
            }
            if !corrections.is_empty() {
                injection.push_str(&format!(
                    "\n\n--- Mistakes to avoid (user feedback) ---\n{}\n--- End mistakes ---",
                    corrections.join("\n")
                ));
            }
            if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                if let Some(content) = system_msg["content"].as_str() {
                    system_msg["content"] = serde_json::Value::String(format!("{}{}", content, injection));
                }
            }
        }
    }
}

/// 3c. INJECT WORKING CONTEXT (scratchpad) + DYNAMIC PROFILE
pub(super) async fn inject_profile_goals_context(
    state: &Arc<AppState>,
    agent: &Agent,
    body: &mut serde_json::Value,
) {
    if let Some(messages) = body["messages"].as_array_mut() {
        let mut injections = Vec::new();

        // Dynamic profile (max 500 chars)
        if !agent.dynamic_profile.is_empty() {
            let profile: String = agent.dynamic_profile.chars().take(500).collect();
            injections.push(format!("--- Your profile ---\n{}\n--- End profile ---", profile));
        }

        // Goals + working context
        {
            let db = state.db.lock().await;

            // Active goals
            let active_goals = crate::goals::list_goals(&db, &agent.id, Some("active")).unwrap_or_default();
            if !active_goals.is_empty() {
                let goals_text = active_goals.iter()
                    .enumerate()
                    .map(|(i, g)| format!("{}. {}", i + 1, g.content))
                    .collect::<Vec<_>>().join("\n");
                injections.push(format!("--- Your current goals ---\n{}\n--- End goals ---", goals_text));
            }

            // Working context (max 1000 chars)
            let ctx = crate::context::get_context(&db, &agent.id).unwrap_or_default();
            if !ctx.is_empty() {
                injections.push(format!(
                    "--- Your working context (you can update this with the update_context tool) ---\n{}\n--- End working context ---",
                    ctx.chars().take(1000).collect::<String>()
                ));
            }
        }

        if !injections.is_empty() {
            let injection_text = injections.join("\n\n");
            if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                if let Some(content) = system_msg["content"].as_str() {
                    system_msg["content"] = serde_json::Value::String(format!("{}\n\n{}", content, injection_text));
                }
            } else {
                messages.insert(0, serde_json::json!({"role": "system", "content": injection_text}));
            }
        }
    }
}

/// 4. INJECT KNOWLEDGE — try semantic search via Ollama, fall back to keyword
pub(super) async fn inject_keyword_knowledge(
    state: &Arc<AppState>,
    agent: &Agent,
    body: &mut serde_json::Value,
) {
    let memory_mode = state.config.read().await.llm.memory_mode.clone();
    if memory_mode == "keyword" {
        if let Some(messages) = body["messages"].as_array_mut() {
            if let Some(last_user_msg) = messages.iter().rev().find(|m| m["role"] == "user").and_then(|m| m["content"].as_str()) {
                let db = state.db.lock().await;
                // Keyword matching for inline recall (semantic search available for batch via recall_smart)
                let knowledge = crate::knowledge::recall_relevant(&db, &agent.id, last_user_msg, 10).unwrap_or_default();
                if !knowledge.is_empty() {
                    // Track: facts used in tasks
                    crate::api::brain::increment_counter(&db, &agent.id, "facts_used");
                    let knowledge_text = knowledge.iter()
                        .map(|k| {
                            let valence_note = match k.valence {
                                -2 => " (very frustrating in past)",
                                -1 => " (was difficult)",
                                1 => " (went well before)",
                                2 => " (was easy/excellent)",
                                _ => "",
                            };
                            format!("- [{}] {}{}", k.category, k.content, valence_note)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let injection = format!("\n\n--- Things you know ---\n{}\n--- End knowledge ---", knowledge_text);
                    if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                        if let Some(content) = system_msg["content"].as_str() {
                            system_msg["content"] = serde_json::Value::String(format!("{}{}", content, injection));
                        }
                    }
                } else {
                    // Fall back to episode-based recall if no knowledge entries yet
                    if let Ok(memories) = episodic::recall_relevant_episodes(&db, &agent.id, last_user_msg, 5) {
                        inject_memories(messages, &memories);
                    }
                }
            }
        }
    }
}

/// 4b. CROSS-AGENT LEARNING: inject relevant knowledge from other agents in the habitat
pub(super) async fn inject_habitat_knowledge(
    state: &Arc<AppState>,
    agent: &Agent,
    body: &mut serde_json::Value,
) {
    if let Some(last_user_msg) = body["messages"].as_array()
        .and_then(|msgs| msgs.iter().rev().find(|m| m["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .map(|s| s.to_string())
    {
        let db = state.db.lock().await;
        let habitat_knowledge = crate::knowledge::recall_habitat_knowledge(&db, &agent.id, &last_user_msg, 3).unwrap_or_default();
        drop(db);

        if !habitat_knowledge.is_empty() {
            let habitat_text = habitat_knowledge.iter()
                .map(|(agent_name, k)| format!("- [from {}] {}", agent_name, k.content))
                .collect::<Vec<_>>()
                .join("\n");

            if let Some(messages) = body["messages"].as_array_mut() {
                let injection = format!("\n\n--- Knowledge from other agents in your habitat ---\n{}\n--- End habitat knowledge ---", habitat_text);
                if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                    if let Some(content) = system_msg["content"].as_str() {
                        system_msg["content"] = serde_json::Value::String(format!("{}{}", content, injection));
                    }
                }
            }
        }
    }
}

/// 5. INJECT TOOL DEFINITIONS + tool-usage hint to system prompt
pub(super) async fn inject_tools_and_hint(
    _state: &Arc<AppState>,
    agent: &Agent,
    body: &mut serde_json::Value,
) {
    // If the agent has tools but the request doesn't include tool definitions,
    // inject them so the LLM knows what tools are available.
    if !agent.tools_allowed.is_empty() {
        let tool_defs = super::tool_defs::build_tool_definitions(&agent.tools_allowed);
        if !tool_defs.is_empty() {
            if body.get("tools").and_then(|t| t.as_array()).map_or(true, |a| a.is_empty()) {
                // No tools in request — inject agent's tools
                body["tools"] = serde_json::Value::Array(tool_defs);
            } else {
                // Client sent tools — filter to only those the agent is allowed
                let filtered: Vec<serde_json::Value> = body["tools"].as_array().unwrap().iter()
                    .filter(|t| t["function"]["name"].as_str().map(|name| permissions::check_tool_permission(agent, name)).unwrap_or(false))
                    .cloned()
                    .collect();
                body["tools"] = serde_json::Value::Array(filtered);
            }
        }
    }

    // Add tool usage hint to system prompt when tools are available
    let has_injected_tools = body.get("tools").and_then(|t| t.as_array()).map_or(false, |a| !a.is_empty());
    if has_injected_tools {
        let tool_names: Vec<String> = body["tools"].as_array()
            .map(|arr| arr.iter()
                .filter_map(|t| t["function"]["name"].as_str().map(|s| s.to_string()))
                .collect())
            .unwrap_or_default();

        if let Some(messages) = body["messages"].as_array_mut() {
            let mut hint = format!(
                "\n\nYou have access to these tools: {}. When the user asks you to perform an action that matches a tool, you MUST call that tool. Do not describe what you would do — actually do it.",
                tool_names.join(", ")
            );

            // Detect send_message intent — force tool usage
            let last_user_msg = messages.iter().rev()
                .find(|m| m["role"] == "user")
                .and_then(|m| m["content"].as_str())
                .unwrap_or("")
                .to_lowercase();
            let msg_patterns = ["send_message", "send a message", "ask ", "tell ", "message ", "talk to "];
            if tool_names.contains(&"send_message".to_string()) && msg_patterns.iter().any(|p| last_user_msg.contains(p)) {
                hint.push_str("\n\nIMPORTANT: The user is requesting communication with another agent. You MUST use the send_message tool. Do NOT answer the question yourself — delegate it to the other agent.");
            }

            if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                if let Some(content) = system_msg["content"].as_str() {
                    system_msg["content"] = serde_json::Value::String(format!("{}{}", content, hint));
                }
            }
        }
    }
}

fn inject_memories(messages: &mut Vec<serde_json::Value>, memories: &[Episode]) {
    if memories.is_empty() { return; }
    let memory_text = memories.iter()
        .map(|ep| format!("[Memory from {}] {}: {}", ep.created_at, ep.event_type, ep.summary))
        .collect::<Vec<_>>()
        .join("\n");
    let injection = format!("\n\n--- Relevant memories from past tasks ---\n{}\n--- End memories ---", memory_text);
    if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
        if let Some(content) = system_msg["content"].as_str() {
            system_msg["content"] = serde_json::Value::String(format!("{}{}", content, injection));
        }
    } else {
        messages.insert(0, serde_json::json!({"role": "system", "content": injection}));
    }
}
