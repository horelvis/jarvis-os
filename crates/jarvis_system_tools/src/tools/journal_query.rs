//! Tool: `journal_query` — query systemd journal entries.

use std::time::Instant;

use async_trait::async_trait;
use ironclaw::context::JobContext;
use ironclaw::tools::{Tool, ToolError, ToolOutput};
use jarvis_policies::{Action, ActionCategory, ActionContext, DefaultPolicy, PolicyEngine};
use serde::Deserialize;
use serde_json::json;

use crate::adapter::journal::{JournalAdapter, Priority};

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
}

impl JournalQueryTool {
    pub fn new(adapter: JournalAdapter) -> Self {
        Self { adapter }
    }
}

#[async_trait]
impl Tool for JournalQueryTool {
    fn name(&self) -> &str {
        "journal_query"
    }

    fn description(&self) -> &str {
        "Query systemd journal entries with optional filters: time range (since), \
         minimum priority, specific unit, max lines. Read-only."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
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
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let action = Action::new("journal_query", ActionCategory::ReadSystem);
        if DefaultPolicy
            .evaluate(&action, &ActionContext::restrictive())
            .is_deny()
        {
            return Err(ToolError::NotAuthorized("policy DENY: journal_query".into()));
        }

        let parsed: Args = if params.is_null() {
            Args {
                since: None,
                priority: None,
                unit: None,
                limit: default_limit(),
            }
        } else {
            serde_json::from_value(params)
                .map_err(|e| ToolError::InvalidParameters(format!("journal_query args: {e}")))?
        };

        let entries = self
            .adapter
            .query(
                parsed.since.as_deref(),
                parsed.priority,
                parsed.unit.as_deref(),
                parsed.limit,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("journalctl: {e}")))?;

        Ok(ToolOutput::success(json!(entries), start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        true // journal entries can contain external data (network logs, app stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn journal_query_name_is_underscore_form() {
        let tool = JournalQueryTool::new(JournalAdapter::new());
        assert_eq!(tool.name(), "journal_query");
    }

    #[tokio::test]
    async fn journal_query_runs_with_default_args() {
        let tool = JournalQueryTool::new(JournalAdapter::new());
        let ctx = JobContext::default();
        let out = tool.execute(json!({}), &ctx).await;
        // Journal may be empty or unreadable in CI; both Ok and
        // ExecutionFailed are acceptable. NotAuthorized is not.
        match out {
            Ok(_) => {}
            Err(ToolError::ExecutionFailed(_)) => {}
            Err(other) => panic!("unexpected error variant: {other:?}"),
        }
    }
}
