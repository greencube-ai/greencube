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
/// Each entry is (tool_name, arguments_json).
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
    system_prompt: String,
    user_message: String,
    responses: Vec<serde_json::Value>,
}

// ─── Bench Result ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct BenchResult {
    task_name: String,
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

        // Brief pause for servers to be ready
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        Self { state, api_url, agent_id, mock_llm }
    }

    async fn reset(&self) {
        // Clear mock queue
        self.mock_llm.response_queue.lock().await.clear();

        // Wipe task-related state, keep agent and provider
        let db = self.state.db.lock().await;
        let _ = db.execute_batch(
            "DELETE FROM episodes;
             DELETE FROM audit_log;
             DELETE FROM competence_map;
             DELETE FROM tool_results;"
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

// ─── Test ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn bench_harness() {
    let runner = BenchRunner::new().await;

    let spec = TaskSpec {
        name: "mixed_tool_2calls_1error".into(),
        system_prompt: "You are a test agent.".into(),
        user_message: "Run the benchmark task.".into(),
        responses: vec![
            // Round 1: 2 tool calls — shell succeeds, unknown tool errors
            mock_tool_calls(&[
                ("shell", serde_json::json!({"command": "echo hello"})),
                ("nonexistent_tool", serde_json::json!({"x": 1})),
            ]),
            // Round 2: final text
            mock_text_response("Done."),
        ],
    };

    let result = runner.run_task(&spec).await;

    // Print for --nocapture
    println!("=== Bench Result ===");
    println!("{}", serde_json::to_string_pretty(&result).unwrap());

    // Assert pipeline correctness — doubles as Judge rulebook regression test
    assert_eq!(result.tool_call_count, 2, "expected 2 tool calls");
    assert_eq!(result.tool_error_count, 1, "expected 1 tool error");
    assert_eq!(result.llm_rounds, 2, "expected 2 LLM rounds");
    assert!(
        (result.delta - (-0.10)).abs() < 0.001,
        "expected delta -0.10, got {}",
        result.delta
    );
    assert!(result.duration_ms > 0, "duration should be > 0");
    assert!(
        result.reason.contains("1/2"),
        "reason should mention '1/2', got: {}",
        result.reason
    );

    // Save to file
    runner.reset().await;
    BenchRunner::save_results(&[result], "bench_results.json");
    assert!(std::path::Path::new("bench_results.json").exists());

    // Clean up
    let _ = std::fs::remove_file("bench_results.json");
}
