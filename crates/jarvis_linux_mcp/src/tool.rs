//! Trait `Tool` y `ToolRegistry`.
//!
//! Cada herramienta del Linux MCP server es un `impl Tool`. El servidor las
//! registra al arrancar y las expone vía protocolo MCP. Antes de invocar
//! una tool, el dispatcher (en F1.2.b) consulta `jarvis_policies` y aplica
//! la decisión ALLOW/CONFIRM/DENY.
//!
//! Las herramientas en sí NO hacen comprobación de política — su único
//! trabajo es ejecutar la operación. La política es responsabilidad del
//! caller (dispatcher), siguiendo el patrón de IronClaw
//! ("Everything goes through tools" en CLAUDE.md raíz).

use crate::error::Result;
use jarvis_policies::ActionCategory;
use serde::{Deserialize, Serialize};

/// Metadata estática de una herramienta. Se sirve al cliente MCP en
/// `tools/list` para que el agente sepa qué hay disponible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetadata {
    /// Identificador único, formato `domain.action` (p.ej. `systemd.unit_status`).
    pub name: String,

    /// Texto descriptivo para que el LLM entienda qué hace.
    /// Lo más explícito posible — esto es el "manual" que ve el modelo.
    pub description: String,

    /// Categoría de la acción según jarvis_policies. Determina la matriz
    /// de decisión ALLOW/CONFIRM/DENY que aplica el dispatcher.
    pub category: ActionCategory,

    /// JSON Schema (Draft 7 subset) describiendo los args esperados.
    /// El servidor MCP lo entrega al cliente para validación cliente-side.
    pub args_schema: serde_json::Value,
}

/// Output de una invocación de tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// Datos estructurados (para el agente / context del LLM).
    pub data: serde_json::Value,

    /// Mensaje breve que el HUD puede mostrar al usuario.
    /// Opcional — no toda tool necesita feedback visual.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
}

impl ToolOutput {
    pub fn new(data: serde_json::Value) -> Self {
        Self {
            data,
            user_message: None,
        }
    }

    pub fn with_user_message(mut self, msg: impl Into<String>) -> Self {
        self.user_message = Some(msg.into());
        self
    }
}

/// Trait que toda tool del Linux MCP server debe implementar.
///
/// Async para permitir IO de D-Bus, ficheros, etc. sin bloquear el runtime.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Metadata estática (no debería cambiar en runtime).
    fn metadata(&self) -> &ToolMetadata;

    /// Ejecuta la tool con los args provistos.
    ///
    /// Importante: NO comprueba política aquí. El dispatcher lo hace antes
    /// de llamar `invoke`. Los errores devueltos son de ejecución, no de
    /// autorización.
    async fn invoke(&self, args: &serde_json::Value) -> Result<ToolOutput>;
}

/// Registro simple de tools.
///
/// `Box<dyn Tool>` permite tools concretas heterogéneas en el mismo registro.
pub struct ToolRegistry {
    tools: std::collections::HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: std::collections::HashMap::new(),
        }
    }

    /// Inscribe una tool. Si ya hay otra con el mismo nombre, la sobrescribe
    /// y devuelve la anterior (útil para tests; en producción no debería
    /// pasar).
    pub fn register(&mut self, tool: Box<dyn Tool>) -> Option<Box<dyn Tool>> {
        let name = tool.metadata().name.clone();
        self.tools.insert(name, tool)
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn list(&self) -> Vec<&ToolMetadata> {
        self.tools.values().map(|t| t.metadata()).collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
