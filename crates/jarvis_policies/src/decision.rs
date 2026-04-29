//! Decisiones del motor de políticas: ALLOW / CONFIRM / DENY.
//!
//! Los tres verbos están fijados por la spec sec 6.1 capa 2 ("Motor de
//! políticas: decisión ALLOW / CONFIRM / DENY por tool, args, hora,
//! contexto"). Cualquier extensión debe expresarse como combinación de
//! estos tres, no como un cuarto verbo.

use serde::{Deserialize, Serialize};

/// Resultado de evaluar una acción contra el motor de políticas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Decision {
    /// Permitido sin fricción. La acción se ejecuta inmediatamente.
    Allow,

    /// Requiere confirmación humana. El HUD muestra el panel inline
    /// (spec sec 5.4.5) con la acción propuesta y el motivo. Si el
    /// usuario no responde antes de `timeout_secs`, se trata como rechazo.
    Confirm {
        reason: ConfirmReason,
        timeout_secs: u32,
    },

    /// Denegado. El agente recibe el rechazo y debe reformular o desistir.
    /// El usuario ve el motivo en el HUD; auditoría registra la decisión.
    Deny { reason: DenyReason },
}

impl Decision {
    /// Atajo: ¿la acción puede ejecutarse sin más fricción?
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// ¿Necesita interacción humana antes de proceder?
    pub fn requires_confirmation(&self) -> bool {
        matches!(self, Self::Confirm { .. })
    }

    /// ¿Bloqueada definitivamente?
    pub fn is_deny(&self) -> bool {
        matches!(self, Self::Deny { .. })
    }
}

/// Motivos por los que una acción requiere confirmación.
///
/// El HUD usa esto para construir el texto del panel inline al usuario,
/// no para decisiones programáticas (la lógica ya está resuelta).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfirmReason {
    /// Acción destructiva: borrado, kill, restart, downgrade.
    Destructive,

    /// Alto impacto: afecta múltiples archivos o configuración crítica
    /// del sistema.
    HighImpact,

    /// Hora atípica (madrugada, fin de semana fuera de patrón habitual).
    UnusualHour,

    /// Lectura de datos sensibles del usuario.
    SensitiveData,

    /// Política custom de jarvis_policies extendido por el operador
    /// con su propia razón en texto libre.
    Custom { description: String },
}

/// Motivos por los que una acción es denegada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DenyReason {
    /// Política prohíbe esta categoría/herramienta en este contexto.
    PolicyForbids { rule: String },

    /// Acción Privileged sin autenticación reciente y sin sysadmin mode.
    UnauthorizedSysadmin,

    /// Fuera del horario permitido para esta acción.
    OutOfHours,

    /// El SafetyLayer de ironclaw_safety bloqueó (validación, prompt
    /// injection, leak). El campo describe qué subsistema disparó.
    SafetyLayerBlocked { subsystem: String, detail: String },

    /// La acción excede una cuota (rate limit, número de archivos, etc.).
    QuotaExceeded { resource: String },
}
