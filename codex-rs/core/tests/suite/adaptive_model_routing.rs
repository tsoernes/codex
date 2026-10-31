use anyhow::Result;
use codex_config::config_toml::AgentModelRoutingScope;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;

const REGULAR_MODEL: &str = "gpt-6.1-sol";
const SIMPLE_MODEL: &str = "gpt-6-luna";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_scope_routes_and_escalates_in_the_main_thread() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("route"),
                ev_function_call(
                    "route-call",
                    "codex_route_model",
                    &json!({"tier": "simple"}).to_string(),
                ),
                ev_completed("route"),
            ]),
            sse(vec![
                ev_response_created("simple"),
                ev_assistant_message("message-simple", "A concise answer from the simple model."),
                ev_function_call("escalate-call", "codex_escalate_to_regular_model", "{}"),
                ev_completed("simple"),
            ]),
            sse(vec![
                ev_response_created("regular"),
                ev_assistant_message("message-regular", "The regular model's improved response."),
                ev_completed("regular"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_model_info_override(REGULAR_MODEL, |_| {})
        .with_model_info_override(SIMPLE_MODEL, |_| {})
        .with_model(REGULAR_MODEL)
        .with_config(|config| {
            config.agent_model_routing_scope = Some(AgentModelRoutingScope::All);
            config.agent_simple_model = SIMPLE_MODEL.to_string();
        })
        .build_with_auto_env(&server)
        .await?;

    test.submit_text_turn("Answer this request").await?;

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
    let first_tools = first_request_body["tools"]
        .as_array()
        .expect("routing request should expose a tool list");
    assert_eq!(first_tools.len(), 1);
    assert_eq!(first_tools[0]["name"], "codex_route_model");

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
                ev_function_call(
                    "regular-route-call",
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
        .with_model_info_override(REGULAR_MODEL, |_| {})
        .with_model_info_override(SIMPLE_MODEL, |_| {})
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
