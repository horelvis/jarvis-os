//! Tool: `systemd_unit_status` — query state of a systemd unit.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use ironclaw::context::JobContext;
use ironclaw::tools::{Tool, ToolError, ToolOutput};
use jarvis_policies::{Action, ActionCategory, ActionContext, DefaultPolicy, PolicyEngine};
use serde::Deserialize;
use serde_json::json;

use crate::adapter::systemd::SystemdAdapter;

#[derive(Debug, Deserialize)]
struct Args {
    /// Systemd unit name, e.g. "bluetooth.service" or "graphical.target"
    unit: String,
}

pub struct SystemdUnitStatusTool {
    adapter: Arc<SystemdAdapter>,
}

impl SystemdUnitStatusTool {
    pub fn new(adapter: Arc<SystemdAdapter>) -> Self {
        Self { adapter }
    }
}

#[async_trait]
impl Tool for SystemdUnitStatusTool {
    fn name(&self) -> &str {
        "systemd_unit_status"
    }

    fn description(&self) -> &str {
        "Get status of a systemd unit (load_state, active_state, sub_state, description). \
         Read-only via D-Bus org.freedesktop.systemd1."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["unit"],
            "properties": {
                "unit": {
                    "type": "string",
                    "description": "Full systemd unit name including .service/.timer/.socket suffix",
                    "examples": ["nginx.service", "bluetooth.service", "ssh.socket"]
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

        let action = Action::new("systemd_unit_status", ActionCategory::ReadSystem);
        if DefaultPolicy
            .evaluate(&action, &ActionContext::restrictive())
            .is_deny()
        {
            return Err(ToolError::NotAuthorized(
                "policy DENY: systemd_unit_status".into(),
            ));
        }

        let parsed: Args = serde_json::from_value(params).map_err(|e| {
            ToolError::InvalidParameters(format!("expected {{ unit: string }}: {e}"))
        })?;

        let status = self
            .adapter
            .unit_status(&parsed.unit)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("D-Bus systemd: {e}")))?;

        Ok(ToolOutput::success(json!(status), start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires D-Bus system bus + systemd"]
    async fn name_is_systemd_unit_status() {
        let adapter = Arc::new(
            SystemdAdapter::connect_system()
                .await
                .expect("system bus available"),
        );
        let tool = SystemdUnitStatusTool::new(adapter);
        assert_eq!(tool.name(), "systemd_unit_status");
    }

    #[tokio::test]
    #[ignore = "requires D-Bus system bus + systemd"]
    async fn rejects_missing_unit_arg() {
        let adapter = Arc::new(
            SystemdAdapter::connect_system()
                .await
                .expect("system bus available"),
        );
        let tool = SystemdUnitStatusTool::new(adapter);
        let ctx = JobContext::default();
        let err = tool.execute(json!({}), &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidParameters(_)));
    }
}
