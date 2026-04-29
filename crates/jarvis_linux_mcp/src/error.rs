//! Tipos de error del Linux MCP server.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    /// Error proveniente de zbus (timeouts D-Bus, métodos inexistentes,
    /// permisos denegados a nivel D-Bus, etc.).
    #[error("D-Bus error: {0}")]
    Dbus(#[from] zbus::Error),

    /// La herramienta está declarada pero su implementación aún no existe.
    /// Útil durante el scaffold de F1.2.
    #[error("Tool not implemented: {tool}")]
    NotImplemented { tool: String },

    /// Args malformados (faltan campos, tipos incorrectos, valores fuera
    /// de rango).
    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    /// `jarvis_policies::Decision::Deny` se reflejó como error a nivel
    /// MCP. El campo describe la razón legible.
    #[error("Policy denied: {reason}")]
    PolicyDenied { reason: String },

    /// IO genérico (ficheros, sockets, etc.).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serde error (al parsear args o serializar output).
    #[error("Serde JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Cualquier cosa que no encaje en lo anterior; usar con cuidado.
    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;
