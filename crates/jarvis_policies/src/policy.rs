//! Motor de políticas: el trait `PolicyEngine` y la implementación default.
//!
//! La política default refleja la matriz que se desprende de la spec
//! secciones 6.x y 7.x. Implementadores pueden swap-ear con su propio
//! `PolicyEngine` para escenarios especiales (CI, testing, modo kiosco,
//! perfil familiar, etc.).

use crate::{
    action::{Action, ActionCategory, ActionContext},
    decision::{ConfirmReason, Decision, DenyReason},
};

/// Trait que cualquier motor de políticas debe implementar.
///
/// Send + Sync para poder compartirlo con `Arc<dyn PolicyEngine>` entre
/// tareas tokio sin gymnastics.
pub trait PolicyEngine: Send + Sync {
    /// Evalúa la acción contra la política, devuelve la decisión.
    fn evaluate(&self, action: &Action, context: &ActionContext) -> Decision;
}

/// Implementación default: matriz simple por categoría + contexto.
///
/// Todas las decisiones tienen rationale comentado al lado de cada arm
/// del match, referenciando la sección de spec que la justifica.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPolicy;

impl PolicyEngine for DefaultPolicy {
    fn evaluate(&self, action: &Action, ctx: &ActionContext) -> Decision {
        match action.category {
            // Lecturas de sistema: bajo riesgo, ALLOW directo.
            // Spec sec 7.1 (flujo nominal de lectura).
            ActionCategory::ReadSystem => Decision::Allow,

            // Datos sensibles: siempre confirmar al usuario.
            // Spec sec 6.x: el usuario debe ser explícitamente consciente
            // cuando el agente accede a su data privada.
            ActionCategory::ReadSensitive => Decision::Confirm {
                reason: ConfirmReason::SensitiveData,
                timeout_secs: 30,
            },

            // Mutación de sistema: en modo sysadmin se relaja a ALLOW
            // (spec sec 6.3 dice "se relajan algunas políticas CONFIRM
            // hacia ALLOW para reducir fricción durante mantenimiento").
            // Fuera de sysadmin, siempre confirmar.
            ActionCategory::MutateSystem => {
                if ctx.sysadmin_mode_active {
                    Decision::Allow
                } else {
                    Decision::Confirm {
                        reason: ConfirmReason::Destructive,
                        timeout_secs: 30,
                    }
                }
            }

            // Mutación de datos del usuario: siempre confirmar.
            // No relajar en sysadmin — sysadmin es para sistema, no para
            // pisar archivos del usuario sin permiso.
            ActionCategory::MutateUserData => Decision::Confirm {
                reason: ConfirmReason::HighImpact,
                timeout_secs: 30,
            },

            // Red saliente: ALLOW por defecto.
            // F2+ añadirá políticas finas a nivel hostname/dominio.
            ActionCategory::NetworkOutbound => Decision::Allow,

            // Privileged: depende de autenticación reciente.
            //   - sin auth reciente y sin sysadmin → DENY (forzar polkit primero).
            //   - con auth reciente → CONFIRM (el usuario sabe qué pasa).
            //   - sysadmin activo → CONFIRM (no ALLOW: privileged es siempre
            //     visible para auditoría incluso en sysadmin).
            ActionCategory::Privileged => {
                if !ctx.user_authenticated_recently && !ctx.sysadmin_mode_active {
                    Decision::Deny {
                        reason: DenyReason::UnauthorizedSysadmin,
                    }
                } else {
                    Decision::Confirm {
                        reason: ConfirmReason::Destructive,
                        timeout_secs: 60,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    fn ctx_idle() -> ActionContext {
        ActionContext {
            sysadmin_mode_active: false,
            user_authenticated_recently: false,
            now: Local::now(),
            is_unattended: false,
        }
    }

    fn ctx_sysadmin() -> ActionContext {
        ActionContext {
            sysadmin_mode_active: true,
            user_authenticated_recently: true,
            now: Local::now(),
            is_unattended: false,
        }
    }

    #[test]
    fn read_system_is_always_allow() {
        let p = DefaultPolicy;
        let a = Action::new("ps_list", ActionCategory::ReadSystem);
        assert_eq!(p.evaluate(&a, &ctx_idle()), Decision::Allow);
        assert_eq!(p.evaluate(&a, &ctx_sysadmin()), Decision::Allow);
    }

    #[test]
    fn mutate_system_requires_confirm_normally() {
        let p = DefaultPolicy;
        let a = Action::new("systemctl_restart", ActionCategory::MutateSystem);
        assert!(p.evaluate(&a, &ctx_idle()).requires_confirmation());
    }

    #[test]
    fn mutate_system_allowed_in_sysadmin() {
        let p = DefaultPolicy;
        let a = Action::new("systemctl_restart", ActionCategory::MutateSystem);
        assert_eq!(p.evaluate(&a, &ctx_sysadmin()), Decision::Allow);
    }

    #[test]
    fn privileged_denied_without_auth() {
        let p = DefaultPolicy;
        let a = Action::new("pkexec_install", ActionCategory::Privileged);
        let d = p.evaluate(&a, &ctx_idle());
        assert!(d.is_deny());
    }

    #[test]
    fn privileged_confirms_with_auth_or_sysadmin() {
        let p = DefaultPolicy;
        let a = Action::new("pkexec_install", ActionCategory::Privileged);
        assert!(p.evaluate(&a, &ctx_sysadmin()).requires_confirmation());

        let mut ctx = ctx_idle();
        ctx.user_authenticated_recently = true;
        assert!(p.evaluate(&a, &ctx).requires_confirmation());
    }

    #[test]
    fn user_data_mutation_always_confirms() {
        let p = DefaultPolicy;
        let a = Action::new("file_delete", ActionCategory::MutateUserData);
        assert!(p.evaluate(&a, &ctx_idle()).requires_confirmation());
        // Sysadmin no relaja mutaciones sobre data del usuario.
        assert!(p.evaluate(&a, &ctx_sysadmin()).requires_confirmation());
    }
}
