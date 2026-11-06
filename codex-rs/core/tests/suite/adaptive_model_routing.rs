use anyhow::Result;
use codex_config::config_toml::AgentModelRoutingScope;
use codex_core::TurnInputRequest;
use codex_features::Feature;
use codex_models_manager::bundled_models_response;
use codex_protocol::items::AgentMessageModelTier;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_custom_tool_call;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_response_once_match;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::sse_response;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const REGULAR_MODEL: &str = "gpt-6.1-sol";
const SIMPLE_MODEL: &str = "gpt-6-luna";

fn input_tool_names(body: &Value) -> Vec<String> {
    body["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "additional_tools")
        .next_back()
        .into_iter()
        .flat_map(|item| item["tools"].as_array().into_iter().flatten())
        .flat_map(|tool| {
            if tool["type"] == "namespace" {
                tool["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|nested| nested["name"].as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
            } else {
                tool["name"]
                    .as_str()
                    .map(str::to_owned)
                    .into_iter()
                    .collect()
            }
        })
        .collect()
}

fn request_agent_name(body: &Value) -> Option<String> {
    body["client_metadata"]["x-codex-turn-metadata"]
        .as_str()
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|metadata| metadata["agent_name"].as_str().map(str::to_owned))
}

fn is_regular_followup_agent(body: &Value) -> bool {
    request_agent_name(body).is_some_and(|name| name.ends_with("/regular_followup"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_scope_routes_and_escalates_in_the_main_thread() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let completed_with_usage = |id: &str, input_tokens: i64, output_tokens: i64| {
        json!({
            "type": "response.completed",
            "response": {
                "id": id,
                "usage": {
                    "input_tokens": input_tokens,
                    "input_tokens_details": null,
                    "output_tokens": output_tokens,
                    "output_tokens_details": null,
                    "total_tokens": input_tokens + output_tokens
                }
            }
        })
    };
    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("route"),
                ev_function_call_with_namespace(
                    "route-call",
                    "functions",
                    "codex_route_model",
                    &json!({"tier": "simple"}).to_string(),
                ),
                completed_with_usage("route", 300, 0),
            ]),
            sse(vec![
                ev_response_created("simple"),
                ev_assistant_message("message-simple", "A concise answer from the simple model."),
                ev_function_call_with_namespace(
                    "escalate-call",
                    "functions",
                    "codex_escalate_to_regular_model",
                    "{}",
                ),
                completed_with_usage("simple", 80, 20),
            ]),
            sse(vec![
                ev_response_created("regular"),
                ev_assistant_message("message-regular", "The regular model's improved response."),
                completed_with_usage("regular", 300, 0),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_model(REGULAR_MODEL)
        .with_config(|config| {
            config.agent_model_routing_scope = Some(AgentModelRoutingScope::All);
            config.agent_simple_model = SIMPLE_MODEL.to_string();
        })
        .build_with_auto_env(&server)
        .await?;

    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Answer this request".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;

    let mut token_info = None;
    let mut assistant_labels = Vec::new();
    loop {
        let event = test.codex.next_event().await?;
        if let EventMsg::TokenCount(count) = &event.msg {
            token_info = count.info.clone();
        }
        if let EventMsg::ItemCompleted(completed) = &event.msg
            && let TurnItem::AgentMessage(message) = &completed.item
        {
            assistant_labels.push((message.model.clone(), message.model_tier));
        }
        if matches!(&event.msg, EventMsg::TurnComplete(_)) {
            break;
        }
    }

    let requests = responses.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| {
                request.body_json()["model"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect::<Vec<_>>(),
        vec![
            REGULAR_MODEL.to_string(),
            SIMPLE_MODEL.to_string(),
            REGULAR_MODEL.to_string(),
        ]
    );
    let first_request_body = requests[0].body_json();
    assert_eq!(first_request_body["tool_choice"], "required");
    assert_eq!(
        input_tool_names(&first_request_body),
        vec!["codex_route_model"]
    );
    assert!(
        first_request_body["instructions"]
            .as_str()
            .is_some_and(|instructions| instructions.contains("<model_routing_preflight>"))
            || requests[0]
                .message_input_texts("developer")
                .iter()
                .any(|text| text.contains("<model_routing_preflight>")),
        "the initial request should explicitly be a routing preflight"
    );
    let simple_request_body = requests[1].body_json();
    let simple_tools = input_tool_names(&simple_request_body);
    assert!(simple_tools.iter().any(|tool| tool == "exec"));
    assert!(!simple_tools.iter().any(|tool| tool == "codex_route_model"));
    assert!(
        requests[2]
            .message_input_texts("developer")
            .iter()
            .any(|text| text.contains("simple_model=gpt-6-luna"))
    );

    let routing_usage = token_info
        .expect("token usage info should be recorded")
        .model_routing_usage
        .expect("adaptive routing usage should be recorded");
    assert_eq!(routing_usage.simple_model_slug, SIMPLE_MODEL);
    assert_eq!(routing_usage.simple_model_token_usage.input_tokens, 80);
    assert_eq!(routing_usage.simple_model_token_usage.output_tokens, 20);
    assert_eq!(routing_usage.simple_model_token_usage.total_tokens, 100);
    assert_eq!(routing_usage.reported_total_token_usage.total_tokens, 700);
    assert_eq!(routing_usage.simple_model_responses, 1);
    assert_eq!(routing_usage.total_responses, 3);
    assert_eq!(routing_usage.unreported_responses, 0);
    assert_eq!(
        assistant_labels,
        vec![
            (
                Some(SIMPLE_MODEL.to_string()),
                Some(AgentMessageModelTier::Simple),
            ),
            (
                Some(REGULAR_MODEL.to_string()),
                Some(AgentMessageModelTier::Regular),
            ),
        ]
    );

    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_scope_discards_an_unrouted_answer_and_retries_with_regular_tools() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("route-with-answer"),
                ev_assistant_message(
                    "unrouted-answer",
                    "This answer must not be shown before model routing completes.",
                ),
                ev_completed("route-with-answer"),
            ]),
            sse(vec![
                ev_response_created("regular-tool-use"),
                ev_custom_tool_call(
                    "shell-call",
                    "exec",
                    "text('ROUTING_PREFLIGHT_FALLBACK_WORKED')",
                ),
                ev_completed("regular-tool-use"),
            ]),
            sse(vec![
                ev_response_created("regular-answer"),
                ev_assistant_message("regular-message", "The regular model completed the task."),
                ev_completed("regular-answer"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_model(REGULAR_MODEL)
        .with_config(|config| {
            config.agent_model_routing_scope = Some(AgentModelRoutingScope::All);
            config.agent_simple_model = SIMPLE_MODEL.to_string();
        })
        .build_with_auto_env(&server)
        .await?;

    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Run the local verification command".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;

    let mut assistant_messages = Vec::new();
    loop {
        let event = test.codex.next_event().await?;
        if let EventMsg::ItemCompleted(completed) = &event.msg
            && let TurnItem::AgentMessage(message) = &completed.item
        {
            assistant_messages.push(
                message
                    .content
                    .iter()
                    .map(|part| match part {
                        codex_protocol::items::AgentMessageContent::Text { text } => text.as_str(),
                    })
                    .collect::<String>(),
            );
        }
        if matches!(&event.msg, EventMsg::TurnComplete(_)) {
            break;
        }
    }

    let requests = responses.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.body_json()["model"]
                .as_str()
                .unwrap_or_default()
                .to_string())
            .collect::<Vec<_>>(),
        vec![
            REGULAR_MODEL.to_string(),
            REGULAR_MODEL.to_string(),
            REGULAR_MODEL.to_string(),
        ]
    );
    assert!(
        requests[1]
            .message_input_texts("assistant")
            .iter()
            .all(|text| !text.contains("This answer must not be shown")),
        "unrouted assistant text must not enter the retried request history"
    );
    assert!(
        input_tool_names(&requests[1].body_json())
            .iter()
            .any(|tool| tool == "exec")
    );
    assert_eq!(
        assistant_messages,
        vec!["The regular model completed the task.".to_string()]
    );

    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_scope_can_select_the_regular_model_without_switching_away() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("regular-route"),
                ev_function_call_with_namespace(
                    "regular-route-call",
                    "functions",
                    "codex_route_model",
                    &json!({"tier": "regular"}).to_string(),
                ),
                ev_completed("regular-route"),
            ]),
            sse(vec![
                ev_response_created("regular-answer"),
                ev_assistant_message("regular-message", "The regular model answer."),
                ev_completed("regular-answer"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_model(REGULAR_MODEL)
        .with_config(|config| {
            config.agent_model_routing_scope = Some(AgentModelRoutingScope::All);
            config.agent_simple_model = SIMPLE_MODEL.to_string();
        })
        .build_with_auto_env(&server)
        .await?;

    test.submit_text_turn("Use the regular model for this request")
        .await?;

    assert_eq!(
        responses
            .requests()
            .iter()
            .map(|request| request.body_json()["model"]
                .as_str()
                .unwrap_or_default()
                .to_string())
            .collect::<Vec<_>>(),
        vec![REGULAR_MODEL.to_string(), REGULAR_MODEL.to_string()]
    );
    assert_eq!(
        responses.requests()[0].body_json()["tool_choice"],
        "required"
    );

    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_scope_simple_answer_completes_without_escalation_or_provider_usage() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("simple-route"),
                ev_function_call_with_namespace(
                    "simple-route-call",
                    "functions",
                    "codex_route_model",
                    &json!({"tier": "simple"}).to_string(),
                ),
                json!({
                    "type": "response.completed",
                    "response": {"id": "simple-route"}
                }),
            ]),
            sse(vec![
                ev_response_created("simple-answer"),
                ev_assistant_message("simple-message", "Hi!"),
                json!({
                    "type": "response.completed",
                    "response": {"id": "simple-answer"}
                }),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_model(REGULAR_MODEL)
        .with_config(|config| {
            config.agent_model_routing_scope = Some(AgentModelRoutingScope::All);
            config.agent_simple_model = SIMPLE_MODEL.to_string();
        })
        .build_with_auto_env(&server)
        .await?;

    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "hi".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;

    let mut token_info = None;
    loop {
        let event = test.codex.next_event().await?;
        if let EventMsg::TokenCount(count) = &event.msg {
            token_info = count.info.clone();
        }
        if matches!(&event.msg, EventMsg::TurnComplete(_)) {
            break;
        }
    }

    let requests = responses.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.body_json()["model"]
                .as_str()
                .unwrap_or_default()
                .to_string())
            .collect::<Vec<_>>(),
        vec![REGULAR_MODEL.to_string(), SIMPLE_MODEL.to_string()]
    );

    let token_info =
        token_info.expect("routing usage should be available even when provider usage is omitted");
    let routing_usage = token_info
        .model_routing_usage
        .expect("adaptive routing usage should be recorded");
    assert_eq!(routing_usage.simple_model_slug, SIMPLE_MODEL);
    assert_eq!(routing_usage.simple_model_token_usage.input_tokens, 0);
    assert_eq!(routing_usage.simple_model_token_usage.output_tokens, 0);
    assert_eq!(routing_usage.simple_model_responses, 1);
    assert_eq!(routing_usage.total_responses, 2);
    assert_eq!(routing_usage.unreported_responses, 2);

    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subagents_only_scope_does_not_route_main_thread_messages() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![sse(vec![
            ev_response_created("main-only"),
            ev_assistant_message("main-message", "Hi!"),
            ev_completed("main-only"),
        ])],
    )
    .await;
    let test = test_codex()
        .with_model(REGULAR_MODEL)
        .with_config(|config| {
            config.agent_model_routing_scope = Some(AgentModelRoutingScope::SubagentsOnly);
            config.agent_simple_model = SIMPLE_MODEL.to_string();
        })
        .build_with_auto_env(&server)
        .await?;

    test.submit_text_turn("hi").await?;

    let request = responses.single_request();
    let body = request.body_json();
    assert_eq!(body["model"], REGULAR_MODEL);
    assert_ne!(body["tool_choice"], "required");
    assert!(
        !input_tool_names(&body)
            .iter()
            .any(|tool| tool == "codex_route_model")
    );

    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simple_subagent_escalation_spawns_a_regular_model_subagent() -> Result<()> {
    skip_if_no_network!(Ok(()));

    const TASK: &str = "Investigate the delegated model-routing behavior.";
    const SIMPLE_SUBAGENT_RESPONSE: &str =
        "Partial analysis. CODEX_SIMPLE_MODEL_ESCALATION: yes: the result needs verification.";
    const REGULAR_SUBAGENT_TASK: &str =
        "REGULAR_MODEL_FOLLOWUP: reassess the simple agent's incomplete analysis.";
    const REGULAR_SUBAGENT_RESULT: &str = "REGULAR_SUBAGENT_RESULT: complete analysis.";
    const SPAWN_NAMESPACE: &str = "collaboration";

    let server = start_mock_server().await;
    let _first_parent_response = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            let body: serde_json::Value =
                request.body_json().expect("request should be valid JSON");
            body["model"] == REGULAR_MODEL && body["input"].to_string().contains(TASK)
        },
        sse(vec![
            ev_response_created("parent-spawn-simple"),
            ev_function_call_with_namespace(
                "spawn-simple-subagent",
                SPAWN_NAMESPACE,
                "spawn_agent",
                &json!({
                    "message": TASK,
                    "model": SIMPLE_MODEL,
                    "task_name": "simple_attempt",
                })
                .to_string(),
            ),
            ev_completed("parent-spawn-simple"),
        ]),
    )
    .await;
    let _parent_waits_for_simple = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            let body: serde_json::Value =
                request.body_json().expect("request should be valid JSON");
            body["model"] == REGULAR_MODEL
                && body["input"].to_string().contains("spawn-simple-subagent")
        },
        sse(vec![
            ev_response_created("parent-waits-for-simple"),
            ev_function_call_with_namespace("wait-simple", SPAWN_NAMESPACE, "wait_agent", "{}"),
            ev_completed("parent-waits-for-simple"),
        ]),
    )
    .await;
    let _simple_subagent_response = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            let body: serde_json::Value =
                request.body_json().expect("request should be valid JSON");
            body["model"] == SIMPLE_MODEL && body["input"].to_string().contains(TASK)
        },
        sse(vec![
            ev_response_created("simple-subagent-response"),
            ev_assistant_message("simple-subagent-message", SIMPLE_SUBAGENT_RESPONSE),
            ev_completed("simple-subagent-response"),
        ]),
    )
    .await;
    let _escalation_parent_response = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            let body: serde_json::Value =
                request.body_json().expect("request should be valid JSON");
            body["model"] == REGULAR_MODEL
                && body["input"].to_string().contains(SIMPLE_SUBAGENT_RESPONSE)
        },
        sse(vec![
            ev_response_created("parent-spawn-regular"),
            ev_function_call_with_namespace(
                "spawn-regular-subagent",
                SPAWN_NAMESPACE,
                "spawn_agent",
                &json!({
                    "message": format!("{REGULAR_SUBAGENT_TASK} Prior output: partial analysis."),
                    "model": REGULAR_MODEL,
                    "task_name": "regular_followup",
                })
                .to_string(),
            ),
            ev_completed("parent-spawn-regular"),
        ]),
    )
    .await;
    let _regular_subagent_response = mount_response_once_match(
        &server,
        |request: &wiremock::Request| {
            let body: serde_json::Value =
                request.body_json().expect("request should be valid JSON");
            body["model"] == REGULAR_MODEL
                && is_regular_followup_agent(&body)
                && body["input"].to_string().contains(REGULAR_SUBAGENT_TASK)
        },
        sse_response(sse(vec![
            ev_response_created("regular-subagent-response"),
            ev_assistant_message("regular-subagent-message", REGULAR_SUBAGENT_RESULT),
            ev_completed("regular-subagent-response"),
        ]))
        .set_delay(std::time::Duration::from_millis(150)),
    )
    .await;
    let _parent_waits_for_regular = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            let body: serde_json::Value =
                request.body_json().expect("request should be valid JSON");
            body["model"] == REGULAR_MODEL
                && body["input"].to_string().contains("spawn-regular-subagent")
                && !body["input"].to_string().contains(REGULAR_SUBAGENT_RESULT)
                && !is_regular_followup_agent(&body)
        },
        sse(vec![
            ev_response_created("parent-waits-for-regular"),
            ev_function_call_with_namespace("wait-regular-1", SPAWN_NAMESPACE, "wait_agent", "{}"),
            ev_completed("parent-waits-for-regular"),
        ]),
    )
    .await;
    let _parent_retries_regular_wait = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            let body: serde_json::Value =
                request.body_json().expect("request should be valid JSON");
            body["model"] == REGULAR_MODEL
                && body["input"].to_string().contains("wait-regular-1")
                && !body["input"].to_string().contains(REGULAR_SUBAGENT_RESULT)
                && !is_regular_followup_agent(&body)
        },
        sse(vec![
            ev_response_created("parent-retries-regular-wait"),
            ev_function_call_with_namespace("wait-regular-2", SPAWN_NAMESPACE, "wait_agent", "{}"),
            ev_completed("parent-retries-regular-wait"),
        ]),
    )
    .await;
    let _parent_retries_regular_wait_again = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            let body: serde_json::Value =
                request.body_json().expect("request should be valid JSON");
            body["model"] == REGULAR_MODEL
                && body["input"].to_string().contains("wait-regular-2")
                && !body["input"].to_string().contains(REGULAR_SUBAGENT_RESULT)
                && !is_regular_followup_agent(&body)
        },
        sse(vec![
            ev_response_created("parent-retries-regular-wait-again"),
            ev_function_call_with_namespace("wait-regular-3", SPAWN_NAMESPACE, "wait_agent", "{}"),
            ev_completed("parent-retries-regular-wait-again"),
        ]),
    )
    .await;
    let _final_parent_response = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            let body: serde_json::Value =
                request.body_json().expect("request should be valid JSON");
            body["model"] == REGULAR_MODEL
                && body["input"].to_string().contains(REGULAR_SUBAGENT_RESULT)
        },
        sse(vec![
            ev_response_created("parent-final-response"),
            ev_assistant_message(
                "parent-final-message",
                "Completed with the regular-model result.",
            ),
            ev_completed("parent-final-response"),
        ]),
    )
    .await;

    let test = test_codex()
        .with_model(REGULAR_MODEL)
        .with_config(|config| {
            config
                .features
                .enable(Feature::Collab)
                .expect("collaboration feature should be available in tests");
            config
                .features
                .enable(Feature::MultiAgentV2)
                .expect("multi-agent v2 feature should be available in tests");
            config.agent_model_routing_scope = Some(AgentModelRoutingScope::SubagentsOnly);
            config.agent_simple_model = SIMPLE_MODEL.to_string();
            let catalog = config.model_catalog.get_or_insert_with(|| {
                bundled_models_response().expect("model fixture should parse")
            });
            for model_slug in [REGULAR_MODEL, SIMPLE_MODEL] {
                if catalog.models.iter().any(|model| model.slug == model_slug) {
                    continue;
                }
                let mut model = bundled_models_response()
                    .expect("bundled models should parse")
                    .models
                    .into_iter()
                    .find(|model| model.slug == "gpt-5.5")
                    .expect("bundled model gpt-5.5 should exist");
                model.slug = model_slug.to_string();
                model.display_name = model_slug.to_string();
                catalog.models.push(model);
            }
        })
        .build_with_auto_env(&server)
        .await?;

    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: TASK.to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let turn_result = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let event = test.codex.next_event().await?;
            if matches!(&event.msg, EventMsg::TurnComplete(_)) {
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await;
    turn_result??;

    let captured_requests = server
        .received_requests()
        .await
        .expect("mock server should capture Responses API requests")
        .iter()
        .filter_map(|request| serde_json::from_slice::<serde_json::Value>(&request.body).ok())
        .collect::<Vec<_>>();
    let request_models = captured_requests
        .iter()
        .map(|request| request["model"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    assert!(
        captured_requests.iter().any(|body| {
            body["model"] == REGULAR_MODEL && body["input"].to_string().contains(TASK)
        }),
        "parent should make the initial decision and spawn the simple subagent; requests: {request_models:?}"
    );
    assert!(
        captured_requests.iter().any(|body| {
            body["model"] == SIMPLE_MODEL && body["input"].to_string().contains(TASK)
        }),
        "the first subagent should use the simple model; requests: {request_models:?}"
    );
    assert!(
        captured_requests.iter().any(|body| {
            body["model"] == REGULAR_MODEL
                && body["input"].to_string().contains(SIMPLE_SUBAGENT_RESPONSE)
        }),
        "the parent should receive the simple subagent's escalation assessment; requests: {request_models:?}"
    );
    assert!(
        captured_requests.iter().any(|body| {
            body["model"] == REGULAR_MODEL
                && body["input"].to_string().contains(REGULAR_SUBAGENT_TASK)
        }),
        "the second subagent should receive a regular-model follow-up task; requests: {request_models:?}"
    );
    assert!(
        captured_requests.iter().any(|body| {
            body["model"] == REGULAR_MODEL
                && body["input"].to_string().contains(REGULAR_SUBAGENT_RESULT)
        }),
        "the parent should wait for and receive the regular-model subagent's result; requests: {request_models:?}"
    );

    test.codex.shutdown_and_wait().await?;
    Ok(())
}
