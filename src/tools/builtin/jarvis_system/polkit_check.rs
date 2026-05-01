//! Tool: `polkit_check` — query whether a polkit action is authorized.

use std::sync::Arc;
use std::time::Instant;

use crate::context::JobContext;
use crate::tools::tool::{Tool, ToolError, ToolOutput};
use async_trait::async_trait;
use jarvis_policies::{Action, ActionCategory, ActionContext, DefaultPolicy, PolicyEngine};
use serde::Deserialize;
use serde_json::json;

use jarvis_system_tools::adapter::polkit::PolkitAdapter;

#[derive(Debug, Deserialize)]
struct Args {
    /// Polkit action ID, e.g. "org.freedesktop.systemd1.manage-units".
    action_id: String,
}

pub struct PolkitCheckTool {
    adapter: Arc<PolkitAdapter>,
}

impl PolkitCheckTool {
    pub fn new(adapter: Arc<PolkitAdapter>) -> Self {
        Self { adapter }
    }
}

#[async_trait]
impl Tool for PolkitCheckTool {
    fn name(&self) -> &str {
        "polkit_check"
    }

    fn description(&self) -> &str {
        "Check if a polkit action is authorized for the current session. \
         Pure query — does NOT trigger biometric prompts. Returns \
         is_authorized + is_challenge (whether interactive prompt would be \
         needed). Useful to know in advance if pkexec/sudo would succeed."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["action_id"],
            "properties": {
                "action_id": {
                    "type": "string",
                    "description": "Polkit action ID (reverse-domain notation)",
                    "examples": [
                        "org.freedesktop.systemd1.manage-units",
                        "org.freedesktop.NetworkManager.network-control",
                        "org.freedesktop.login1.reboot"
                    ]
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

        let action = Action::new("polkit_check", ActionCategory::ReadSystem);
        if DefaultPolicy
            .evaluate(&action, &ActionContext::restrictive())
            .is_deny()
        {
            return Err(ToolError::NotAuthorized("policy DENY: polkit_check".into()));
        }

        let parsed: Args = serde_json::from_value(params)
            .map_err(|e| ToolError::InvalidParameters(format!("polkit_check args: {e}")))?;

        let check = self
            .adapter
            .check_authorization(&parsed.action_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("polkit: {e}")))?;

        Ok(ToolOutput::success(json!(check), start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires D-Bus system bus + polkit"]
    async fn name_is_polkit_check() {
        let adapter = Arc::new(
            PolkitAdapter::connect_system()
                .await
                .expect("polkit available"),
        );
        let tool = PolkitCheckTool::new(adapter);
        assert_eq!(tool.name(), "polkit_check");
    }
}
