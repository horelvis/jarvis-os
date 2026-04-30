//! Tool: `btrfs_snapshot` — manage btrfs subvolumes and snapshots.
//!
//! Sub-operations: `list` (read), `create` (mutate), `delete` (mutate).
//! Tool category is `MutateSystem` for the conservative policy default;
//! a future split into `btrfs_list_snapshots` (read) and `btrfs_create_snapshot`
//! (mutate) is possible once the HUD differentiates approval flows.

use std::time::Instant;

use async_trait::async_trait;
use crate::context::JobContext;
use crate::tools::tool::{ApprovalRequirement, RiskLevel, Tool, ToolError, ToolOutput};
use jarvis_policies::{Action, ActionCategory, ActionContext, DefaultPolicy, PolicyEngine};
use serde::Deserialize;
use serde_json::json;

use jarvis_system_tools::adapter::btrfs::BtrfsAdapter;

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Args {
    List {
        #[serde(default = "default_mount")]
        mount_path: String,
    },
    Create {
        source: String,
        dest: String,
    },
    Delete {
        path: String,
    },
}

fn default_mount() -> String {
    "/".to_string()
}

pub struct BtrfsSnapshotTool {
    adapter: BtrfsAdapter,
}

impl BtrfsSnapshotTool {
    pub fn new(adapter: BtrfsAdapter) -> Self {
        Self { adapter }
    }
}

#[async_trait]
impl Tool for BtrfsSnapshotTool {
    fn name(&self) -> &str {
        "btrfs_snapshot"
    }

    fn description(&self) -> &str {
        "Manage btrfs subvolumes and snapshots: list existing, create read-only \
         snapshots, or delete. Operations on the filesystem require CONFIRM/ALLOW \
         per jarvis_policies. Returns empty list if mount_path is not on btrfs."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["op"],
            "oneOf": [
                {
                    "properties": {
                        "op": { "const": "list" },
                        "mount_path": {
                            "type": "string",
                            "default": "/",
                            "description": "Path to a btrfs mount point"
                        }
                    }
                },
                {
                    "properties": {
                        "op": { "const": "create" },
                        "source": { "type": "string", "description": "Path to source subvolume" },
                        "dest":   { "type": "string", "description": "Path where snapshot will be created" }
                    },
                    "required": ["source", "dest"]
                },
                {
                    "properties": {
                        "op": { "const": "delete" },
                        "path": { "type": "string", "description": "Path of subvolume/snapshot to delete" }
                    },
                    "required": ["path"]
                }
            ]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let action = Action::new("btrfs_snapshot", ActionCategory::MutateSystem);
        let decision = DefaultPolicy.evaluate(&action, &ActionContext::restrictive());
        if decision.is_deny() {
            return Err(ToolError::NotAuthorized(format!("policy DENY: {decision:?}")));
        }

        let parsed: Args = serde_json::from_value(params)
            .map_err(|e| ToolError::InvalidParameters(format!("btrfs_snapshot args: {e}")))?;

        match parsed {
            Args::List { mount_path } => {
                let subvols = self
                    .adapter
                    .list(&mount_path)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(format!("btrfs list: {e}")))?;
                Ok(ToolOutput::success(json!(subvols), start.elapsed()))
            }
            Args::Create { source, dest } => {
                self.adapter
                    .snapshot_readonly(&source, &dest)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(format!("btrfs create: {e}")))?;
                Ok(ToolOutput::success(
                    json!({ "created": dest, "source": source, "readonly": true }),
                    start.elapsed(),
                ))
            }
            Args::Delete { path } => {
                self.adapter
                    .delete(&path)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(format!("btrfs delete: {e}")))?;
                Ok(ToolOutput::success(
                    json!({ "deleted": path }),
                    start.elapsed(),
                ))
            }
        }
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        // Mutating tool — always surface approval; session auto-approve can bypass.
        ApprovalRequirement::UnlessAutoApproved
    }

    fn risk_level_for(&self, _params: &serde_json::Value) -> RiskLevel {
        // create/delete subvolumes is reversible-ish (snapshots are cheap),
        // not destructive at OS level. Medium fits.
        RiskLevel::Medium
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn name_is_btrfs_snapshot() {
        let tool = BtrfsSnapshotTool::new(BtrfsAdapter::new());
        assert_eq!(tool.name(), "btrfs_snapshot");
    }

    #[tokio::test]
    async fn list_op_runs_against_root_mount() {
        let tool = BtrfsSnapshotTool::new(BtrfsAdapter::new());
        let ctx = JobContext::default();
        let out = tool.execute(json!({"op": "list"}), &ctx).await;
        // Root may not be btrfs in CI; both Ok (with empty list) and
        // ExecutionFailed are acceptable. NotAuthorized is not.
        match out {
            Ok(_) => {}
            Err(ToolError::ExecutionFailed(_)) => {}
            Err(other) => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn requires_approval_unless_auto() {
        let tool = BtrfsSnapshotTool::new(BtrfsAdapter::new());
        assert!(matches!(
            tool.requires_approval(&serde_json::Value::Null),
            ApprovalRequirement::UnlessAutoApproved
        ));
    }
}
