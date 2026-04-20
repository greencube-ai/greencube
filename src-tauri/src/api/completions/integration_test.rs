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

/// Plain-text mock: always returns a single finish_reason=stop response, no tool_calls.
/// Used by the injection-shape test so the tool loop doesn't fire after the first request.
async fn mock_chat_completions_plain(
    State(state): State<Arc<MockLlmState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    state.requests.lock().await.push(body);
    *state.call_count.lock().await += 1;
    Json(serde_json::json!({
        "id": "chatcmpl-plain-1",
        "object": "chat.completion",
        "created": 1711360000,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "ok"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 1, "total_tokens": 11}
    }))
}

async fn start_mock_llm_plain() -> (Arc<MockLlmState>, String) {
    let state = Arc::new(MockLlmState {
        requests: Mutex::new(vec![]),
        call_count: Mutex::new(0),
    });
    let router = Router::new()
        .route("/chat/completions", post(mock_chat_completions_plain))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind plain mock LLM");
    let addr = listener.local_addr().expect("plain mock LLM addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (state, format!("http://{}", addr))
}

/// Streaming mock: returns a canned SSE body (3 delta frames + [DONE]).
async fn mock_chat_completions_stream(
    State(state): State<Arc<MockLlmState>>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    state.requests.lock().await.push(body);
    *state.call_count.lock().await += 1;
    let body_str = concat!(
        "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"}}]}\n\n",
        "data: {\"id\":\"c2\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"}}]}\n\n",
        "data: {\"id\":\"c3\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"!\"}}]}\n\n",
        "data: [DONE]\n\n"
    );
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .body(axum::body::Body::from(body_str))
        .unwrap()
}

/// Starts a streaming mock LLM server. Returns (state, url).
async fn start_mock_llm_stream() -> (Arc<MockLlmState>, String) {
    let state = Arc::new(MockLlmState {
        requests: Mutex::new(vec![]),
        call_count: Mutex::new(0),
    });
    let router = Router::new()
        .route("/chat/completions", post(mock_chat_completions_stream))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind streaming mock LLM");
    let addr = listener.local_addr().expect("streaming mock LLM addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (state, format!("http://{}", addr))
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
/// 2. Send completion request mentioning "Stripe API" → mock returns tool_call → tool execution fails → mock returns final text
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

/// Smoke test for the SSE streaming path: proves that streaming forwards frames,
/// terminates with [DONE], and still fires post-task side effects (task_end
/// episode + audit). Guards Sub-prompt 5b (extraction of stream_llm_response).
#[tokio::test]
async fn test_streaming_end_to_end() {
    // 1. Streaming mock LLM
    let (_mock_state, mock_url) = start_mock_llm_stream().await;
    let app_state = create_test_state(&mock_url);
    let base_url = start_greencube_api(app_state).await;
    let client = reqwest::Client::new();

    // 2. Create agent with empty tools_allowed — streaming is gated on !has_tools,
    //    and tool defs are auto-injected when tools_allowed is non-empty.
    let create_resp = client
        .post(format!("{}/v1/agents", base_url))
        .json(&serde_json::json!({
            "name": "StreamBot",
            "system_prompt": "You reply briefly.",
            "tools_allowed": []
        }))
        .send()
        .await
        .expect("create agent");
    assert_eq!(create_resp.status(), 201, "agent creation");
    let agent: serde_json::Value = create_resp.json().await.expect("parse agent");
    let agent_id = agent["id"].as_str().expect("agent id");

    // 3. POST /v1/chat/completions with stream: true
    let chat_resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("x-agent-id", agent_id)
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "stream": true,
            "messages": [
                {"role": "system", "content": "You reply briefly."},
                {"role": "user", "content": "say hi"}
            ]
        }))
        .send()
        .await
        .expect("streaming completion request");

    // 4. HTTP status + content-type
    assert_eq!(chat_resp.status(), 200, "streaming status should be 200");
    let ct = chat_resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.contains("text/event-stream"),
        "expected SSE content-type, got: {}",
        ct
    );

    // 5. task_id header must be stamped on the response
    let task_id_header = chat_resp
        .headers()
        .get("x-greencube-task-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    assert!(
        task_id_header.is_some() && !task_id_header.as_deref().unwrap_or("").is_empty(),
        "expected x-greencube-task-id header on streaming response"
    );

    // 6. Read streamed body — reqwest buffers until the server closes the stream.
    //    The streaming handler runs run_post_task inline BEFORE returning, so by
    //    the time .text() resolves, task_end side effects are durable.
    let body_text = chat_resp.text().await.expect("read streamed body");
    let data_line_count = body_text.matches("data: ").count();
    assert!(
        data_line_count >= 3,
        "expected at least 3 `data: ` lines, got {} in body:\n{}",
        data_line_count,
        body_text
    );
    assert!(
        body_text.contains("data: [DONE]"),
        "expected `data: [DONE]` terminator, body was:\n{}",
        body_text
    );

    // 7. Side effect: task_end episode written via run_post_task
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
    let event_types: Vec<&str> = episode_list
        .iter()
        .filter_map(|e| e["event_type"].as_str())
        .collect();
    assert!(
        event_types.contains(&"task_end"),
        "streaming path should write task_end episode; got event_types: {:?}",
        event_types
    );

    // 8. Side effect: audit entry for task_end
    let audit_resp = client
        .get(format!(
            "{}/v1/agents/{}/audit?limit=50",
            base_url, agent_id
        ))
        .send()
        .await
        .expect("audit request");
    let audit: serde_json::Value = audit_resp.json().await.expect("parse audit");
    let entries = audit["entries"].as_array().expect("audit entries");
    let action_types: Vec<&str> = entries
        .iter()
        .filter_map(|e| e["action_type"].as_str())
        .collect();
    assert!(
        action_types.contains(&"task_end"),
        "streaming path should write task_end audit entry; got action_types: {:?}",
        action_types
    );
}

/// Snapshot test: captures body.messages forwarded to the LLM after all injections run,
/// and asserts the full ordered sequence of system-prompt injections. Guards Sub-prompt 6b
/// (extraction of injection.rs) — any re-ordering or skipped injection fails this test.
#[tokio::test]
async fn test_injection_chain_shape() {
    // 1. Plain mock LLM — always returns finish_reason=stop so tool loop never runs.
    let (mock_state, mock_url) = start_mock_llm_plain().await;
    let app_state = create_test_state(&mock_url);

    // 2. Populate DB fixture directly (before agent-creates a specialist child, etc.)
    let (agent_id, user_id) = {
        let db = app_state.db.lock().await;

        // Primary agent — tools_allowed=["shell"] so tool-defs + tool-hint injections fire.
        let primary = crate::identity::registry::create_agent(
            &db,
            "InjectionBot",
            "base system prompt",
            &["shell".to_string()],
        )
        .expect("create primary agent");

        // Dynamic profile — triggers "--- Your profile ---" injection.
        crate::identity::registry::update_agent_dynamic_profile(
            &db,
            &primary.id,
            "expert in frontend layouts",
        )
        .expect("set profile");

        // Competence: domain=css, 5 failures → task_count=5, confidence=0.0 → warning fires
        // (query contains "css", no child specialist exists).
        for _ in 0..5 {
            crate::competence::update_competence(&db, &primary.id, "css", false, None)
                .expect("update competence");
        }

        // Knowledge: preference + correction + keyword fact (so recall_relevant returns ≥1).
        crate::knowledge::insert_knowledge(
            &db,
            &primary.id,
            "prefers terse css code",
            "preference",
            None,
        )
        .expect("insert pref");
        crate::knowledge::insert_knowledge(
            &db,
            &primary.id,
            "avoid stripe payment inline styles",
            "correction",
            None,
        )
        .expect("insert correction");
        crate::knowledge::insert_knowledge(
            &db,
            &primary.id,
            "stripe api needs bearer auth",
            "fact",
            None,
        )
        .expect("insert fact");

        // Goal: triggers "--- Your current goals ---" injection.
        crate::goals::insert_goal(&db, &primary.id, "finish css refactor")
            .expect("insert goal");

        // Working context: triggers "--- Your working context" injection.
        crate::context::set_context(&db, &primary.id, "scratchpad notes on layouts")
            .expect("set context");

        // Relationship: 3+ interactions → get_relationship_prompt returns Some(...).
        let user_id = "default_user";
        for _ in 0..3 {
            crate::relationships::record_interaction(&db, &primary.id, user_id)
                .expect("record interaction");
        }

        // Habitat: a separate agent with stripe-related knowledge → habitat injection fires.
        let neighbor = crate::identity::registry::create_agent(
            &db,
            "NeighborBot",
            "neighbor",
            &["shell".to_string()],
        )
        .expect("create neighbor");
        crate::knowledge::insert_knowledge(
            &db,
            &neighbor.id,
            "stripe webhooks need signature verification",
            "fact",
            None,
        )
        .expect("insert habitat fact");

        (primary.id, user_id.to_string())
    };

    // 3. Bring API up AFTER fixtures are committed.
    let base_url = start_greencube_api(app_state.clone()).await;
    let client = reqwest::Client::new();

    // 4. POST a user query that triggers both the competence warning (contains "css")
    //    and knowledge recall (contains >3-char words like "stripe" matching knowledge content).
    let query = "Help me style my css for stripe payments";
    let chat_resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("x-agent-id", &agent_id)
        .header("x-user-id", &user_id)
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "base system prompt"},
                {"role": "user", "content": query}
            ]
        }))
        .send()
        .await
        .expect("chat request");
    assert!(
        chat_resp.status().is_success(),
        "completion should succeed, got {}",
        chat_resp.status()
    );

    // 5. Grab the first request body forwarded to the mock — that is the post-injection
    //    snapshot. Subsequent calls (if any) would have tool_result appended but same system.
    let requests = mock_state.requests.lock().await;
    assert!(
        !requests.is_empty(),
        "mock should have received at least one request"
    );
    let forwarded = &requests[0];
    let messages = forwarded["messages"]
        .as_array()
        .expect("forwarded body has messages array");

    // 6. Structural: at least one user msg with the original query, and exactly one system msg
    //    (all injections concatenate into the single pre-existing system message).
    let user_msg_count = messages.iter().filter(|m| m["role"] == "user").count();
    assert!(user_msg_count >= 1, "expected at least one user message");
    let user_present = messages
        .iter()
        .any(|m| m["role"] == "user" && m["content"].as_str() == Some(query));
    assert!(user_present, "original user query missing from forwarded body");

    let system_msgs: Vec<&str> = messages
        .iter()
        .filter(|m| m["role"] == "system")
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert_eq!(
        system_msgs.len(),
        1,
        "expected exactly one system message (all injections concatenate), got {}",
        system_msgs.len()
    );
    let sys = system_msgs[0];

    // 7. ORDER assertion: every injection marker must appear, in this exact order.
    //    Markers are chosen as structural substrings that identify each injection block
    //    without being brittle to exact wording. The list mirrors the execution order
    //    in mod.rs chat_completions:
    //      1. competence warning (2c)
    //      2. relationship (post task_start)
    //      3. learned preferences (3b)
    //      4. mistakes to avoid (3b)
    //      5. profile (3c)
    //      6. current goals (3c)
    //      7. working context (3c)
    //      8. knowledge — keyword recall (4)
    //      9. habitat knowledge (4b)
    //     10. tool-usage hint (5)
    let expected_markers: Vec<(&str, &str)> = vec![
        ("competence warning", "WARNING:"),
        ("relationship", "interacted with this user"),
        ("preferences", "--- Apply these learned preferences ---"),
        ("corrections", "--- Mistakes to avoid"),
        ("profile", "--- Your profile ---"),
        ("goals", "--- Your current goals ---"),
        ("working context", "--- Your working context"),
        ("knowledge", "--- Things you know ---"),
        ("habitat", "--- Knowledge from other agents in your habitat ---"),
        ("tool hint", "You have access to these tools:"),
    ];

    let mut positions: Vec<(&str, usize)> = Vec::new();
    for (label, marker) in &expected_markers {
        match sys.find(marker) {
            Some(pos) => positions.push((label, pos)),
            None => panic!(
                "missing injection marker [{}] => `{}` in system content:\n---\n{}\n---",
                label, marker, sys
            ),
        }
    }

    for w in positions.windows(2) {
        let (a_label, a_pos) = w[0];
        let (b_label, b_pos) = w[1];
        assert!(
            a_pos < b_pos,
            "injection order drift: [{}] at {} should precede [{}] at {}.\n\
             Full system content was:\n---\n{}\n---",
            a_label, a_pos, b_label, b_pos, sys
        );
    }
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
    assert_eq!(body["version"], "1.0.0");
    // Health endpoint only returns status + version
}
