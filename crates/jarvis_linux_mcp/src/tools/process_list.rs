//! Tool: `process.list` — enumera procesos del sistema.
//!
//! Categoría: `ReadSystem` (bajo riesgo, ALLOW directo).
//! Args: `{ "limit": 25, "sort_by": "memory" }` (todos opcionales).
//! Output: array de `ProcessInfo`.

use crate::{
    adapter::process::{ProcessAdapter, SortBy},
    error::{Error, Result},
    tool::{Tool, ToolMetadata, ToolOutput},
};
use jarvis_policies::ActionCategory;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    sort_by: SortBy,
}

fn default_limit() -> usize {
    25
}

pub struct ProcessListTool {
    adapter: ProcessAdapter,
    metadata: ToolMetadata,
}

impl ProcessListTool {
    pub fn new(adapter: ProcessAdapter) -> Self {
        let metadata = ToolMetadata {
            name: "process.list".to_string(),
            description: "List system processes with PID, name, cmdline, RSS memory, \
                          and CPU time. Read-only snapshot from /proc filesystem."
                .to_string(),
            category: ActionCategory::ReadSystem,
            args_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 200,
                        "default": 25,
                        "description": "Max number of processes to return (after sorting)"
                    },
                    "sort_by": {
                        "type": "string",
                        "enum": ["pid", "memory", "cpu", "name"],
                        "default": "memory",
                        "description": "Field to sort by (descending for memory/cpu, ascending for pid/name)"
                    }
                }
            }),
        };
        Self { adapter, metadata }
    }
}

#[async_trait::async_trait]
impl Tool for ProcessListTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    async fn invoke(&self, args: &serde_json::Value) -> Result<ToolOutput> {
        // Args opcionales — si vienen vacíos {}, defaults se aplican.
        let parsed: Args = if args.is_null() {
            Args {
                limit: default_limit(),
                sort_by: SortBy::default(),
            }
        } else {
            serde_json::from_value(args.clone()).map_err(|e| {
                Error::InvalidArguments(format!(
                    "expected {{ limit?: int, sort_by?: \"pid\"|\"memory\"|\"cpu\"|\"name\" }}: {e}"
                ))
            })?
        };

        // procfs es síncrono y rápido (~10-50 ms para todos los procesos
        // en un sistema típico). Lo movemos a un blocking thread por
        // higiene del runtime tokio. Adapter es Copy, sin gymnastics.
        let adapter = self.adapter;
        let limit = parsed.limit;
        let sort_by = parsed.sort_by;
        let processes = tokio::task::spawn_blocking(move || adapter.list(limit, sort_by))
            .await
            .map_err(|e| Error::Internal(format!("blocking task join: {e}")))??;

        let count = processes.len();
        let data = serde_json::to_value(&processes)?;
        Ok(ToolOutput::new(data)
            .with_user_message(format!("listed {count} processes (sort: {sort_by:?})")))
    }
}
