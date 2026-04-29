//! Acciones del agente: categorías y contexto de ejecución.
//!
//! Las seis categorías mapean 1:1 con las que el HUD muestra en su lateral
//! derecho (CAPABILITIES, spec sec 5.4.2). El motor de políticas decide
//! ALLOW/CONFIRM/DENY en función de la categoría y el contexto.

use serde::{Deserialize, Serialize};

/// Las seis categorías canónicas de acción del agente.
///
/// Coinciden con las del HUD (spec sec 5.4.2) y se reflejan en tiempo real
/// en su panel lateral derecho con el indicador de color de la política
/// vigente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionCategory {
    /// Lectura de estado del sistema (procesos, servicios systemd, journald, ps).
    /// Bajo riesgo: típicamente ALLOW.
    ReadSystem,

    /// Lectura de datos sensibles del usuario (claves SSH/GPG, history del browser,
    /// documentos privados). Riesgo medio: típicamente CONFIRM.
    ReadSensitive,

    /// Mutación de estado del sistema (start/stop services, instalar paquetes,
    /// modificar /etc, /opt). Riesgo medio-alto: CONFIRM (ALLOW si sysadmin).
    MutateSystem,

    /// Mutación de datos del usuario (crear, modificar, borrar archivos en $HOME).
    /// Riesgo medio: CONFIRM, especialmente si afecta múltiples archivos.
    MutateUserData,

    /// Operaciones de red salientes (HTTP, DNS, otros protocolos).
    /// Bajo riesgo en general: ALLOW (políticas finas a nivel de hostname/dominio
    /// llegan en F2+, vía allowlist).
    NetworkOutbound,

    /// Acciones que requieren elevación (polkit, root). Alto riesgo:
    /// DENY si el usuario no se autenticó recientemente, CONFIRM si sí.
    /// En modo sysadmin algunas pueden relajarse a ALLOW (spec sec 6.3).
    Privileged,
}

impl ActionCategory {
    /// Etiqueta legible (para el HUD y logs).
    pub fn label(&self) -> &'static str {
        match self {
            Self::ReadSystem => "read.system",
            Self::ReadSensitive => "read.sensitive",
            Self::MutateSystem => "mutate.system",
            Self::MutateUserData => "mutate.user_data",
            Self::NetworkOutbound => "network.outbound",
            Self::Privileged => "privileged",
        }
    }
}

/// Acción concreta que el agente quiere ejecutar.
///
/// Construida por el dispatcher antes de invocar la herramienta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    /// Identificador de la herramienta (`systemctl_status`, `file_write`, etc.).
    pub tool_name: String,

    /// Categoría a la que pertenece.
    pub category: ActionCategory,

    /// Argumentos de la invocación. Tipo libre — la política puede inspeccionar
    /// keys específicos para reglas finas (p.ej. `path` para detectar `/etc/*`).
    pub args: serde_json::Value,
}

impl Action {
    pub fn new(tool_name: impl Into<String>, category: ActionCategory) -> Self {
        Self {
            tool_name: tool_name.into(),
            category,
            args: serde_json::Value::Null,
        }
    }

    pub fn with_args(mut self, args: serde_json::Value) -> Self {
        self.args = args;
        self
    }
}

/// Contexto en el momento de evaluar la acción.
///
/// La política puede modular su decisión según el estado del sistema:
/// - hora del día (acciones destructivas a las 3am son sospechosas)
/// - modo sysadmin activo o no
/// - autenticación reciente (caché breve tras polkit)
/// - presencia del usuario (unattended = más restrictivo)
#[derive(Debug, Clone)]
pub struct ActionContext {
    /// Spec sec 6.3: modo sysadmin permite relajar algunas confirmaciones.
    pub sysadmin_mode_active: bool,

    /// Última autenticación polkit/biométrica fue hace < N minutos.
    /// Habilita acciones Privileged sin re-prompt inmediato.
    pub user_authenticated_recently: bool,

    /// Hora local en el momento de la decisión. Política puede penalizar
    /// horarios atípicos (madrugada, etc.) endureciendo a CONFIRM.
    pub now: chrono::DateTime<chrono::Local>,

    /// Si no hay un humano frente a la pantalla (detectado por inactividad
    /// de input + cámara apagada), las políticas se endurecen.
    pub is_unattended: bool,
}

impl ActionContext {
    /// Constructor con defaults conservadores (estado más restrictivo posible).
    pub fn restrictive() -> Self {
        Self {
            sysadmin_mode_active: false,
            user_authenticated_recently: false,
            now: chrono::Local::now(),
            is_unattended: true,
        }
    }
}
