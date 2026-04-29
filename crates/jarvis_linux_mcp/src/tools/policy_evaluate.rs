//! Tool: `policy.evaluate` — expone `jarvis_policies::DefaultPolicy` al agente.
//!
//! Permite al LLM razonar sobre qué decidiría jarvis_policies para una acción
//! hipotética sin ejecutarla. Útil para:
//!   - Self-reflection del agente: "¿debería intentar esto o me lo van a denegar?"
//!   - Diagnóstico de seguridad: el usuario pregunta "¿bajo qué condición se
//!     permitiría borrar /etc/foo?" y el agente lo modela.
//!   - Tests E2E de la matriz de policies.
//!
//! Categoría: `ReadSystem` — solo evalúa, no muta nada.

use crate::{
    error::{Error, Result},
    tool::{Tool, ToolMetadata, ToolOutput},
};
use chrono::Local;
use jarvis_policies::{
    Action, ActionCategory, ActionContext, DefaultPolicy, PolicyEngine,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Args {
    /// Nombre de la herramienta hipotética (`systemctl_restart`, `file_delete`, etc.).
    /// Solo se usa para el campo `tool_name` del Action; no afecta la decisión.
    tool_name: String,

    /// Categoría de la acción. Las 6 del spec sec 5.4.2.
    category: ActionCategory,

    /// Contexto opcional. Si no se pasa, defaults restrictivos
    /// (no sysadmin, no auth, ahora, unattended).
    #[serde(default)]
    context: Option<ContextArgs>,
}

#[derive(Debug, Default, Deserialize)]
struct ContextArgs {
    #[serde(default)]
    sysadmin_mode_active: bool,
    #[serde(default)]
    user_authenticated_recently: bool,
    #[serde(default)]
    is_unattended: bool,
}

pub struct PolicyEvaluateTool {
    policy: DefaultPolicy,
    metadata: ToolMetadata,
}

impl PolicyEvaluateTool {
    pub fn new(policy: DefaultPolicy) -> Self {
        let metadata = ToolMetadata {
            name: "policy.evaluate".to_string(),
            description:
                "Evaluate jarvis_policies::DefaultPolicy decision for a hypothetical \
                 action. Returns ALLOW / CONFIRM / DENY verdict with reason. Read-only \
                 reflection — does NOT execute anything. Use this to reason about \
                 whether an action would be permitted under current policy + context."
                    .to_string(),
            category: ActionCategory::ReadSystem,
            args_schema: serde_json::json!({
                "type": "object",
                "required": ["tool_name", "category"],
                "properties": {
                    "tool_name": {
                        "type": "string",
                        "description": "Hypothetical tool name (does not need to exist)",
                        "examples": ["systemctl_restart", "file_delete", "pkexec_install"]
                    },
                    "category": {
                        "type": "string",
                        "enum": [
                            "read_system",
                            "read_sensitive",
                            "mutate_system",
                            "mutate_user_data",
                            "network_outbound",
                            "privileged"
                        ],
                        "description": "Action category from jarvis_policies::ActionCategory"
                    },
                    "context": {
                        "type": "object",
                        "description": "Optional execution context. Defaults to restrictive (no sysadmin, no auth, unattended).",
                        "properties": {
                            "sysadmin_mode_active": {
                                "type": "boolean",
                                "default": false,
                                "description": "Whether sysadmin mode is active (spec sec 6.3)"
                            },
                            "user_authenticated_recently": {
                                "type": "boolean",
                                "default": false,
                                "description": "Whether user did polkit/biometric auth in last few minutes"
                            },
                            "is_unattended": {
                                "type": "boolean",
                                "default": false,
                                "description": "Whether no human is present at the screen"
                            }
                        }
                    }
                }
            }),
        };
        Self { policy, metadata }
    }
}

#[async_trait::async_trait]
impl Tool for PolicyEvaluateTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    async fn invoke(&self, args: &serde_json::Value) -> Result<ToolOutput> {
        let parsed: Args = serde_json::from_value(args.clone()).map_err(|e| {
            Error::InvalidArguments(format!(
                "policy.evaluate args: {e} (expected {{ tool_name: string, category: enum, context?: {{...}} }})"
            ))
        })?;

        let ctx_args = parsed.context.unwrap_or_default();
        let context = ActionContext {
            sysadmin_mode_active: ctx_args.sysadmin_mode_active,
            user_authenticated_recently: ctx_args.user_authenticated_recently,
            is_unattended: ctx_args.is_unattended,
            now: Local::now(),
        };

        let action = Action::new(parsed.tool_name.clone(), parsed.category);
        let decision = self.policy.evaluate(&action, &context);

        // Mensaje legible para el HUD/usuario.
        let user_message = match &decision {
            jarvis_policies::Decision::Allow => {
                format!("ALLOW: {} ({:?})", parsed.tool_name, parsed.category)
            }
            jarvis_policies::Decision::Confirm { reason, timeout_secs } => {
                format!(
                    "CONFIRM: {} requires user approval ({reason:?}, timeout {timeout_secs}s)",
                    parsed.tool_name
                )
            }
            jarvis_policies::Decision::Deny { reason } => {
                format!("DENY: {} blocked ({reason:?})", parsed.tool_name)
            }
        };

        let data = serde_json::json!({
            "action": {
                "tool_name": parsed.tool_name,
                "category": action.category.label(),
            },
            "context": {
                "sysadmin_mode_active": context.sysadmin_mode_active,
                "user_authenticated_recently": context.user_authenticated_recently,
                "is_unattended": context.is_unattended,
                "evaluated_at": context.now.to_rfc3339(),
            },
            "decision": decision,
        });

        Ok(ToolOutput::new(data).with_user_message(user_message))
    }
}
