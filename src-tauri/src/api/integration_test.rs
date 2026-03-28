use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use std::sync::Arc;
use tokio::sync::Mutex;

// ─── Mock LLM Server ───────────────────────────────────────────────────────

struct MockLlmState {
    requests: Mutex<Vec<serde_json::Value>>,
    call_count: Mutex<usize>,
}

async fn mock_chat_completions(
    State(state): State<Arc<MockLlmState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let mut requests = state.requests.lock().await;
    requests.push(body);
    let mut count = state.call_count.lock().await;
    *count += 1;

    if *count == 1 {
        // First call: return a tool_call for "shell"
        Json(serde_json::json!({
            "id": "chatcmpl-mock-1",
            "object": "chat.completion",
            "created": 1711360000,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_stripe_123",
                        "type": "function",
                        "function": {
                            "name": "shell",
                            "arguments": "{\"command\": \"echo Stripe integration complete\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 50, "completion_tokens": 20, "total_tokens": 70 }
        }))
    } else {
        // Subsequent calls: return plain text
        Json(serde_json::json!({
            "id": format!("chatcmpl-mock-{}", *count),
            "object": "chat.completion",
            "created": 1711360000,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "I completed the Stripe API integration task successfully."
                },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 80, "completion_tokens": 30, "total_tokens": 110 }
        }))
    }
}

/// Starts a mock LLM server on an OS-assigned port. Returns (state, url).
async fn start_mock_llm() -> (Arc<MockLlmState>, String) {
    let state = Arc::new(MockLlmState {
        requests: Mutex::new(vec![]),
        call_count: Mutex::new(0),
    });
    let router = Router::new()
        .route("/chat/completions", post(mock_chat_completions))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock LLM");
    let addr = listener.local_addr().expect("mock LLM addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (state, format!("http://{}", addr))
}

// ─── Test Helper ────────────────────────────────────────────────────────────

fn create_test_state(mock_llm_url: &str) -> Arc<crate::state::AppState> {
    let conn = crate::db::init_memory_database().expect("init in-memory db");

    // Create a provider pointing at the mock LLM
    crate::providers::create_provider(
        &conn, "MockProvider", mock_llm_url, "test-key-12345", "gpt-4o", "openai"
    ).expect("create test provider");

    let mut config = crate::config::AppConfig::default();
    config.llm.memory_mode = "keyword".into(); // Enable keyword recall for integration tests
    config.llm.self_reflection_enabled = false; // Disable reflection in tests (no real LLM)

    Arc::new(crate::state::AppState {
        db: tokio::sync::Mutex::new(conn),
        config: tokio::sync::RwLock::new(config),
        app_handle: None,
        actual_port: 0,
    })
}

/// Starts the GreenCube API on an OS-assigned port. Returns the base URL.
async fn start_greencube_api(state: Arc<crate::state::AppState>) -> String {
    let router = crate::api::create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind GreenCube API");
    let addr = listener.local_addr().expect("API addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    format!("http://{}", addr)
}

// ─── Integration Tests ─────────────────────────────────────────────────────

/// THE KILLER TEST: Proves memory persists and gets injected across tasks.
///
/// Flow:
/// 1. Create agent
/// 2. Send completion request mentioning "Stripe API" → mock returns tool_call → Docker fails → mock returns final text
/// 3. Verify audit log has tool_call entry
/// 4. Verify episodes were created
/// 5. Send SECOND request mentioning "Stripe"
/// 6. Verify the forwarded request to mock LLM contains injected memories from the first task
#[tokio::test]
async fn test_end_to_end_completions_with_memory() {
    // 1. Start mock LLM
    let (mock_state, mock_url) = start_mock_llm().await;

    // 2. Create GreenCube API with test state pointing at mock
    let app_state = create_test_state(&mock_url);
    let base_url = start_greencube_api(app_state.clone()).await;
    let client = reqwest::Client::new();

    // 3. Create agent
    let create_resp = client
        .post(format!("{}/v1/agents", base_url))
        .json(&serde_json::json!({
            "name": "StripeBot",
            "system_prompt": "You help with Stripe API integration.",
            "tools_allowed": ["shell", "read_file"]
        }))
        .send()
        .await
        .expect("create agent request");
    assert_eq!(create_resp.status(), 201, "Agent creation should return 201");
    let agent: serde_json::Value = create_resp.json().await.expect("parse agent response");
    let agent_id = agent["id"].as_str().expect("agent has id");

    // 4. Send FIRST chat completion — mentions "Stripe API"
    let chat_resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("x-agent-id", agent_id)
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You help with Stripe API integration."},
                {"role": "user", "content": "Help me integrate the Stripe API for payment processing"}
            ]
        }))
        .send()
        .await
        .expect("first completion request");
    assert!(
        chat_resp.status().is_success(),
        "First completion should succeed, got {}",
        chat_resp.status()
    );
    let _first_response: serde_json::Value = chat_resp.json().await.expect("parse first response");

    // 5. Verify audit log has tool_call entry
    let audit_resp = client
        .get(format!("{}/v1/agents/{}/audit?limit=50", base_url, agent_id))
        .send()
        .await
        .expect("audit request");
    let audit: serde_json::Value = audit_resp.json().await.expect("parse audit");
    let entries = audit["entries"].as_array().expect("entries is array");
    assert!(
        entries.iter().any(|e| e["action_type"] == "tool_call"),
        "Audit log should contain a tool_call entry. Got: {:?}",
        entries.iter().map(|e| e["action_type"].as_str()).collect::<Vec<_>>()
    );

    // 6. Verify episodes were created
    let episodes_resp = client
        .get(format!(
            "{}/v1/agents/{}/episodes?limit=50",
            base_url, agent_id
        ))
        .send()
        .await
        .expect("episodes request");
    let episodes: serde_json::Value = episodes_resp.json().await.expect("parse episodes");
    let episode_list = episodes["episodes"].as_array().expect("episodes array");
    assert!(
        !episode_list.is_empty(),
        "Should have at least one episode"
    );
    // Should have task_start, at least one llm_response, and task_end
    let event_types: Vec<&str> = episode_list
        .iter()
        .filter_map(|e| e["event_type"].as_str())
        .collect();
    assert!(
        event_types.contains(&"task_start"),
        "Should have task_start episode"
    );
    assert!(
        event_types.contains(&"task_end"),
        "Should have task_end episode"
    );

    // 7. Reset mock call counter for second request
    // The mock needs to return plain text on the NEXT call (not a tool_call)
    {
        let mut count = mock_state.call_count.lock().await;
        *count = 1; // Next call will be count=2, which returns plain text
    }

    // 8. Send SECOND chat completion — also mentions "Stripe"
    let chat_resp2 = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("x-agent-id", agent_id)
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You help with Stripe integration."},
                {"role": "user", "content": "Tell me about Stripe payment processing"}
            ]
        }))
        .send()
        .await
        .expect("second completion request");
    assert!(
        chat_resp2.status().is_success(),
        "Second completion should succeed"
    );

    // 9. THE KILLER ASSERTION: Verify memory was injected into the forwarded request
    let requests = mock_state.requests.lock().await;
    // The last request to the mock should have memories injected into the system prompt
    let last_request = requests.last().expect("mock should have received requests");
    let messages = last_request["messages"]
        .as_array()
        .expect("messages array in forwarded request");

    // Find the system message — it should contain injected memories
    let system_msg = messages
        .iter()
        .find(|m| m["role"] == "system")
        .expect("forwarded request should have a system message");
    let system_content = system_msg["content"]
        .as_str()
        .expect("system message has content");

    assert!(
        system_content.contains("Relevant memories from past tasks"),
        "System prompt should contain injected memories header. Got:\n{}",
        system_content
    );
    assert!(
        system_content.contains("Stripe") || system_content.contains("stripe"),
        "Injected memories should reference Stripe (from first task's episodes). Got:\n{}",
        system_content
    );
}

/// Tests that creating an agent via API and listing it works end-to-end.
#[tokio::test]
async fn test_agent_crud_via_api() {
    let (_mock_state, mock_url) = start_mock_llm().await;
    let app_state = create_test_state(&mock_url);
    let base_url = start_greencube_api(app_state).await;
    let client = reqwest::Client::new();

    // Create
    let resp = client
        .post(format!("{}/v1/agents", base_url))
        .json(&serde_json::json!({
            "name": "TestAgent",
            "tools_allowed": ["shell"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // List
    let resp = client
        .get(format!("{}/v1/agents", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let agents = body["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["name"], "TestAgent");

    // Get by ID
    let id = agents[0]["id"].as_str().unwrap();
    let resp = client
        .get(format!("{}/v1/agents/{}", base_url, id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 404 for non-existent
    let resp = client
        .get(format!("{}/v1/agents/nonexistent-id", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

/// Tests health endpoint.
#[tokio::test]
async fn test_health_endpoint() {
    let (_mock_state, mock_url) = start_mock_llm().await;
    let app_state = create_test_state(&mock_url);
    let base_url = start_greencube_api(app_state).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/health", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], "0.9.0");
    // Docker removed — health only returns status + version
}
