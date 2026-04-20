use crate::state::AppState;

pub(super) async fn execute_tool_call(state: &AppState, agent_id: &str, tool_name: &str, arguments: &serde_json::Value) -> String {
    // Tool memory: check for recent identical calls
    let previous = {
        let db = state.db.lock().await;
        crate::tool_memory::lookup_recent(&db, agent_id, tool_name, arguments).ok().flatten()
    };

    let mut prefix = String::new();
    if let Some(ref prev) = previous {
        if prev.success {
            prefix = format!("[Note: identical call was made at {} and returned: {}]\n\n", prev.created_at, prev.result.chars().take(200).collect::<String>());
        } else {
            prefix = format!("[Warning: this same call failed at {} with: {}]\n\n", prev.created_at, prev.result.chars().take(200).collect::<String>());
        }
    }

    let result = match tool_name {
        "shell" => {
            let command = match arguments["command"].as_str() {
                Some(c) => c,
                None => return "Error: shell tool requires 'command' argument".into(),
            };
            execute_shell_direct(command).await
        }
        "read_file" => {
            let path = match arguments["path"].as_str() {
                Some(p) => p,
                None => return "Error: read_file requires 'path' argument".into(),
            };
            match tokio::fs::read_to_string(path).await {
                Ok(content) => content,
                Err(e) => format!("Error reading file: {}", e),
            }
        }
        "write_file" => {
            let path = match arguments["path"].as_str() {
                Some(p) => p,
                None => return "Error: write_file requires 'path' argument".into(),
            };
            let content = match arguments["content"].as_str() {
                Some(c) => c,
                None => return "Error: write_file requires 'content' argument".into(),
            };
            match tokio::fs::write(path, content).await {
                Ok(()) => format!("File written: {}", path),
                Err(e) => format!("Error writing file: {}", e),
            }
        }
        "http_get" => {
            let url = match arguments["url"].as_str() {
                Some(u) => u,
                None => return "Error: http_get requires 'url' argument".into(),
            };
            match reqwest::Client::new().get(url).timeout(std::time::Duration::from_secs(15)).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    match resp.text().await {
                        Ok(body) => {
                            let truncated: String = body.chars().take(5000).collect();
                            format!("HTTP {}\n{}", status, truncated)
                        }
                        Err(e) => format!("HTTP {} (body read error: {})", status, e),
                    }
                }
                Err(e) => format!("Error: {}", e),
            }
        }
        "update_context" => {
            let content = match arguments["content"].as_str() {
                Some(c) => c,
                None => return "Error: update_context requires 'content' argument".into(),
            };
            let db = state.db.lock().await;
            match crate::context::set_context(&db, agent_id, content) {
                Ok(()) => "Context updated successfully.".into(),
                Err(e) => format!("Error updating context: {}", e),
            }
        }
        "set_reminder" => {
            let prompt = match arguments["prompt"].as_str() {
                Some(p) => p,
                None => return "Error: set_reminder requires 'prompt' argument".into(),
            };
            let minutes = arguments["minutes_from_now"].as_i64().unwrap_or(60);
            let provider_id = {
                let db = state.db.lock().await;
                let agent = crate::identity::registry::get_agent(&db, agent_id).ok().flatten();
                agent.and_then(|a| a.provider_id)
            };
            let db = state.db.lock().await;
            match crate::task_queue::queue_reminder(&db, agent_id, prompt, minutes, provider_id.as_deref()) {
                Ok(_) => format!("Reminder set. I will execute '{}' in {} minutes.", prompt, minutes),
                Err(e) => format!("Error setting reminder: {}", e),
            }
        }
        "spawn_specialist" => {
            let domain = match arguments["domain"].as_str() {
                Some(d) => d,
                None => return "Error: spawn_specialist requires 'domain' argument".into(),
            };
            match crate::spawn::execute_spawn(state, agent_id, domain).await {
                Ok(child_name) => format!(
                    "Successfully created specialist: {}. You can delegate {} tasks to them using send_message(to=\"{}\", content=\"...\").",
                    child_name, domain, child_name
                ),
                Err(e) => format!("Could not spawn specialist: {}", e),
            }
        }
        "send_message" => {
            let to_name = match arguments["to"].as_str() {
                Some(t) => t,
                None => return "Error: send_message requires 'to' argument (agent name)".into(),
            };
            let content = match arguments["content"].as_str() {
                Some(c) => c,
                None => return "Error: send_message requires 'content' argument".into(),
            };
            let depth = arguments["_depth"].as_u64().unwrap_or(0) as u32;
            match crate::agent_messages::send_message(state, agent_id, to_name, content, depth).await {
                Ok(response) => format!("Response from {}: {}", to_name, response),
                Err(e) => format!("Error sending message to {}: {}", to_name, e),
            }
        }
        "fork_agent" => {
            let reason = match arguments["reason"].as_str() {
                Some(r) => r,
                None => return "Error: fork_agent requires 'reason' argument".into(),
            };
            let branch_a = match arguments["branch_a"].as_str() {
                Some(a) => a,
                None => return "Error: fork_agent requires 'branch_a' argument".into(),
            };
            let branch_b = match arguments["branch_b"].as_str() {
                Some(b) => b,
                None => return "Error: fork_agent requires 'branch_b' argument".into(),
            };
            // Get the original messages from the current conversation context
            // We pass empty messages since the fork will use the branch descriptions as the task
            let fork_messages = vec![
                serde_json::json!({"role": "user", "content": reason}),
            ];

            match crate::fork::execute_fork(state, agent_id, reason, branch_a, branch_b, &fork_messages).await {
                Ok(result) => result,
                Err(e) => format!("Fork failed: {}", e),
            }
        }
        _ => format!("Error: unknown tool '{}'", tool_name),
    };

    // Store tool result for future memory
    if tool_name != "update_context" { // Don't cache context updates
        let success = !result.starts_with("Error:");
        let db = state.db.lock().await;
        let _ = crate::tool_memory::store_result(&db, agent_id, tool_name, arguments, &result, success);
    }

    format!("{}{}", prefix, result)
}

/// Execute a shell command directly on the host machine.
async fn execute_shell_direct(command: &str) -> String {
    use tokio::process::Command;

    let shell = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
    let flag = if cfg!(target_os = "windows") { "/C" } else { "-c" };

    match Command::new(shell)
        .arg(flag)
        .arg(command)
        .output()
        .await
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let code = output.status.code().unwrap_or(-1);
            if stderr.is_empty() {
                format!("Exit code: {}\n{}", code, stdout)
            } else {
                format!("Exit code: {}\nStdout:\n{}\nStderr:\n{}", code, stdout, stderr)
            }
        }
        Err(e) => format!("Error executing command: {}", e),
    }
}
