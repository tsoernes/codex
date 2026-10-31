use crate::function_tool::FunctionCallError;
use crate::session::turn_context::AdaptiveModelRoutingState;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::protocol::TurnSettingsUpdateOutcome;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use std::collections::BTreeMap;

const ROUTE_TOOL: &str = "codex_route_model";
const ESCALATE_TOOL: &str = "codex_escalate_to_regular_model";

pub(crate) struct RouteModelHandler;

impl ToolExecutor<ToolInvocation> for RouteModelHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(ROUTE_TOOL)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: ROUTE_TOOL.to_string(),
            description: "Before answering this main-thread request, classify whether the simpler configured model can answer it adequately. Choose `simple` or `regular`; Codex switches the next response model in this same thread.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: JsonSchema::object(
                BTreeMap::from([(
                    "tier".to_string(),
                    JsonSchema::string(Some(
                        "Use `simple` when the configured simpler model is sufficient; otherwise use `regular`."
                            .to_string(),
                    )),
                )]),
                Some(vec!["tier".to_string()]),
                Some(false.into()),
            ),
            output_schema: None,
        })
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(async move {
            let ToolPayload::Function { arguments } = &invocation.payload else {
                return Err(FunctionCallError::RespondToModel(
                    "route_model received unsupported payload".to_string(),
                ));
            };
            let args: RouteModelArgs = parse_arguments(arguments)?;
            let target = match args.tier.as_str() {
                "simple" => AdaptiveModelRoutingState::Simple,
                "regular" => AdaptiveModelRoutingState::Regular,
                _ => {
                    return Err(FunctionCallError::RespondToModel(
                        "route_model tier must be `simple` or `regular`".to_string(),
                    ));
                }
            };
            apply_routing_selection(&invocation, target).await?;
            let model = match target {
                AdaptiveModelRoutingState::Simple => {
                    invocation.turn.config.agent_simple_model.as_str()
                }
                AdaptiveModelRoutingState::Regular => invocation.turn.model_info().slug.as_str(),
                _ => unreachable!("route_model only accepts simple or regular"),
            };
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                format!("This main-thread response will use `{model}`."),
                Some(true),
            )))
        })
    }
}

impl CoreToolRuntime for RouteModelHandler {
    fn is_builtin_control_tool(&self) -> bool {
        true
    }
}

pub(crate) struct EscalateModelHandler;

impl ToolExecutor<ToolInvocation> for EscalateModelHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(ESCALATE_TOOL)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: ESCALATE_TOOL.to_string(),
            description: "After giving your best answer, call this only if the regular model could materially improve it. Codex will continue in the same main thread using the regular model.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: JsonSchema::object(BTreeMap::new(), None, Some(false.into())),
            output_schema: None,
        })
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(async move {
            apply_routing_selection(&invocation, AdaptiveModelRoutingState::Escalated).await?;
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                "Continuing with the regular model in this main thread.".to_string(),
                Some(true),
            )))
        })
    }
}

impl CoreToolRuntime for EscalateModelHandler {
    fn is_builtin_control_tool(&self) -> bool {
        true
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteModelArgs {
    tier: String,
}

async fn apply_routing_selection(
    invocation: &ToolInvocation,
    target: AdaptiveModelRoutingState,
) -> Result<(), FunctionCallError> {
    match invocation
        .session
        .apply_adaptive_model_route(&invocation.turn.sub_id, target)
        .await
    {
        TurnSettingsUpdateOutcome::Applied => Ok(()),
        TurnSettingsUpdateOutcome::Rejected { reason } => {
            Err(FunctionCallError::RespondToModel(reason))
        }
        TurnSettingsUpdateOutcome::TargetUnavailable => Err(FunctionCallError::RespondToModel(
            "the current turn is no longer available for adaptive model routing".to_string(),
        )),
    }
}
