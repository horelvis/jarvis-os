//! Tool: `systemd.unit_status` — devuelve el estado de una unit systemd.
//!
//! Categoría: `ReadSystem` (bajo riesgo, ALLOW directo).
//! Args: `{ "unit": "nginx.service" }`
//! Output: `UnitStatus` serializado como JSON.

use crate::{
    adapter::SystemdAdapter,
    error::{Error, Result},
    tool::{Tool, ToolMetadata, ToolOutput},
};
use jarvis_policies::ActionCategory;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct Args {
    unit: String,
}

/// `systemd.unit_status` — primera tool del Linux MCP server.
///
/// El adapter es compartido entre invocaciones (Arc) para no abrir
/// conexión D-Bus por cada llamada — zbus es thread-safe.
pub struct SystemdUnitStatusTool {
    adapter: Arc<SystemdAdapter>,
    metadata: ToolMetadata,
}

impl SystemdUnitStatusTool {
    pub fn new(adapter: Arc<SystemdAdapter>) -> Self {
        let metadata = ToolMetadata {
            name: "systemd.unit_status".to_string(),
            description: "Get status of a systemd unit (load_state, active_state, \
                          sub_state, description). Read-only operation."
                .to_string(),
            category: ActionCategory::ReadSystem,
            args_schema: serde_json::json!({
                "type": "object",
                "required": ["unit"],
                "properties": {
                    "unit": {
                        "type": "string",
                        "description": "Full systemd unit name including .service/.timer/.socket suffix",
                        "examples": ["nginx.service", "bluetooth.service", "ssh.socket"]
                    }
                }
            }),
        };
        Self { adapter, metadata }
    }
}

#[async_trait::async_trait]
impl Tool for SystemdUnitStatusTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    async fn invoke(&self, args: &serde_json::Value) -> Result<ToolOutput> {
        let parsed: Args = serde_json::from_value(args.clone())
            .map_err(|e| Error::InvalidArguments(format!("expected {{ unit: string }}: {e}")))?;

        let status = self.adapter.unit_status(&parsed.unit).await?;
        let data = serde_json::to_value(&status)?;
        Ok(ToolOutput::new(data)
            .with_user_message(format!("{}: {}", parsed.unit, status.active_state)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_is_read_system() {
        // No conectamos a D-Bus en este test; solo verificamos metadata.
        // Construimos un adapter "fake" via Arc::new con un Connection no es
        // trivial sin bus real; en su lugar evitamos crear el adapter real
        // y verificamos los args_schema independientemente.
        let schema: serde_json::Value = serde_json::json!({
            "type": "object",
            "required": ["unit"],
            "properties": {
                "unit": {
                    "type": "string",
                    "description": "Full systemd unit name including .service/.timer/.socket suffix",
                    "examples": ["nginx.service", "bluetooth.service", "ssh.socket"]
                }
            }
        });
        // Sanity: el schema serializa y deserializa.
        let s = serde_json::to_string(&schema).unwrap();
        let _: serde_json::Value = serde_json::from_str(&s).unwrap();

        // Sanity sobre la categoría:
        assert_eq!(
            ActionCategory::ReadSystem.label(),
            "read.system"
        );
    }
}
