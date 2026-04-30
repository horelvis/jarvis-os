//! Tool: `network_status` — snapshot of NetworkManager state.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use crate::context::JobContext;
use crate::tools::tool::{Tool, ToolError, ToolOutput};
use jarvis_policies::{Action, ActionCategory, ActionContext, DefaultPolicy, PolicyEngine};
use serde_json::json;

use jarvis_system_tools::adapter::network::NetworkManagerAdapter;

pub struct NetworkStatusTool {
    adapter: Arc<NetworkManagerAdapter>,
}

impl NetworkStatusTool {
    pub fn new(adapter: Arc<NetworkManagerAdapter>) -> Self {
        Self { adapter }
    }
}

#[async_trait]
impl Tool for NetworkStatusTool {
    fn name(&self) -> &str {
        "network_status"
    }

    fn description(&self) -> &str {
        "Get current network status from NetworkManager: connection state \
         (connected/disconnected), hostname, and active connections with their \
         type (wifi/ethernet/vpn) and interfaces."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let action = Action::new("network_status", ActionCategory::ReadSystem);
        if DefaultPolicy
            .evaluate(&action, &ActionContext::restrictive())
            .is_deny()
        {
            return Err(ToolError::NotAuthorized("policy DENY: network_status".into()));
        }

        let status = self
            .adapter
            .status()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("D-Bus NetworkManager: {e}")))?;

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
    #[ignore = "requires D-Bus system bus + NetworkManager"]
    async fn name_is_network_status() {
        let adapter = Arc::new(
            NetworkManagerAdapter::connect_system()
                .await
                .expect("NetworkManager available"),
        );
        let tool = NetworkStatusTool::new(adapter);
        assert_eq!(tool.name(), "network_status");
    }
}
