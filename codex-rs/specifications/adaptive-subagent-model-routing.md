# Adaptive subagent model routing

**Status:** Implemented on `feature/adaptive-subagent-model-routing`; focused tests pass.
**Source:** User request in Codex CLI session, 2026-10-31

## User-specified requirements

- Provide a `config.toml` flag to enable model routing for delegated subagent tasks.
- Before dispatching a delegated task, the main/default model decides whether a simpler configured model is sufficient.
- Route the task to either the normal/default model or the configured simpler model based on that decision.
- When the simpler model handles a task, it should finish its response and then assess whether the normal model could give a materially better response because the simpler model did not answer adequately.
- If the simpler model recommends escalation, wait for a response from the normal model before treating the task as complete.
- The user has now requested two routing features: (1) route all messages and subagents; and (2) route only when the main thread asks a subagent to perform a task.
- Use `gpt-6.1-sol` as the regular model and `gpt-6-luna` as the simple model.
- If a main-thread message is routed to the simpler model, the response remains in the main thread.
- If a simpler-model main-thread response recommends escalation, continue with the regular model in that same main thread.
- If a simpler-model subagent recommends escalation, launch another subagent using the normal/default model.

## Agent-proposed implementation choices (not user requirements)

- Keep routing opt-in so existing model selection remains unchanged unless enabled.
- Represent the two requested scopes with `[agents].model_routing = "all"` or `"subagents_only"`; use `[agents].simple_model` for the simple-tier model and default the regular tier to the current/default model.
- In `all` scope, make the initial main-thread model call a bounded routing-tool decision, then switch subsequent sampling within the same main thread. The simpler main-thread model can request same-thread continuation with the regular model.
- Require the routing tool on the initial classification request so the model cannot skip the model-selection step and answer directly.
- In either enabled scope, guide the main model to select the simple model on child spawn; a simple child reports an escalation marker, and the main model retries the task in a new regular-model subagent.
- Use a bounded machine-readable escalation marker in the simpler agent's final response; the parent model should continue with the normal model when the marker requests escalation.
- Prefer one escalation attempt and preserve existing explicit model/role choices unless the routing tier was explicitly selected.

## History

- 2026-10-31: Initial requirements recorded from the user's implementation request. No implementation decisions above are treated as user requirements.
- 2026-10-31: User expanded the request to include all-message-plus-subagent routing as well as subagent-only routing.
- 2026-10-31: User clarified that simpler-model top-level responses stay in the main thread and a simpler subagent's escalation launches a regular-model subagent.
- 2026-10-31: User confirmed the target pair `gpt-6.1-sol` (regular) and `gpt-6-luna` (simple); the two scope behaviors are `all` and `subagents_only`.
- 2026-10-31: Implementation completed with an opt-in scope selector, configurable simple model, main-thread routing/escalation tools, and simple-subagent escalation guidance. The exact config keys and routing-tool mechanism are agent choices.
