//! # jarvis_policies
//!
//! Motor de políticas de jarvis-os: decisiones ALLOW / CONFIRM / DENY
//! sobre acciones del agente al sistema operativo.
//!
//! Esta crate extiende `ironclaw_safety` añadiendo una capa de decisión
//! a nivel de **acción concreta sobre el SO** (no a nivel de texto del
//! LLM). Las dos capas se complementan: ironclaw_safety mira el texto,
//! jarvis_policies mira la acción derivada del texto.
//!
//! ## Uso típico
//!
//! ```no_run
//! use jarvis_policies::{Action, ActionCategory, ActionContext, DefaultPolicy, PolicyEngine};
//!
//! let policy = DefaultPolicy;
//! let action = Action::new("systemctl_restart", ActionCategory::MutateSystem);
//! let ctx = ActionContext::restrictive();
//!
//! match policy.evaluate(&action, &ctx) {
//!     d if d.is_allow() => { /* ejecutar inmediato */ }
//!     d if d.requires_confirmation() => { /* HUD inline confirmation */ }
//!     d if d.is_deny() => { /* devolver al agente como rechazo */ }
//!     _ => unreachable!(),
//! }
//! ```
//!
//! ## Reemplazo de OPA
//!
//! La spec sec 6.1 capa 2 menciona "OPA (Rego) embebido". Decisión cerrada
//! del proyecto: **no usar OPA externo**. Toda la lógica de políticas vive
//! en este crate, en Rust nativo. Esto reduce dependencias, mejora
//! performance (no IPC ni evaluación dinámica), y mantiene la política
//! revisable como código auditado en el mismo repo.
//!
//! Si en algún momento se necesitara políticas dinámicas reload-ables sin
//! recompilar, se evaluará añadir un sub-crate `jarvis_policies_dynamic`
//! con un DSL propio o YAML, pero NO Rego.

pub mod action;
pub mod adapter;
pub mod decision;
pub mod policy;

pub use action::{Action, ActionCategory, ActionContext};
pub use adapter::CombinedSafety;
pub use decision::{ConfirmReason, Decision, DenyReason};
pub use policy::{DefaultPolicy, PolicyEngine};
