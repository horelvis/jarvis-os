//! Tool: `polkit.check` — consulta si una action polkit está autorizada.
//!
//! Categoría: ReadSystem (consulta pasiva, no triggers prompt biométrico).

use crate::{
    adapter::PolkitAdapter,
    error::{Error, Result},
    tool::{Tool, ToolMetadata, ToolOutput},
};
use jarvis_policies::ActionCategory;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct Args {
    /// ID de la acción polkit a consultar (p.ej.
    /// `org.freedesktop.systemd1.manage-units`).
    action_id: String,
}

pub struct PolkitCheckTool {
    adapter: Arc<PolkitAdapter>,
    metadata: ToolMetadata,
}

impl PolkitCheckTool {
    pub fn new(adapter: Arc<PolkitAdapter>) -> Self {
        let metadata = ToolMetadata {
            name: "polkit.check".to_string(),
            description: "Check if a polkit action is authorized for the current \
                          session. Pure query — does NOT trigger biometric prompts. \
                          Returns is_authorized + is_challenge (whether interactive \
                          prompt would be needed). Useful to know in advance if \
                          pkexec/sudo would succeed before attempting."
                .to_string(),
            category: ActionCategory::ReadSystem,
            args_schema: serde_json::json!({
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
            }),
        };
        Self { adapter, metadata }
    }
}

#[async_trait::async_trait]
impl Tool for PolkitCheckTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    async fn invoke(&self, args: &serde_json::Value) -> Result<ToolOutput> {
        let parsed: Args = serde_json::from_value(args.clone())
            .map_err(|e| Error::InvalidArguments(format!("polkit.check args: {e}")))?;
        let check = self.adapter.check_authorization(&parsed.action_id).await?;

        let user_msg = if check.is_authorized {
            format!("{}: AUTHORIZED", check.action_id)
        } else if check.is_challenge {
            format!("{}: would prompt biometric/password", check.action_id)
        } else {
            format!("{}: NOT authorized", check.action_id)
        };

        let data = serde_json::to_value(&check)?;
        Ok(ToolOutput::new(data).with_user_message(user_msg))
    }
}
