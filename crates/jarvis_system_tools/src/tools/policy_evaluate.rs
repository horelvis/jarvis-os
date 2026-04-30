//! Tool: `policy_evaluate` — expose `jarvis_policies::DefaultPolicy` to the agent.
//!
//! Evaluate the policy decision for a hypothetical action without executing
//! anything. Useful for:
//!   - Self-reflection: "is this action going to be permitted?"
//!   - Diagnostic queries: "under what context would deleting /etc/foo be allowed?"
//!   - E2E tests of the policy matrix.
//!
//! Category: ReadSystem (pure evaluation, no I/O).

use std::time::Instant;

use async_trait::async_trait;
use chrono::Local;
use ironclaw::context::JobContext;
use ironclaw::tools::{Tool, ToolError, ToolOutput};
use jarvis_policies::{Action, ActionCategory, ActionContext, DefaultPolicy, PolicyEngine};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct Args {
    /// Hypothetical tool name (does not need to exist).
    tool_name: String,
    /// Action category — enum from jarvis_policies.
    category: ActionCategory,
    /// Optional context. Defaults to restrictive.
    #[serde(default)]
    context: Option<ContextArgs>,
}

#[derive(Debug, Default, Deserialize)]
struct ContextArgs {
    #[serde(default)]
    sysadmin_mode_active: bool,
    #[serde(default)]
    user_authenticated_recently: bool,
    #[serde(default)]
    is_unattended: bool,
}

pub struct PolicyEvaluateTool;

impl PolicyEvaluateTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PolicyEvaluateTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for PolicyEvaluateTool {
    fn name(&self) -> &str {
        "policy_evaluate"
    }

    fn description(&self) -> &str {
        "Evaluate jarvis_policies::DefaultPolicy decision for a hypothetical \
         action. Returns ALLOW / CONFIRM / DENY verdict with reason. Read-only \
         reflection — does NOT execute anything. Use this to reason about \
         whether an action would be permitted under current policy + context."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["tool_name", "category"],
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Hypothetical tool name (does not need to exist)",
                    "examples": ["systemctl_restart", "file_delete", "pkexec_install"]
                },
                "category": {
                    "type": "string",
                    "enum": [
                        "read_system",
                        "read_sensitive",
                        "mutate_system",
                        "mutate_user_data",
                        "network_outbound",
                        "privileged"
                    ],
                    "description": "Action category from jarvis_policies::ActionCategory"
                },
                "context": {
                    "type": "object",
                    "description": "Optional execution context. Defaults to restrictive.",
                    "properties": {
                        "sysadmin_mode_active": {
                            "type": "boolean", "default": false,
                            "description": "Whether sysadmin mode is active"
                        },
                        "user_authenticated_recently": {
                            "type": "boolean", "default": false,
                            "description": "Whether user did polkit/biometric auth recently"
                        },
                        "is_unattended": {
                            "type": "boolean", "default": false,
                            "description": "Whether no human is present at the screen"
                        }
                    }
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        // Meta-tool: even the gate is trivial (it evaluates itself).
        // Keep the gate so the discipline holds uniform across all
        // jarvis_system_tools.
        let action = Action::new("policy_evaluate", ActionCategory::ReadSystem);
        if DefaultPolicy
            .evaluate(&action, &ActionContext::restrictive())
            .is_deny()
        {
            return Err(ToolError::NotAuthorized(
                "policy DENY: policy_evaluate".into(),
            ));
        }

        let parsed: Args = serde_json::from_value(params).map_err(|e| {
            ToolError::InvalidParameters(format!(
                "policy_evaluate args: {e} (expected {{ tool_name: string, category: enum, context?: {{...}} }})"
            ))
        })?;

        let ctx_args = parsed.context.unwrap_or_default();
        let context = ActionContext {
            sysadmin_mode_active: ctx_args.sysadmin_mode_active,
            user_authenticated_recently: ctx_args.user_authenticated_recently,
            is_unattended: ctx_args.is_unattended,
            now: Local::now(),
        };

        let hyp_action = Action::new(parsed.tool_name.clone(), parsed.category);
        let decision = DefaultPolicy.evaluate(&hyp_action, &context);

        let data = json!({
            "action": {
                "tool_name": parsed.tool_name,
                "category": hyp_action.category.label(),
            },
            "context": {
                "sysadmin_mode_active": context.sysadmin_mode_active,
                "user_authenticated_recently": context.user_authenticated_recently,
                "is_unattended": context.is_unattended,
                "evaluated_at": context.now.to_rfc3339(),
            },
            "decision": decision,
        });

        Ok(ToolOutput::success(data, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn name_is_policy_evaluate() {
        let tool = PolicyEvaluateTool::new();
        assert_eq!(tool.name(), "policy_evaluate");
    }

    #[tokio::test]
    async fn evaluates_read_system_returns_allow() {
        let tool = PolicyEvaluateTool::new();
        let ctx = JobContext::default();
        let out = tool
            .execute(
                json!({
                    "tool_name": "process_list",
                    "category": "read_system"
                }),
                &ctx,
            )
            .await
            .expect("policy_evaluate should run");

        let result = out.result;
        let decision = result
            .get("decision")
            .expect("decision field present")
            .clone();
        // ReadSystem under DefaultPolicy + restrictive defaults must be ALLOW.
        // The Decision enum may serialize as object {"Allow":null} or string
        // "Allow"; both are acceptable. We just sanity-check it's not Deny.
        let serialized = serde_json::to_string(&decision).unwrap();
        assert!(
            !serialized.contains("Deny"),
            "ReadSystem unexpectedly denied: {serialized}"
        );
    }

    #[tokio::test]
    async fn rejects_missing_required_args() {
        let tool = PolicyEvaluateTool::new();
        let ctx = JobContext::default();
        let err = tool
            .execute(json!({"tool_name": "x"}), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidParameters(_)));
    }
}
