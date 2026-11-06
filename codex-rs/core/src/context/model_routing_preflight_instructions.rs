use super::ContextualUserFragment;
use codex_protocol::models::ContentItemKind;
use codex_protocol::models::input_text;

/// Keeps the first request in all-scope routing focused on selecting a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelRoutingPreflightInstructions;

impl ContextualUserFragment for ModelRoutingPreflightInstructions {
    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("model_routing.preflight_instructions".to_string())
    }

    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<model_routing_preflight>", "</model_routing_preflight>")
    }

    fn body(&self) -> codex_protocol::models::ContentItem {
        input_text(
            "This response is an internal model-routing preflight, not the answer to the user's request. Do not answer the request, describe missing tools, or produce user-facing text. Your only task is to call `codex_route_model` exactly once with `simple` if that model can adequately handle the request, otherwise `regular` (choose `regular` when uncertain). Codex will then issue a separate request with the selected model and its normal tools.",
        )
    }
}
