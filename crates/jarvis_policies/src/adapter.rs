//! Puente con `ironclaw_safety`.
//!
//! `ironclaw_safety` se encarga de defensa anti prompt-injection,
//! validación de inputs, sanitizado de outputs y detección de fugas
//! de credenciales. Opera a nivel de "qué texto entra y sale del LLM".
//!
//! `jarvis_policies` opera a nivel de "qué acción concreta del SO se
//! permite ejecutar". Las dos capas son complementarias y se invocan
//! en orden: primero ironclaw_safety filtra el texto, después
//! jarvis_policies decide sobre la acción derivada.

use crate::{action::Action, action::ActionContext, decision::Decision, policy::PolicyEngine};

/// Combina ambas capas en una única fachada.
///
/// Diseñado para que el dispatcher de jarvis-os no tenga que saber qué
/// subsistema bloqueó la acción — el `Decision` resultante captura todo.
///
/// El `SafetyLayer` de ironclaw_safety tiene un modelo distinto (sanitiza
/// strings, detecta patrones), así que en este scaffold solo lo
/// almacenamos. El cableado real (cómo traducir un bloqueo de
/// `LeakDetector` a un `Decision::Deny`) se implementa en F1.2 cuando
/// el Linux MCP server invoque concretamente herramientas y se vea el
/// punto exacto donde traducir.
pub struct CombinedSafety<P: PolicyEngine> {
    pub safety: ironclaw_safety::SafetyLayer,
    pub policy: P,
}

impl<P: PolicyEngine> CombinedSafety<P> {
    pub fn new(safety: ironclaw_safety::SafetyLayer, policy: P) -> Self {
        Self { safety, policy }
    }

    /// Evalúa una acción concreta. Por ahora delega al `PolicyEngine`;
    /// en F1.2 se añadirá el cruce con `safety` para acciones cuyos args
    /// puedan contener material sensible (paths, comandos shell, etc.).
    pub fn evaluate(&self, action: &Action, context: &ActionContext) -> Decision {
        self.policy.evaluate(action, context)
    }

    /// Acceso de solo lectura al SafetyLayer subyacente para que callers
    /// hagan sanitización de strings/outputs por su cuenta cuando toque.
    pub fn safety_layer(&self) -> &ironclaw_safety::SafetyLayer {
        &self.safety
    }
}
