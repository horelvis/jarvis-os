//! Tool: `network.status` — snapshot del estado de NetworkManager.
//!
//! Categoría: `ReadSystem` (read-only, agrega estado de NM via D-Bus).

use crate::{
    adapter::NetworkManagerAdapter,
    error::Result,
    tool::{Tool, ToolMetadata, ToolOutput},
};
use jarvis_policies::ActionCategory;
use std::sync::Arc;

pub struct NetworkStatusTool {
    adapter: Arc<NetworkManagerAdapter>,
    metadata: ToolMetadata,
}

impl NetworkStatusTool {
    pub fn new(adapter: Arc<NetworkManagerAdapter>) -> Self {
        let metadata = ToolMetadata {
            name: "network.status".to_string(),
            description: "Get current network status from NetworkManager: connection \
                          state (connected/disconnected), hostname, and active connections \
                          with their type (wifi/ethernet/vpn) and interfaces."
                .to_string(),
            category: ActionCategory::ReadSystem,
            args_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        };
        Self { adapter, metadata }
    }
}

#[async_trait::async_trait]
impl Tool for NetworkStatusTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    async fn invoke(&self, _args: &serde_json::Value) -> Result<ToolOutput> {
        let status = self.adapter.status().await?;
        let n_active = status.active_connections.len();
        let state_label = status.state.clone();
        let data = serde_json::to_value(&status)?;
        Ok(ToolOutput::new(data)
            .with_user_message(format!("network: {state_label}, {n_active} active connection(s)")))
    }
}
