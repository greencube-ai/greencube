//! Benchmark harness — validates the greencube pipeline end-to-end with mock LLM.
//!
//! Run with: cargo test bench_harness -- --nocapture

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api;
use crate::db;
use crate::identity::registry;
use crate::providers;
use crate::state::AppState;

// ─── Mock LLM ──────────────────────────────────────────────────────────────

struct MockLlm {
    response_queue: Mutex<VecDeque<serde_json::Value>>,
}

async fn mock_handler(
    State(mock): State<Arc<MockLlm>>,
    Json(_body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let mut queue = mock.response_queue.lock().await;
    let response = queue.pop_front().unwrap_or_else(|| mock_text_response("(default response)"));
    Json(response)
}

/// Build an OpenAI response containing tool calls.
fn mock_tool_calls(calls: &[(&str, serde_json::Value)]) -> serde_json::Value {
    let tool_calls: Vec<serde_json::Value> = calls
        .iter()
        .enumerate()
        .map(|(i, (name, args))| {
            serde_json::json!({
                "id": format!("call_{}", i),
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": serde_json::to_string(args).unwrap()
                }
            })
        })
        .collect();

    serde_json::json!({
        "id": "chatcmpl-bench",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": tool_calls
            },
            "finish_reason": "tool_calls"
        }],
        "usage": { "prompt_tokens": 50, "completion_tokens": 20, "total_tokens": 70 }
    })
}

/// Build an OpenAI response containing a single tool call (convenience).
fn mock_single_tool(name: &str, args: serde_json::Value) -> serde_json::Value {
    mock_tool_calls(&[(name, args)])
}

/// Build an OpenAI response containing a final text message.
fn mock_text_response(text: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-bench-final",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": text
            },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 80, "completion_tokens": 30, "total_tokens": 110 }
    })
}

// ─── Task Spec ─────────────────────────────────────────────────────────────

struct TaskSpec {
    name: String,
    category: String,
    is_trigger: bool,
    sibling_of: Option<String>,
    expected_mistake: String,
    expected_verdict_delta: f64,
    expected_tool_call_count: u32,
    expected_llm_rounds: u32,
    system_prompt: String,
    user_message: String,
    responses: Vec<serde_json::Value>,
}

// ─── Bench Result ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct BenchResult {
    task_name: String,
    category: String,
    is_trigger: bool,
    sibling_of: Option<String>,
    expected_mistake: String,
    expected_verdict_delta: f64,
    verdict_raw: serde_json::Value,
    delta: f64,
    reason: String,
    tool_call_count: u32,
    tool_error_count: u32,
    llm_rounds: u32,
    duration_ms: u64,
    timestamp: String,
}

// ─── Bench Runner ──────────────────────────────────────────────────────────

struct BenchRunner {
    state: Arc<AppState>,
    api_url: String,
    agent_id: String,
    mock_llm: Arc<MockLlm>,
}

impl BenchRunner {
    async fn new() -> Self {
        // 1. Start mock LLM
        let mock_llm = Arc::new(MockLlm {
            response_queue: Mutex::new(VecDeque::new()),
        });
        let mock_router = Router::new()
            .route("/chat/completions", post(mock_handler))
            .with_state(mock_llm.clone());
        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock LLM");
        let mock_addr = mock_listener.local_addr().expect("mock LLM addr");
        tokio::spawn(async move {
            axum::serve(mock_listener, mock_router).await.unwrap();
        });
        let mock_url = format!("http://{}", mock_addr);

        // 2. Create AppState with in-memory DB
        let conn = db::init_memory_database().expect("init in-memory db");
        providers::create_provider(&conn, "BenchProvider", &mock_url, "bench-key", "gpt-4o", "openai")
            .expect("create bench provider");
        let agent = registry::create_agent(&conn, "BenchAgent", "You are a test agent.", &["shell".into(), "read_file".into(), "write_file".into()])
            .expect("create bench agent");
        let agent_id = agent.id.clone();

        let mut config = crate::config::AppConfig::default();
        config.llm.memory_mode = "keyword".into();
        config.llm.self_reflection_enabled = false;

        let state = Arc::new(AppState {
            db: tokio::sync::Mutex::new(conn),
            config: tokio::sync::RwLock::new(config),
            app_handle: None,
            actual_port: 0,
        });

        // 3. Start greencube API
        let api_router = api::create_router(state.clone());
        let api_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind API");
        let api_addr = api_listener.local_addr().expect("API addr");
        tokio::spawn(async move {
            axum::serve(api_listener, api_router).await.unwrap();
        });
        let api_url = format!("http://{}", api_addr);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        Self { state, api_url, agent_id, mock_llm }
    }

    async fn reset(&self) {
        self.mock_llm.response_queue.lock().await.clear();

        let db = self.state.db.lock().await;
        let _ = db.execute_batch(
            "DELETE FROM episodes;
             DELETE FROM audit_log;
             DELETE FROM competence_map;
             DELETE FROM tool_results;
             DELETE FROM knowledge;"
        );
        let _ = registry::update_agent_status(&db, &self.agent_id, "idle");
    }

    async fn run_task(&self, spec: &TaskSpec) -> BenchResult {
        // 1. Load responses into mock
        {
            let mut queue = self.mock_llm.response_queue.lock().await;
            queue.clear();
            for r in &spec.responses {
                queue.push_back(r.clone());
            }
        }

        // 2. Send completion request
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/v1/chat/completions", self.api_url))
            .header("x-agent-id", &self.agent_id)
            .json(&serde_json::json!({
                "model": "gpt-4o",
                "messages": [
                    {"role": "system", "content": spec.system_prompt},
                    {"role": "user", "content": spec.user_message}
                ]
            }))
            .send()
            .await
            .expect("completion request failed");

        assert!(
            resp.status().is_success(),
            "Task '{}' failed with HTTP {}",
            spec.name,
            resp.status()
        );

        // 3. Query DB for judge_verdict episode
        let db = self.state.db.lock().await;
        let row: (String, String, String) = db
            .query_row(
                "SELECT raw_data, summary, created_at FROM episodes
                 WHERE agent_id = ?1 AND event_type = 'judge_verdict'
                 ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![self.agent_id],
                |row| Ok((
                    row.get::<_, String>(0).unwrap_or_default(),
                    row.get::<_, String>(1).unwrap_or_default(),
                    row.get::<_, String>(2).unwrap_or_default(),
                )),
            )
            .expect("No judge_verdict episode found — pipeline didn't run?");

        let verdict_raw: serde_json::Value =
            serde_json::from_str(&row.0).unwrap_or(serde_json::Value::Null);

        BenchResult {
            task_name: spec.name.clone(),
            category: spec.category.clone(),
            is_trigger: spec.is_trigger,
            sibling_of: spec.sibling_of.clone(),
            expected_mistake: spec.expected_mistake.clone(),
            expected_verdict_delta: spec.expected_verdict_delta,
            delta: verdict_raw["delta"].as_f64().unwrap_or(0.0),
            reason: verdict_raw["reason"].as_str().unwrap_or("").into(),
            tool_call_count: verdict_raw["outcome"]["tool_call_count"].as_u64().unwrap_or(0) as u32,
            tool_error_count: verdict_raw["outcome"]["tool_error_count"].as_u64().unwrap_or(0) as u32,
            llm_rounds: verdict_raw["outcome"]["llm_rounds"].as_u64().unwrap_or(0) as u32,
            duration_ms: verdict_raw["outcome"]["duration_ms"].as_u64().unwrap_or(0),
            timestamp: row.2,
            verdict_raw,
        }
    }

    fn save_results(results: &[BenchResult], path: &str) {
        let json = serde_json::to_string_pretty(results).expect("serialize results");
        std::fs::write(path, json).expect("write results file");
    }
}

// ─── Benchmark Tasks ───────────────────────────────────────────────────────

fn benchmark_tasks() -> Vec<TaskSpec> {
    vec![
        // ── Category 1: Edge cases ─────────────────────────────────────
        TaskSpec {
            name: "edge_empty_list".into(),
            category: "edge_cases".into(),
            is_trigger: true,
            sibling_of: None,
            expected_mistake: "Agent calls unknown tool on edge-case input".into(),
            expected_verdict_delta: -0.10,
            expected_tool_call_count: 2,
            expected_llm_rounds: 2,
            system_prompt: "You are a data processing agent.".into(),
            user_message: "Sum the values in this empty list: []".into(),
            responses: vec![
                // R1: shell succeeds, unknown tool "check_result" errors
                mock_tool_calls(&[
                    ("shell", serde_json::json!({"command": "echo 0"})),
                    ("check_result", serde_json::json!({"value": 0})),
                ]),
                // R2: final text
                mock_text_response("The sum of an empty list is 0."),
            ],
        },
        TaskSpec {
            name: "edge_null_input".into(),
            category: "edge_cases".into(),
            is_trigger: false,
            sibling_of: Some("edge_empty_list".into()),
            expected_mistake: "Agent calls unknown tool on null input".into(),
            expected_verdict_delta: -0.10,
            expected_tool_call_count: 2,
            expected_llm_rounds: 2,
            system_prompt: "You are a data processing agent.".into(),
            user_message: "Compute the average of null input".into(),
            responses: vec![
                mock_tool_calls(&[
                    ("shell", serde_json::json!({"command": "echo 0"})),
                    ("validate_type", serde_json::json!({"input": null})),
                ]),
                mock_text_response("Cannot compute average of null input."),
            ],
        },

        // ── Category 2: File system assumptions ────────────────────────
        TaskSpec {
            name: "fs_read_missing".into(),
            category: "filesystem".into(),
            is_trigger: true,
            sibling_of: None,
            expected_mistake: "Agent calls read_file without required path arg".into(),
            expected_verdict_delta: -0.25,
            expected_tool_call_count: 1,
            expected_llm_rounds: 2,
            system_prompt: "You are a file management agent.".into(),
            user_message: "Read the config from the default location".into(),
            responses: vec![
                // R1: read_file with no path arg → "Error: read_file requires 'path' argument"
                mock_single_tool("read_file", serde_json::json!({})),
                // R2: final text
                mock_text_response("I was unable to read the config file."),
            ],
        },
        TaskSpec {
            name: "fs_write_no_dir".into(),
            category: "filesystem".into(),
            is_trigger: false,
            sibling_of: Some("fs_read_missing".into()),
            expected_mistake: "Agent calls write_file without required content arg".into(),
            expected_verdict_delta: -0.25,
            expected_tool_call_count: 1,
            expected_llm_rounds: 2,
            system_prompt: "You are a file management agent.".into(),
            user_message: "Write the config to /tmp/app/config.toml".into(),
            responses: vec![
                // R1: write_file with path but no content → "Error: write_file requires 'content' argument"
                mock_single_tool("write_file", serde_json::json!({"path": "/tmp/app/config.toml"})),
                // R2: final text
                mock_text_response("I was unable to write the config file."),
            ],
        },

        // ── Category 3: Wrong API usage ────────────────────────────────
        TaskSpec {
            name: "api_wrong_args".into(),
            category: "api_usage".into(),
            is_trigger: true,
            sibling_of: None,
            expected_mistake: "Agent makes multiple calls with missing required args".into(),
            expected_verdict_delta: -0.25,
            expected_tool_call_count: 3,
            expected_llm_rounds: 2,
            system_prompt: "You are an API integration agent.".into(),
            user_message: "Fetch the user data and parse the config".into(),
            responses: vec![
                // R1: shell ok, shell missing cmd, read_file missing path → 2 errors out of 3
                mock_tool_calls(&[
                    ("shell", serde_json::json!({"command": "echo ok"})),
                    ("shell", serde_json::json!({})),
                    ("read_file", serde_json::json!({})),
                ]),
                mock_text_response("Encountered errors fetching data."),
            ],
        },
        TaskSpec {
            name: "api_bad_format".into(),
            category: "api_usage".into(),
            is_trigger: false,
            sibling_of: Some("api_wrong_args".into()),
            expected_mistake: "Agent makes multiple calls with missing required args".into(),
            expected_verdict_delta: -0.25,
            expected_tool_call_count: 3,
            expected_llm_rounds: 2,
            system_prompt: "You are an API integration agent.".into(),
            user_message: "Read the YAML config and write the output".into(),
            responses: vec![
                // R1: shell ok, write_file no path, read_file no path → 2 errors out of 3
                mock_tool_calls(&[
                    ("shell", serde_json::json!({"command": "echo ok"})),
                    ("write_file", serde_json::json!({})),
                    ("read_file", serde_json::json!({})),
                ]),
                mock_text_response("Encountered errors processing config."),
            ],
        },

        // ── Category 4: Flailing / excessive retries ───────────────────
        TaskSpec {
            name: "retry_cascade".into(),
            category: "flailing".into(),
            is_trigger: true,
            sibling_of: None,
            expected_mistake: "Agent flails with unknown tools across many rounds".into(),
            expected_verdict_delta: -0.30,
            expected_tool_call_count: 5,
            expected_llm_rounds: 6,
            system_prompt: "You are a testing agent.".into(),
            user_message: "Verify the deployment is healthy".into(),
            responses: vec![
                // R1: unknown tool → error
                mock_single_tool("check_result", serde_json::json!({})),
                // R2: shell ok
                mock_single_tool("shell", serde_json::json!({"command": "echo ok"})),
                // R3: unknown tool → error
                mock_single_tool("validate_output", serde_json::json!({})),
                // R4: shell ok
                mock_single_tool("shell", serde_json::json!({"command": "echo fixed"})),
                // R5: unknown tool → error
                mock_single_tool("verify_result", serde_json::json!({})),
                // R6: final text
                mock_text_response("Deployment verified after retries."),
            ],
        },
        TaskSpec {
            name: "retry_repeated_fail".into(),
            category: "flailing".into(),
            is_trigger: false,
            sibling_of: Some("retry_cascade".into()),
            expected_mistake: "Agent flails with unknown tools across many rounds".into(),
            expected_verdict_delta: -0.30,
            expected_tool_call_count: 5,
            expected_llm_rounds: 6,
            system_prompt: "You are a testing agent.".into(),
            user_message: "Run the integration test suite".into(),
            responses: vec![
                // R1: unknown tool → error
                mock_single_tool("run_tests", serde_json::json!({})),
                // R2: shell ok
                mock_single_tool("shell", serde_json::json!({"command": "echo pass"})),
                // R3: unknown tool → error
                mock_single_tool("assert_output", serde_json::json!({})),
                // R4: shell ok
                mock_single_tool("shell", serde_json::json!({"command": "echo retry"})),
                // R5: unknown tool → error
                mock_single_tool("check_coverage", serde_json::json!({})),
                // R6: final text
                mock_text_response("Tests completed with issues."),
            ],
        },

        // ── Category 5: Clean success (control) ────────────────────────
        TaskSpec {
            name: "clean_simple".into(),
            category: "clean_success".into(),
            is_trigger: true,
            sibling_of: None,
            expected_mistake: "(control — no mistake expected)".into(),
            expected_verdict_delta: 0.15,
            expected_tool_call_count: 2,
            expected_llm_rounds: 2,
            system_prompt: "You are a helpful assistant.".into(),
            user_message: "List the files and show the date".into(),
            responses: vec![
                // R1: two shell calls, both succeed
                mock_tool_calls(&[
                    ("shell", serde_json::json!({"command": "echo file1.txt file2.txt"})),
                    ("shell", serde_json::json!({"command": "echo 2026-04-12"})),
                ]),
                // R2: final text
                mock_text_response("Here are the files and today's date."),
            ],
        },
        TaskSpec {
            name: "clean_multi_step".into(),
            category: "clean_success".into(),
            is_trigger: false,
            sibling_of: Some("clean_simple".into()),
            expected_mistake: "(control — no mistake expected)".into(),
            expected_verdict_delta: 0.15,
            expected_tool_call_count: 3,
            expected_llm_rounds: 3,
            system_prompt: "You are a helpful assistant.".into(),
            user_message: "Check the project config and list source files".into(),
            responses: vec![
                // R1: one shell call
                mock_single_tool("shell", serde_json::json!({"command": "echo step1"})),
                // R2: shell + read_file both succeed
                mock_tool_calls(&[
                    ("shell", serde_json::json!({"command": "echo step2"})),
                    ("read_file", serde_json::json!({"path": "Cargo.toml"})),
                ]),
                // R3: final text
                mock_text_response("Project config and source files listed."),
            ],
        },
    ]
}

// ─── Test ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn bench_harness() {
    let runner = BenchRunner::new().await;
    let tasks = benchmark_tasks();
    let mut results = Vec::new();

    for spec in &tasks {
        runner.reset().await;
        let result = runner.run_task(spec).await;

        // Print each result for --nocapture
        println!("--- {} ({}) ---", result.task_name, result.category);
        println!("  delta: {:+.2} (expected {:+.2})", result.delta, spec.expected_verdict_delta);
        println!("  tools: {} (expected {}), errors: {}, rounds: {} (expected {})",
            result.tool_call_count, spec.expected_tool_call_count,
            result.tool_error_count,
            result.llm_rounds, spec.expected_llm_rounds);

        // Assert verdict matches expected delta
        assert!(
            (result.delta - spec.expected_verdict_delta).abs() < 0.001,
            "Task '{}': expected delta {:+.2}, got {:+.2}",
            spec.name, spec.expected_verdict_delta, result.delta
        );

        // Permanent regression catches on grounded counts
        assert_eq!(
            result.tool_call_count, spec.expected_tool_call_count,
            "Task '{}': expected tool_call_count {}, got {}",
            spec.name, spec.expected_tool_call_count, result.tool_call_count
        );
        assert_eq!(
            result.llm_rounds, spec.expected_llm_rounds,
            "Task '{}': expected llm_rounds {}, got {}",
            spec.name, spec.expected_llm_rounds, result.llm_rounds
        );

        results.push(result);
    }

    // Save all results
    BenchRunner::save_results(&results, "bench_results.json");
    println!("\n=== {} tasks completed, results saved to bench_results.json ===", results.len());

    // Clean up
    let _ = std::fs::remove_file("bench_results.json");
}
