//! Tool: `journal.query` — consulta el journal de systemd.
//!
//! Categoría: `ReadSystem`.
//! Args: `{ "since": "5min ago", "priority": "err", "unit": "...", "limit": 50 }`.

use crate::{
    adapter::journal::{JournalAdapter, Priority},
    error::{Error, Result},
    tool::{Tool, ToolMetadata, ToolOutput},
};
use jarvis_policies::ActionCategory;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    priority: Option<Priority>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

pub struct JournalQueryTool {
    adapter: JournalAdapter,
    metadata: ToolMetadata,
}

impl JournalQueryTool {
    pub fn new(adapter: JournalAdapter) -> Self {
        let metadata = ToolMetadata {
            name: "journal.query".to_string(),
            description: "Query systemd journal entries with optional filters: \
                          time range (since), minimum priority, specific unit, max lines. \
                          Read-only."
                .to_string(),
            category: ActionCategory::ReadSystem,
            args_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "since": {
                        "type": "string",
                        "description": "Time range in journalctl syntax",
                        "examples": ["5min ago", "1h ago", "today", "2026-04-28"]
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"],
                        "description": "Min priority level (err = err+crit+alert+emerg)"
                    },
                    "unit": {
                        "type": "string",
                        "description": "Filter by systemd unit name",
                        "examples": ["nginx.service", "bluetooth.service"]
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 500,
                        "default": 50,
                        "description": "Max entries to return (most recent first)"
                    }
                }
            }),
        };
        Self { adapter, metadata }
    }
}

#[async_trait::async_trait]
impl Tool for JournalQueryTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    async fn invoke(&self, args: &serde_json::Value) -> Result<ToolOutput> {
        let parsed: Args = if args.is_null() {
            Args {
                since: None,
                priority: None,
                unit: None,
                limit: default_limit(),
            }
        } else {
            serde_json::from_value(args.clone())
                .map_err(|e| Error::InvalidArguments(format!("journal.query args: {e}")))?
        };

        let entries = self
            .adapter
            .query(
                parsed.since.as_deref(),
                parsed.priority,
                parsed.unit.as_deref(),
                parsed.limit,
            )
            .await?;

        let count = entries.len();
        let data = serde_json::to_value(&entries)?;
        Ok(ToolOutput::new(data).with_user_message(format!("{count} journal entries")))
    }
}
