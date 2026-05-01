//! Adapter polkit vía D-Bus.
//!
//! Polkit expone su API en `org.freedesktop.PolicyKit1.Authority`. La
//! interfaz relevante para v11 es `CheckAuthorization` que consulta si
//! un subject (proceso, sesión, user) tiene autorización para una
//! `action_id` declarada.
//!
//! Las action IDs que jarvis-os declarará están en spec sec 5.3.2:
//!   - `org.jarvis.privileged` (acciones privileged genéricas)
//!   - `org.jarvis.sysadmin.activate` (entrar/salir de sysadmin mode)
//!
//! En v11 NO instalamos esas actions todavía (eso es F4 con polkit + biometric).
//! Esta tool sirve para CONSULTAR cualquier action_id polkit existente del
//! sistema, útil para que el agente sepa antes de invocar pkexec/sudo si
//! hay capability.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use zbus::{Connection, Proxy, zvariant::Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCheck {
    pub action_id: String,
    pub is_authorized: bool,
    pub is_challenge: bool,
    pub details: std::collections::HashMap<String, String>,
}

pub struct PolkitAdapter {
    connection: Connection,
}

impl PolkitAdapter {
    pub async fn connect_system() -> Result<Self> {
        let connection = Connection::system().await?;
        Ok(Self { connection })
    }

    /// Consulta si la sesión actual tiene autorización para `action_id`.
    /// El subject se construye como "unix-session" del PID actual,
    /// equivalente a preguntar "para mí mismo, ahora".
    pub async fn check_authorization(&self, action_id: &str) -> Result<AuthCheck> {
        let proxy = Proxy::new(
            &self.connection,
            "org.freedesktop.PolicyKit1",
            "/org/freedesktop/PolicyKit1/Authority",
            "org.freedesktop.PolicyKit1.Authority",
        )
        .await?;

        // Subject: unix-session del PID actual.
        // Estructura: (subject_kind, subject_details).
        let pid = std::process::id();
        let mut subject_details: std::collections::HashMap<&str, Value> =
            std::collections::HashMap::new();
        subject_details.insert("pid", Value::U32(pid));
        subject_details.insert("start-time", Value::U64(0));

        let subject = ("unix-process", subject_details);

        let details: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();

        // Flags: 0 = sin allow_user_interaction, no preguntar; consulta pasiva.
        let flags: u32 = 0;
        let cancellation_id = "";

        let result: (bool, bool, std::collections::HashMap<String, String>) = proxy
            .call(
                "CheckAuthorization",
                &(subject, action_id, details, flags, cancellation_id),
            )
            .await?;

        Ok(AuthCheck {
            action_id: action_id.to_string(),
            is_authorized: result.0,
            is_challenge: result.1,
            details: result.2,
        })
    }
}
