//! Tool: `process_list` — enumerates system processes.
//!
//! Read-only snapshot from /proc filesystem. Sorted and truncated by args.

use std::time::Instant;

use async_trait::async_trait;
use ironclaw::context::JobContext;
use ironclaw::tools::{Tool, ToolError, ToolOutput};
use jarvis_policies::{Action, ActionCategory, ActionContext, DefaultPolicy, PolicyEngine};
use serde::Deserialize;
use serde_json::json;

use crate::adapter::process::{ProcessAdapter, SortBy};

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
}

impl ProcessListTool {
    pub fn new(adapter: ProcessAdapter) -> Self {
        Self { adapter }
    }
}

#[async_trait]
impl Tool for ProcessListTool {
    fn name(&self) -> &str {
        "process_list"
    }

    fn description(&self) -> &str {
        "List system processes with PID, name, cmdline, RSS memory, and CPU time. \
         Read-only snapshot from /proc filesystem."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
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
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        // Policy gate (M4: jarvis_policies → safety pipeline)
        let action = Action::new("process_list", ActionCategory::ReadSystem);
        let decision = DefaultPolicy.evaluate(&action, &ActionContext::restrictive());
        if decision.is_deny() {
            return Err(ToolError::NotAuthorized(format!("policy DENY: {decision:?}")));
        }

        let parsed: Args = if params.is_null() {
            Args {
                limit: default_limit(),
                sort_by: SortBy::default(),
            }
        } else {
            serde_json::from_value(params).map_err(|e| {
                ToolError::InvalidParameters(format!(
                    "expected {{ limit?: int, sort_by?: \"pid\"|\"memory\"|\"cpu\"|\"name\" }}: {e}"
                ))
            })?
        };

        let adapter = self.adapter;
        let limit = parsed.limit;
        let sort_by = parsed.sort_by;
        let processes = tokio::task::spawn_blocking(move || adapter.list(limit, sort_by))
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("blocking task join: {e}")))?
            .map_err(|e| ToolError::ExecutionFailed(format!("procfs: {e}")))?;

        Ok(ToolOutput::success(json!(processes), start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false // procfs-derived data; no external untrusted source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn process_list_returns_array_for_valid_args() {
        let adapter = ProcessAdapter::new();
        let tool = ProcessListTool::new(adapter);
        let ctx = JobContext::default();

        let out = tool
            .execute(json!({ "limit": 5, "sort_by": "memory" }), &ctx)
            .await
            .expect("tool should succeed");

        let arr = out.result.as_array().expect("result is JSON array");
        assert!(arr.len() <= 5, "limit honored");
        assert!(!arr.is_empty(), "system has at least one process");
    }

    #[tokio::test]
    async fn process_list_name_is_underscore_form() {
        let tool = ProcessListTool::new(ProcessAdapter::new());
        assert_eq!(tool.name(), "process_list");
    }
}
