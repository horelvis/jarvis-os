//! Servidor MCP stdio (JSON-RPC 2.0 newline-delimited).
//!
//! Implementa los métodos mínimos para que IronClaw (que usa MCP protocol
//! `2024-11-05`) pueda registrarnos como server, listar tools y llamarlas.
//!
//! Métodos soportados:
//!   - `initialize` → handshake, devuelve protocolVersion + capabilities + serverInfo.
//!   - `notifications/initialized` → cliente notifica que terminó init (no requiere respuesta).
//!   - `tools/list` → lista de tools registradas con sus inputSchema.
//!   - `tools/call` → ejecuta una tool con sus argumentos.
//!   - `ping` → keepalive (devuelve `{}`).
//!
//! El protocolo MCP transporta JSON-RPC sobre stdio: cada mensaje es UNA línea
//! (terminada en `\n`), JSON serializado. Logs y diagnósticos van a stderr
//! para no contaminar el stream del protocolo.

use crate::tool::ToolRegistry;
use jarvis_policies::{Action, ActionContext, Decision, PolicyEngine};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Versión de protocolo MCP que hablamos. Coincide con la que IronClaw
/// usa en `src/tools/mcp/protocol.rs::PROTOCOL_VERSION`.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON-RPC error codes (subset estándar + reservados de MCP).
const PARSE_ERROR: i32 = -32700;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;
const INTERNAL_ERROR: i32 = -32603;

/// Petición JSON-RPC 2.0 entrante. `id` puede faltar para notifications.
#[derive(Debug, Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

/// Respuesta JSON-RPC 2.0 saliente.
#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl Response {
    fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// Estado compartido entre todas las requests del server: registry de tools
/// + motor de policies. Esto es el "guardian" — toda invocación de tool pasa
/// primero por `policy.evaluate` y solo se ejecuta si la decisión lo permite.
pub struct ServerState {
    pub registry: ToolRegistry,
    pub policy: Arc<dyn PolicyEngine>,
}

impl ServerState {
    pub fn new(registry: ToolRegistry, policy: Arc<dyn PolicyEngine>) -> Self {
        Self { registry, policy }
    }
}

/// Loop principal del servidor MCP por stdio.
///
/// Lee líneas de stdin, parsea cada una como Request, despacha por método,
/// escribe Response en stdout. Termina al EOF de stdin (cliente cerró).
///
/// **Guardian:** todas las invocaciones de `tools/call` consultan el
/// `PolicyEngine` antes de ejecutar la tool. Decision::Deny bloquea con
/// error MCP. Decision::Confirm logea y procede (en F2 será UI inline).
/// Decision::Allow procede normal. Esto materializa la capa 2 del spec
/// sec 6.1.
pub async fn run(state: ServerState) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = stdout;
    let mut line = String::new();

    eprintln!(
        "[jarvis-linux-mcp] MCP server stdio iniciado, protocolVersion={PROTOCOL_VERSION}, \
         tools={}, guardian=on",
        state.registry.len()
    );

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            // EOF: cliente cerró stdin. Salida limpia.
            eprintln!("[jarvis-linux-mcp] EOF en stdin, server terminando");
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Request>(trimmed) {
            Ok(req) => handle_request(req, &state).await,
            Err(e) => {
                eprintln!("[jarvis-linux-mcp] JSON parse error: {e}");
                Some(Response::err(
                    None,
                    PARSE_ERROR,
                    format!("invalid JSON: {e}"),
                ))
            }
        };

        // Notificaciones (sin id en la request) NO reciben respuesta.
        if let Some(resp) = response {
            let serialized = serde_json::to_string(&resp)?;
            writer.write_all(serialized.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
    }

    Ok(())
}

/// Solicita confirmación humana al HUD vía FIFOs.
///
/// Protocolo:
///   1. Escribe `prompt` (texto plano) al FIFO `/tmp/jarvis-confirm-request`
///      (lo crea si no existe). El daemon `eww` está leyendo ese FIFO via
///      `deflisten` y muestra el panel ámbar.
///   2. Espera respuesta en `/tmp/jarvis-confirm-response` (también FIFO).
///      El usuario aprieta `Super+Y` (escribe "approve") o `Super+N`
///      ("deny"). Hyprland tiene esos keybinds escribiendo al FIFO.
///   3. Limpia el FIFO de request escribiendo cadena vacía (cierra el panel).
///   4. Devuelve `Ok(true)` si approve, `Ok(false)` si deny, `Err` si timeout.
///
/// MVP simple: un solo confirm pendiente a la vez (mcp_server bloquea las
/// siguientes llamadas mientras espera). Si en F2+ hace falta concurrent,
/// se añade un ID por request y un mapa de canales.
async fn request_human_confirmation(
    prompt: &str,
    timeout_secs: u32,
) -> Result<bool, String> {
    use tokio::time::timeout;

    let req_path = "/tmp/jarvis-confirm-request";
    let resp_path = "/tmp/jarvis-confirm-response";

    // Asegura FIFOs creados (idempotente — mkfifo falla si ya existe, ignoramos).
    let _ = tokio::process::Command::new("mkfifo")
        .arg(req_path)
        .output()
        .await;
    let _ = tokio::process::Command::new("mkfifo")
        .arg(resp_path)
        .output()
        .await;

    // Escribe el prompt. Tiene que abrir + escribir + cerrar para que el
    // lector (eww deflisten) reciba EOF y procese el chunk.
    {
        let mut req = OpenOptions::new()
            .write(true)
            .open(req_path)
            .await
            .map_err(|e| format!("open request fifo: {e}"))?;
        req.write_all(prompt.as_bytes())
            .await
            .map_err(|e| format!("write request: {e}"))?;
        req.write_all(b"\n").await.ok();
        req.flush().await.ok();
    } // Drop = close → EOF al lector.

    // Espera respuesta con timeout.
    let read_resp = async {
        let mut resp = OpenOptions::new()
            .read(true)
            .open(resp_path)
            .await
            .map_err(|e| format!("open response fifo: {e}"))?;
        let mut buf = String::new();
        resp.read_to_string(&mut buf)
            .await
            .map_err(|e| format!("read response: {e}"))?;
        Ok::<String, String>(buf)
    };

    let response = timeout(Duration::from_secs(timeout_secs as u64), read_resp)
        .await
        .map_err(|_| format!("timeout {timeout_secs}s — default deny"))??;

    // Limpia el panel del HUD: escribe vacío al request FIFO para que
    // deflisten se cierre.
    if let Ok(mut req) = OpenOptions::new().write(true).open(req_path).await {
        let _ = req.write_all(b"\n").await;
    }

    let trimmed = response.trim().to_ascii_lowercase();
    Ok(matches!(trimmed.as_str(), "approve" | "yes" | "y" | "allow"))
}

/// Despacha una request al handler correspondiente. Devuelve `None` si
/// la petición es una notification (no debe responderse).
async fn handle_request(req: Request, state: &ServerState) -> Option<Response> {
    let id = req.id.clone();

    // Notificaciones: id ausente. Dispatch sin generar Response.
    let is_notification = id.is_none();

    let result = match req.method.as_str() {
        "initialize" => Ok(handle_initialize()),
        "notifications/initialized" => {
            // El cliente avisa que terminó init. Sin respuesta.
            return None;
        }
        "tools/list" => Ok(handle_tools_list(&state.registry)),
        "tools/call" => handle_tools_call(req.params, state).await,
        "ping" => Ok(json!({})),
        other => Err((METHOD_NOT_FOUND, format!("method not found: {other}"))),
    };

    if is_notification {
        return None;
    }

    Some(match result {
        Ok(value) => Response::ok(id, value),
        Err((code, msg)) => Response::err(id, code, msg),
    })
}

/// Handshake. Devuelve qué versión hablamos, qué capabilities tenemos
/// (solo `tools` por ahora; resources/prompts/logging vendrán en el futuro).
fn handle_initialize() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": "jarvis-linux-mcp",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

/// Lista todas las tools registradas con metadata MCP (name, description, inputSchema).
fn handle_tools_list(registry: &ToolRegistry) -> Value {
    let tools: Vec<Value> = registry
        .list()
        .iter()
        .map(|meta| {
            json!({
                "name": meta.name,
                "description": meta.description,
                "inputSchema": meta.args_schema,
                "annotations": {
                    // ReadSystem y NetworkOutbound son lecturas seguras según
                    // la matriz de DefaultPolicy. Las otras categorías quedan
                    // sin readOnlyHint para que el cliente sepa que pueden
                    // mutar estado.
                    "readOnlyHint": matches!(
                        meta.category,
                        jarvis_policies::ActionCategory::ReadSystem
                            | jarvis_policies::ActionCategory::ReadSensitive
                            | jarvis_policies::ActionCategory::NetworkOutbound
                    )
                }
            })
        })
        .collect();

    json!({ "tools": tools })
}

/// Invoca una tool por nombre con sus argumentos.
///
/// **Guardian-gated:** antes de invocar, consulta `state.policy.evaluate()`
/// con la categoría de la tool y un contexto restrictivo (defaults seguros).
/// Si Deny → `isError: true`, no ejecuta. Si Confirm → log + ejecuta
/// (TODO en F2: pausar y esperar confirmación inline del HUD). Si Allow → ejecuta.
async fn handle_tools_call(
    params: Option<Value>,
    state: &ServerState,
) -> Result<Value, (i32, String)> {
    let params = params.ok_or((INVALID_PARAMS, "missing params".to_string()))?;
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or((INVALID_PARAMS, "missing 'name' string".to_string()))?;
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

    let tool = state
        .registry
        .get(name)
        .ok_or((METHOD_NOT_FOUND, format!("tool not found: {name}")))?;

    // ── GUARDIAN ──
    // Construye Action a partir del metadata de la tool (category) +
    // ActionContext con defaults restrictivos. F2 enriquecerá el contexto
    // leyéndolo del estado del agente (sysadmin mode, last auth, presence).
    let metadata = tool.metadata().clone();
    let action = Action::new(metadata.name.clone(), metadata.category)
        .with_args(arguments.clone());
    let context = ActionContext::restrictive();

    let decision = state.policy.evaluate(&action, &context);

    match decision {
        Decision::Allow => {
            // Continúa al execute normal abajo.
            eprintln!(
                "[jarvis-linux-mcp] policy ALLOW: {} ({:?})",
                metadata.name, metadata.category
            );
        }
        Decision::Confirm { ref reason, timeout_secs } => {
            eprintln!(
                "[jarvis-linux-mcp] policy CONFIRM: {} ({:?}) reason={reason:?} timeout={timeout_secs}s — \
                 esperando respuesta del HUD",
                metadata.name, metadata.category
            );
            // F1.5 inline confirm: escribe el evento al FIFO que el HUD EWW
            // está leyendo, espera respuesta o timeout.
            let prompt = format!(
                "{}\n  category: {:?}\n  reason: {:?}\n  timeout: {}s",
                metadata.name, metadata.category, reason, timeout_secs
            );
            match request_human_confirmation(&prompt, timeout_secs).await {
                Ok(true) => {
                    eprintln!("[jarvis-linux-mcp] human APPROVED {}", metadata.name);
                }
                Ok(false) => {
                    eprintln!("[jarvis-linux-mcp] human DENIED {}", metadata.name);
                    return Ok(json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "user denied: {} blocked at HUD inline confirmation prompt",
                                metadata.name
                            )
                        }],
                        "isError": true,
                    }));
                }
                Err(e) => {
                    eprintln!("[jarvis-linux-mcp] confirm error: {e}");
                    return Ok(json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "policy confirmation timeout/error: {} (default = deny). reason: {e}",
                                metadata.name
                            )
                        }],
                        "isError": true,
                    }));
                }
            }
        }
        Decision::Deny { ref reason } => {
            eprintln!(
                "[jarvis-linux-mcp] policy DENY: {} ({:?}) reason={reason:?}",
                metadata.name, metadata.category
            );
            // Devuelve al agente como tool error (con isError=true), NO como
            // JSON-RPC error. Así el LLM lo ve como output que puede razonar.
            return Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!(
                        "policy denied: {} blocked by jarvis_policies (reason: {:?}). \
                         The action category {:?} is not permitted in the current context. \
                         Consider activating sysadmin mode or completing user authentication.",
                        metadata.name, reason, metadata.category
                    )
                }],
                "isError": true,
            }));
        }
    }

    match tool.invoke(&arguments).await {
        Ok(output) => {
            // MCP `tools/call` devuelve un array `content` con items tipados.
            // Para datos estructurados usamos type=text con JSON serializado;
            // el cliente (IronClaw) lo entiende.
            let body = serde_json::to_string_pretty(&output.data)
                .map_err(|e| (INTERNAL_ERROR, format!("serialize output: {e}")))?;

            let mut content = vec![json!({ "type": "text", "text": body })];
            if let Some(msg) = &output.user_message {
                content.push(json!({ "type": "text", "text": format!("note: {msg}") }));
            }

            Ok(json!({
                "content": content,
                "isError": false,
            }))
        }
        Err(e) => {
            // Errores de tool van como isError=true en el body, NO como
            // JSON-RPC error: el agente los ve como output válido y puede
            // razonar sobre ellos (reintentar, ajustar args, abandonar).
            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("tool error: {e}")
                }],
                "isError": true,
            }))
        }
    }
}

