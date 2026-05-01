#![allow(dead_code)] // consumers wired in Tracks D2/E

use uuid::Uuid;

use crate::agent::submission::Submission;
use crate::channels::IncomingMessage;
use crate::channels::local_ipc::protocol::{ApprovalAction, ClientCommand, ClientId};

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("invalid request_id (expected UUID): {0}")]
    InvalidRequestId(String),
}

/// Translate a non-Message ClientCommand into the IncomingMessage the
/// agent loop expects. Uses the typed `with_structured_submission`
/// sideband instead of serializing JSON into `content` (cleaner than
/// the legacy web-channel pattern; agent_loop reads the sideband first
/// at src/agent/agent_loop.rs:1412 before any content parsing).
///
/// Returns `Ok(None)` for `Ping` (no submission — handled at the writer
/// task as a wire-only no-op) and for `Message` (built directly in the
/// reader task with the user's text as `content`).
pub fn build_control_submission(
    cmd: &ClientCommand,
    user_id: &str,
    client_id: &ClientId,
) -> Result<Option<IncomingMessage>, ControlError> {
    let submission = match cmd {
        ClientCommand::Approval { request_id, action } => {
            let rid = Uuid::parse_str(request_id)
                .map_err(|_| ControlError::InvalidRequestId(request_id.clone()))?;
            Submission::ExecApproval {
                request_id: rid,
                approved: matches!(action, ApprovalAction::Approve),
                always: false,
            }
        }
        ClientCommand::Cancel { step_id: _ } => {
            // step_id is informational; engine v2 Interrupt cancels the
            // current turn for this user_id regardless.
            Submission::Interrupt
        }
        ClientCommand::Message { .. } | ClientCommand::Ping => return Ok(None),
    };
    let metadata = serde_json::json!({ "client_id": client_id.as_str() });
    Ok(Some(
        IncomingMessage::new("local_ipc", user_id, "")
            .with_structured_submission(submission)
            .with_metadata(metadata),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cid() -> ClientId {
        ClientId::new("c1").unwrap()
    }

    #[test]
    fn approval_builds_exec_approval_sideband() {
        let req_id = Uuid::new_v4();
        let cmd = ClientCommand::Approval {
            request_id: req_id.to_string(),
            action: ApprovalAction::Approve,
        };
        let msg = build_control_submission(&cmd, "owner", &cid())
            .unwrap()
            .expect("approval must produce a submission");
        assert_eq!(msg.channel, "local_ipc");
        assert_eq!(msg.content, "", "sideband used; content stays empty");
        match msg.structured_submission.expect("sideband set") {
            Submission::ExecApproval {
                request_id,
                approved,
                always,
            } => {
                assert_eq!(request_id, req_id);
                assert!(approved);
                assert!(!always);
            }
            other => panic!("expected ExecApproval, got {other:?}"),
        }
    }

    #[test]
    fn approval_deny_sideband_marks_not_approved() {
        let cmd = ClientCommand::Approval {
            request_id: Uuid::new_v4().to_string(),
            action: ApprovalAction::Deny,
        };
        let msg = build_control_submission(&cmd, "owner", &cid())
            .unwrap()
            .expect("must produce");
        assert!(matches!(
            msg.structured_submission.unwrap(),
            Submission::ExecApproval {
                approved: false,
                ..
            }
        ));
    }

    #[test]
    fn invalid_request_id_rejected() {
        let cmd = ClientCommand::Approval {
            request_id: "not-a-uuid".into(),
            action: ApprovalAction::Approve,
        };
        let res = build_control_submission(&cmd, "owner", &cid());
        assert!(matches!(res, Err(ControlError::InvalidRequestId(_))));
    }

    #[test]
    fn cancel_builds_interrupt_sideband() {
        let cmd = ClientCommand::Cancel { step_id: None };
        let msg = build_control_submission(&cmd, "owner", &cid())
            .unwrap()
            .unwrap();
        assert!(matches!(
            msg.structured_submission.unwrap(),
            Submission::Interrupt
        ));
    }

    #[test]
    fn ping_yields_no_submission() {
        let res = build_control_submission(&ClientCommand::Ping, "owner", &cid()).unwrap();
        assert!(res.is_none());
    }

    #[test]
    fn message_yields_no_submission() {
        let cmd = ClientCommand::Message {
            content: "hi".into(),
            thread_id: None,
        };
        let res = build_control_submission(&cmd, "owner", &cid()).unwrap();
        assert!(res.is_none(), "Message routing happens in the reader task");
    }

    #[test]
    fn metadata_carries_client_id() {
        let cmd = ClientCommand::Cancel { step_id: None };
        let msg = build_control_submission(&cmd, "owner", &cid())
            .unwrap()
            .unwrap();
        // IncomingMessage.metadata is serde_json::Value (Null by default,
        // not Option). Verified at src/channels/channel.rs:101.
        assert_eq!(msg.metadata["client_id"], "c1");
    }
}
