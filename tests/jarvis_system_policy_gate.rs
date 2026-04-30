//! Regression: every jarvis-os system tool invokes the policy gate
//! before doing any I/O (M4).
//!
//! For tools whose default policy is ALLOW under
//! `ActionContext::restrictive()`, a successful invocation must NOT
//! trigger NotAuthorized. Explicit DENY scenarios are deferred until a
//! dynamic policy injection mechanism lands — out of scope for v1.
//!
//! This single ALLOW smoke covers the gate-is-wired check. If a future
//! tool author forgets the `DefaultPolicy.evaluate` call inside
//! `execute()`, this test still passes (the gate's absence does not
//! flip ALLOW into NotAuthorized) — but the tools that DO have it must
//! at least run successfully under the restrictive default.

use ironclaw::context::JobContext;
use ironclaw::tools::{Tool, ToolError};
use ironclaw::tools::builtin::jarvis_system::{
    policy_evaluate::PolicyEvaluateTool, process_list::ProcessListTool,
};
use jarvis_system_tools::adapter::process::ProcessAdapter;
use serde_json::json;

#[tokio::test]
async fn process_list_runs_when_policy_allows() {
    let tool = ProcessListTool::new(ProcessAdapter::new());
    let ctx = JobContext::default();

    let out = tool.execute(json!({"limit": 1}), &ctx).await;
    match out {
        Ok(_) => {}
        Err(other) => panic!(
            "process_list expected ALLOW under DefaultPolicy + restrictive, got: {other:?}"
        ),
    }
}

#[tokio::test]
async fn policy_evaluate_runs_when_policy_allows() {
    // policy_evaluate is read-only meta — must always be reachable.
    let tool = PolicyEvaluateTool::new();
    let ctx = JobContext::default();

    let out = tool
        .execute(
            json!({"tool_name": "process_list", "category": "read_system"}),
            &ctx,
        )
        .await;
    match out {
        Ok(_) => {}
        Err(other) => panic!(
            "policy_evaluate must be reachable under restrictive ctx, got: {other:?}"
        ),
    }
}

#[tokio::test]
async fn invalid_args_surface_as_invalid_parameters_not_authorization() {
    // Sanity: the gate fires BEFORE arg parsing? After? Either way, an
    // arg-malformed call must surface as InvalidParameters, never as
    // NotAuthorized — that would imply the gate accidentally treated
    // the args as a deny signal.
    let tool = ProcessListTool::new(ProcessAdapter::new());
    let ctx = JobContext::default();

    let err = tool
        .execute(json!({"limit": "not-a-number"}), &ctx)
        .await
        .unwrap_err();

    assert!(
        matches!(err, ToolError::InvalidParameters(_)),
        "expected InvalidParameters, got: {err:?}"
    );
}
