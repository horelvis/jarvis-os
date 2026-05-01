# jarvis-os Local IPC Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reemplazar el wrapper `crates/jarvis_ui_bridge/` por un canal nativo `src/channels/local_ipc/` que expone un UNIX socket NDJSON dentro del propio core IronClaw, single-user implícito por permisos POSIX, sin auth/tokens/tunnel; eliminar el bridge y su unit systemd.

**Architecture:** Nuevo módulo bajo `src/channels/local_ipc/` que implementa el `Channel` trait. El `LocalIpcChannel` levanta un `UnixListener` en `$XDG_RUNTIME_DIR/ironclaw.sock`, acepta múltiples clientes locales concurrentes, suscribe cada writer-task al `SseManager` ya existente vía `subscribe_raw(Some(local_user_id), false)`, y empuja `Submission`s serializadas (`Message`, `Approval`, `Cancel`) al mismo `mpsc::Sender<IncomingMessage>` que ya consume el `ChannelManager`. El gateway HTTP/WS sigue intocado, sirviendo solo el caso remoto. Quickshell y voice daemon pasan a ser clientes directos del nuevo socket; el bridge desaparece.

**Tech Stack:** Rust (edition 2024), tokio (UnixListener, mpsc, broadcast, BufReader, signal), serde + serde_json (NDJSON wire), thiserror (errores), tempfile (tests). Spec base: `docs/superpowers/specs/2026-04-30-jarvis-os-local-ipc-design.md` (commit `68c56cd1`).

**Notas implementativas que el spec dejó imprecisas y se corrigen aquí:**

- El spec menciona `gate_manager: Arc<GateManager>` y `cancel_handle: CancelHandle`. Esos tipos NO existen como tales en el repo. Para Approval/Cancel usamos el sideband tipado `IncomingMessage::with_structured_submission(Submission)` (definido en `src/channels/channel.rs:251`, consumido por `src/agent/agent_loop.rs:1412`). El web channel usa el patrón legacy de serializar `Submission` como JSON en `content`; channels nuevos prefieren el sideband — evita un round-trip de serialización y mantiene el tipo a lo largo del path. `LocalIpcChannel` no necesita handles paralelos; `build_control_submission` adjunta la `Submission` al sideband del `IncomingMessage` y la inyecta por el mismo `MessageStream` que devuelve `start()`.
- El spec dice `local_user_id: UserId`. En el código actual `config.owner_id` es `String` (ver `src/main.rs:486` y todos los `.clone()` siguientes). Para no introducir conversiones nuevas, este plan usa `String` en el wire interno del módulo (igual que el resto de los canales) y pospone la migración a `UserId` newtype hasta que el resto del codebase lo haya hecho.
- `GatewayChannel::new()` construye su propio `Arc<SseManager>` internamente y NO permite inyección externa. El `agent.deps.sse_tx` se alimenta del `gw.state().sse`. Por lo tanto el `LocalIpcChannel` reusa el SseManager del gateway cuando éste está activo, y construye uno propio asignándolo a `sse_manager` cuando el gateway está desactivado (CLI-only mode). Ver Track F2 para el wire-up exacto.
- Eventos transport-only (`ipc_hello`, `error` de spec §5.1) no son `AppEvent` del core. Se modelan con `TransportEvent` y se multiplexan junto con `AppEvent` vía `WireMessage = App(AppEvent) | Transport(TransportEvent)` en el writer mpsc. Esto permite que el reader emita transport errors al cliente cuando una línea es malformada, en vez de solo loggearlos en servidor.

---

## File Structure

### Files created

| Path | Responsibility |
|---|---|
| `src/channels/local_ipc/mod.rs` | Module root: re-exports y `pub async fn create(...) -> Result<Option<LocalIpcChannel>, LocalIpcError>` |
| `src/channels/local_ipc/error.rs` | `LocalIpcError` (thiserror) |
| `src/channels/local_ipc/protocol.rs` | `ClientId` newtype, `ClientCommand` enum, `ApprovalAction` enum, `IpcErrorKind` enum wire-stable, `IpcHello` evento sintético, `TransportEvent` envelope |
| `src/channels/local_ipc/socket.rs` | `pub fn resolve_socket_path()`, `cleanup_orphan_socket()`, `run_listener()` |
| `src/channels/local_ipc/client.rs` | `ClientSession`, reader y writer tasks |
| `src/channels/local_ipc/control.rs` | `process_control_command()` — convierte `ClientCommand::{Approval, Cancel}` en `Submission` y la inyecta en el `MessageStream` |
| `src/channels/local_ipc/channel_impl.rs` | `impl Channel for LocalIpcChannel` |
| `tests/local_ipc_integration.rs` | 11 integration tests (sección 11.2 del spec) |

### Files modified

| Path | Change |
|---|---|
| `src/channels/mod.rs` | Add `pub mod local_ipc;` |
| `src/main.rs` (≈ line 757, después del bloque HTTP channel) | Insert `local_ipc::create(...).await` y `channels.add(...)` |
| `src/config/channels.rs` | Add `pub local_ipc: LocalIpcConfig` con envvars `IRONCLAW_LOCAL_SOCKET` y `IRONCLAW_LOCAL_IPC_BUFFER` |
| `Cargo.toml` (root workspace, line 2) | Eliminar `"crates/jarvis_ui_bridge"` de `members` |
| `arch/install.sh` | Eliminar las 3 referencias a `jarvis-ui-bridge` (build, install, enable) |
| `ui/jarvis-os/core/EventBus.qml` (line 30) | `socketPath` cambia de `/run/user/1000/jarvis-ui-bridge.sock` a `/run/user/1000/ironclaw.sock` |
| `.env.example` | Documentar `IRONCLAW_LOCAL_SOCKET` y `IRONCLAW_LOCAL_IPC_BUFFER` |
| `CLAUDE.md` | Reemplazar la línea de `jarvis_ui_bridge` en Project Structure por `src/channels/local_ipc/` |

### Files deleted

| Path | Reason |
|---|---|
| `crates/jarvis_ui_bridge/` (todo el directorio) | Superseded por `src/channels/local_ipc/` |
| `arch/systemd-user/jarvis-ui-bridge.service` | El daemon ya no existe |

### Files NOT touched (lista explícita para evitar borrado accidental)

- `crates/ironclaw_gateway/` — sigue vivo para acceso remoto.
- `src/channels/web/` — web channel intacto.
- `crates/jarvis_voice_daemon/` — verificar en Track G que no use el bridge; al inspeccionarse en pre-implementation no aparecen referencias a `gateway`/`bridge` en su `src/` (solo lee `~/.ironclaw/.env` para `JARVIS_VOICE_VARS`). Si una inspección posterior encuentra dependencia oculta, reapuntar al socket UNIX del core.
- `arch/systemd-user/jarvis-voice-daemon.service` — el voice daemon sigue como antes.
- `arch/systemd-user/jarvis-ui.service` — la unit de Quickshell solo cambia internamente vía EventBus.qml (mismo binario).

---

## Track A · Scaffolding del módulo (sin cambios funcionales)

### Task A1: Crear directorio + módulo vacío

**Files:**
- Create: `src/channels/local_ipc/mod.rs`
- Modify: `src/channels/mod.rs`

- [ ] **Step 1: Crear directorio**

```bash
mkdir -p src/channels/local_ipc
```

- [ ] **Step 2: `src/channels/local_ipc/mod.rs` mínimo**

```rust
//! Local UNIX-socket IPC channel.
//!
//! Reemplaza `crates/jarvis_ui_bridge/` exponiendo un UNIX socket NDJSON
//! directamente en el core IronClaw para que voice daemon y Quickshell UI
//! consuman eventos y manden comandos sin pasar por el gateway HTTP/WS.
//!
//! Ver `docs/superpowers/specs/2026-04-30-jarvis-os-local-ipc-design.md`.

mod channel_impl;
mod client;
mod control;
mod error;
mod protocol;
mod socket;

pub use channel_impl::LocalIpcChannel;
pub use error::LocalIpcError;
pub use socket::resolve_socket_path;
```

- [ ] **Step 3: Registrar el módulo en `src/channels/mod.rs`**

Añade junto a las demás declaraciones de submódulos (alfabéticamente entre `http` y `manager`):

```rust
pub mod local_ipc;
```

- [ ] **Step 4: Crear stubs vacíos para que el `mod.rs` compile**

Crea cada archivo con la herramienta `Write` (NO usar `echo` — viola la regla de CLAUDE.md sobre redirección manual de strings). Cada stub contiene literalmente:

| Path | Contenido único |
|---|---|
| `src/channels/local_ipc/protocol.rs` | `#![allow(dead_code)] // populated by Track B` |
| `src/channels/local_ipc/socket.rs` | `#![allow(dead_code)] // populated by Track C` |
| `src/channels/local_ipc/client.rs` | `#![allow(dead_code)] // populated by Track D` |
| `src/channels/local_ipc/control.rs` | `#![allow(dead_code)] // populated by Track D` |
| `src/channels/local_ipc/channel_impl.rs` | `#![allow(dead_code)] // populated by Track E` |

Los stubs no exportan nada todavía. El `mod.rs` que escribimos en Step 2 no compila aún porque hace `pub use` de tipos inexistentes — comentar esas líneas temporalmente:

Edita `src/channels/local_ipc/mod.rs` y reemplaza las tres `pub use` por:

```rust
// pub use channel_impl::LocalIpcChannel;  // populated by Task E2
// pub use error::LocalIpcError;           // populated by Task A2
// pub use socket::resolve_socket_path;    // populated by Task C1
```

- [ ] **Step 5: Verificar compilación**

```bash
cargo check
```

Expected: `Finished` sin warnings nuevos del módulo (solo los `dead_code` ya silenciados).

- [ ] **Step 6: Commit**

```bash
git add src/channels/local_ipc/ src/channels/mod.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): scaffold module skeleton

Empty submodule files + mod registration. No functional change yet;
each subsequent track populates one of the stubs (Task A2 fills error,
Track B fills protocol, etc.). Required so we can land each piece as a
self-contained TDD commit.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task A2: `error.rs` con `LocalIpcError`

**Files:**
- Modify: `src/channels/local_ipc/error.rs`
- Modify: `src/channels/local_ipc/mod.rs` (descomentar `pub use error::LocalIpcError`)

- [ ] **Step 1: Reemplazar el stub de `error.rs`**

```rust
use std::path::PathBuf;

use thiserror::Error;

use crate::error::ChannelError;

#[derive(Debug, Error)]
pub enum LocalIpcError {
    #[error("socket bind failed at {path}: {reason}")]
    BindFailed { path: PathBuf, reason: String },

    #[error("another IronClaw instance owns the socket at {path}")]
    SocketBusy { path: PathBuf },

    #[error("socket file at {path} could not be cleaned up: {reason}")]
    CleanupFailed { path: PathBuf, reason: String },

    #[error("unable to resolve local user id: {reason}")]
    LocalUserResolve { reason: String },

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl From<LocalIpcError> for ChannelError {
    fn from(e: LocalIpcError) -> Self {
        // ChannelError uses struct variants ({ name, reason }), not tuple
        // variants. There is no Io(io::Error) variant on ChannelError —
        // io errors collapse into StartupFailed at the channel boundary,
        // sanitized to a string. Reference: src/error.rs:115.
        ChannelError::StartupFailed {
            name: "local_ipc".into(),
            reason: e.to_string(),
        }
    }
}
```

- [ ] **Step 2: Confirmar que `ChannelError` declara las struct variants esperadas**

```bash
grep -n "pub enum ChannelError\|StartupFailed\|HealthCheckFailed" src/error.rs | head -5
```

Expected (verificado al escribir el plan): `pub enum ChannelError` en `src/error.rs:115` con `StartupFailed { name: String, reason: String }` (línea 117). NO inventar variantes nuevas. Si la signature difiere, actualizar el `From` impl en consecuencia.

- [ ] **Step 3: Descomentar la re-exportación en `mod.rs`**

Edita `src/channels/local_ipc/mod.rs` y reemplaza la línea comentada por:

```rust
pub use error::LocalIpcError;
```

- [ ] **Step 4: Verificar**

```bash
cargo check
```

Expected: ok, sin warnings nuevos.

- [ ] **Step 5: Commit**

```bash
git add src/channels/local_ipc/error.rs src/channels/local_ipc/mod.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): add LocalIpcError + From<ChannelError>

Five variants cover bind/busy/cleanup failures, primary-user resolution
failures, and generic IO. All variants collapse to ChannelError::
StartupFailed { name: "local_ipc", reason } at the Channel trait
boundary because ChannelError is struct-variant-only and has no Io
variant — io errors get sanitized to a string. Reference: src/error.rs:
115-141.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Track B · Tipos del wire protocol (TDD)

### Task B1: `ClientId` newtype

**Files:**
- Modify: `src/channels/local_ipc/protocol.rs`

Sigue el template canónico de `.claude/rules/types.md`.

- [ ] **Step 1: Escribir el test failing**

Reemplaza el stub de `protocol.rs` por:

```rust
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientIdError {
    #[error("client id must not be empty")]
    Empty,
    #[error("client id must be <= 64 chars (got {0})")]
    TooLong(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ClientId(String);

impl ClientId {
    fn validate(s: &str) -> Result<(), ClientIdError> {
        if s.is_empty() {
            return Err(ClientIdError::Empty);
        }
        let count = s.chars().count();
        if count > 64 {
            return Err(ClientIdError::TooLong(count));
        }
        Ok(())
    }

    pub fn new(raw: impl Into<String>) -> Result<Self, ClientIdError> {
        let s = raw.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ClientId {
    type Error = ClientIdError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::validate(&value)?;
        Ok(Self(value))
    }
}

impl AsRef<str> for ClientId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_rejects_empty() {
        assert!(matches!(ClientId::new(""), Err(ClientIdError::Empty)));
    }

    #[test]
    fn client_id_rejects_too_long() {
        let s = "a".repeat(65);
        assert!(matches!(
            ClientId::new(s),
            Err(ClientIdError::TooLong(65))
        ));
    }

    #[test]
    fn client_id_accepts_valid() {
        let id = ClientId::new("ipc-42").expect("valid id");
        assert_eq!(id.as_str(), "ipc-42");
    }

    #[test]
    fn client_id_serde_roundtrip() {
        let id = ClientId::new("c1").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"c1\"");
        let back: ClientId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn client_id_serde_rejects_invalid() {
        let res: Result<ClientId, _> = serde_json::from_str("\"\"");
        assert!(res.is_err());
    }
}
```

- [ ] **Step 2: Run tests — primero verificar que compilan y pasan**

```bash
cargo test -p ironclaw --lib channels::local_ipc::protocol::tests
```

Expected: 5 passed.

- [ ] **Step 3: Commit**

```bash
git add src/channels/local_ipc/protocol.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): add ClientId newtype with validation

Per types.md template: shared validate(), try_from for wire path,
explicit ::new for construction, no infallible From<String>. Cap is
64 chars (chars().count(), not bytes — matches the error message).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task B2: `IpcErrorKind` enum wire-stable

**Files:**
- Modify: `src/channels/local_ipc/protocol.rs`

- [ ] **Step 1: Append al final de `protocol.rs`**

```rust
/// Wire-stable error kinds emitted to the client as a synthetic `error`
/// transport event. Snake_case on the wire (rule: types.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcErrorKind {
    CommandInvalid,
    CommandTooLarge,
    RateLimit,
    InternalError,
}

#[cfg(test)]
mod kind_tests {
    use super::IpcErrorKind;

    #[test]
    fn kind_serializes_snake_case() {
        let s = serde_json::to_string(&IpcErrorKind::CommandInvalid).unwrap();
        assert_eq!(s, "\"command_invalid\"");
        let s = serde_json::to_string(&IpcErrorKind::CommandTooLarge).unwrap();
        assert_eq!(s, "\"command_too_large\"");
        let s = serde_json::to_string(&IpcErrorKind::RateLimit).unwrap();
        assert_eq!(s, "\"rate_limit\"");
        let s = serde_json::to_string(&IpcErrorKind::InternalError).unwrap();
        assert_eq!(s, "\"internal_error\"");
    }

    #[test]
    fn kind_deserializes_snake_case() {
        let k: IpcErrorKind = serde_json::from_str("\"command_invalid\"").unwrap();
        assert_eq!(k, IpcErrorKind::CommandInvalid);
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p ironclaw --lib channels::local_ipc::protocol::kind_tests
```

Expected: 2 passed.

- [ ] **Step 3: Commit**

```bash
git add src/channels/local_ipc/protocol.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): add IpcErrorKind wire-stable enum

Four variants — command_invalid, command_too_large, rate_limit,
internal_error — locked to snake_case on the wire so adding a fifth
variant tomorrow can't drift the serialization.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task B3: `ApprovalAction` + `ClientCommand` enums

**Files:**
- Modify: `src/channels/local_ipc/protocol.rs`

- [ ] **Step 1: Append a `protocol.rs`**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalAction {
    Approve,
    Deny,
}

/// Commands the client may send to the server. Wire-stable.
///
/// `thread_id` and `step_id` are kept as `String` here (not the engine's
/// `ThreadId` newtype) because the wire payload is untrusted and the
/// engine-facing constructors will validate at the call site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommand {
    Message {
        content: String,
        #[serde(default)]
        thread_id: Option<String>,
    },
    Approval {
        request_id: String,
        action: ApprovalAction,
    },
    Cancel {
        #[serde(default)]
        step_id: Option<String>,
    },
    Ping,
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn message_roundtrip() {
        let raw = r#"{"type":"message","content":"hola","thread_id":"t1"}"#;
        let cmd: ClientCommand = serde_json::from_str(raw).unwrap();
        assert_eq!(
            cmd,
            ClientCommand::Message {
                content: "hola".into(),
                thread_id: Some("t1".into()),
            }
        );
    }

    #[test]
    fn message_thread_id_optional() {
        let raw = r#"{"type":"message","content":"hi"}"#;
        let cmd: ClientCommand = serde_json::from_str(raw).unwrap();
        assert_eq!(
            cmd,
            ClientCommand::Message {
                content: "hi".into(),
                thread_id: None,
            }
        );
    }

    #[test]
    fn approval_roundtrip() {
        let raw = r#"{"type":"approval","request_id":"r1","action":"approve"}"#;
        let cmd: ClientCommand = serde_json::from_str(raw).unwrap();
        assert_eq!(
            cmd,
            ClientCommand::Approval {
                request_id: "r1".into(),
                action: ApprovalAction::Approve,
            }
        );
    }

    #[test]
    fn cancel_roundtrip() {
        let raw = r#"{"type":"cancel"}"#;
        let cmd: ClientCommand = serde_json::from_str(raw).unwrap();
        assert_eq!(cmd, ClientCommand::Cancel { step_id: None });
    }

    #[test]
    fn ping_roundtrip() {
        let raw = r#"{"type":"ping"}"#;
        let cmd: ClientCommand = serde_json::from_str(raw).unwrap();
        assert_eq!(cmd, ClientCommand::Ping);
    }

    #[test]
    fn unknown_type_rejected() {
        let raw = r#"{"type":"frobnicate"}"#;
        let res: Result<ClientCommand, _> = serde_json::from_str(raw);
        assert!(res.is_err());
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p ironclaw --lib channels::local_ipc::protocol::command_tests
```

Expected: 6 passed.

- [ ] **Step 3: Commit**

```bash
git add src/channels/local_ipc/protocol.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): add ClientCommand + ApprovalAction wire enums

#[serde(tag = "type", rename_all = "snake_case")]. Variants:
message{content,thread_id?}, approval{request_id,action},
cancel{step_id?}, ping. No user_id field — server resolves it at
startup; client cannot impersonate.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task B4: `IpcHello` + `TransportEvent` envelope

**Files:**
- Modify: `src/channels/local_ipc/protocol.rs`

`AppEvent` se serializa con `#[serde(tag = "type")]` ya. Necesitamos dos eventos sintéticos del transporte (`ipc_hello` y `error`) que no son `AppEvent` del core. Los modelamos como un enum aparte serializable al mismo formato `{"type": "...", ...}`.

- [ ] **Step 1: Append a `protocol.rs`**

```rust
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcHello {
    pub protocol_version: u32,
    pub local_user_id: String,
}

/// Envelope for transport-only synthetic events that don't originate
/// from the engine `AppEvent` log. Serialized with the same `{"type":
/// "...", ...}` shape so the QML / voice-daemon parser only needs one
/// case branch.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransportEvent {
    IpcHello(IpcHello),
    Error { kind: IpcErrorKind, detail: String },
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    #[test]
    fn hello_serializes_with_type_tag() {
        let ev = TransportEvent::IpcHello(IpcHello {
            protocol_version: 1,
            local_user_id: "owner".into(),
        });
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"type\":\"ipc_hello\""));
        assert!(s.contains("\"protocol_version\":1"));
        assert!(s.contains("\"local_user_id\":\"owner\""));
    }

    #[test]
    fn error_serializes_snake_case_kind() {
        let ev = TransportEvent::Error {
            kind: IpcErrorKind::CommandInvalid,
            detail: "bad json".into(),
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"type\":\"error\""));
        assert!(s.contains("\"kind\":\"command_invalid\""));
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p ironclaw --lib channels::local_ipc::protocol::transport_tests
```

Expected: 2 passed.

- [ ] **Step 3: Commit**

```bash
git add src/channels/local_ipc/protocol.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): add IpcHello + TransportEvent envelope

PROTOCOL_VERSION constant starts at 1. TransportEvent serializes with
the same {type,...} shape as AppEvent so QML/voice-daemon parsers only
need one branch (ipc_hello | error | <AppEvent variants>).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Track C · Resolución del path + cleanup

### Task C1: `resolve_socket_path()`

**Files:**
- Modify: `src/channels/local_ipc/socket.rs`
- Modify: `src/channels/local_ipc/mod.rs` (descomentar `pub use socket::resolve_socket_path`)

- [ ] **Step 1: Reemplazar el stub de `socket.rs`**

```rust
use std::path::PathBuf;

const ENV_OVERRIDE: &str = "IRONCLAW_LOCAL_SOCKET";
const DISABLED_TOKEN: &str = "disabled";
const FALLBACK_BASENAME: &str = "ironclaw.sock";

/// Resolved outcome for the socket path lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketResolution {
    /// IPC explicitly disabled by env (`IRONCLAW_LOCAL_SOCKET=disabled`).
    Disabled,
    /// Use this path.
    Path(PathBuf),
}

/// Resolve the socket path according to the documented order:
/// 1. `IRONCLAW_LOCAL_SOCKET` env var (verbatim, or `disabled`).
/// 2. `$XDG_RUNTIME_DIR/ironclaw.sock`.
/// 3. `$HOME/.ironclaw/ironclaw.sock`.
///
/// Pure function — no filesystem side effects (does NOT create directories).
/// Errors propagate from the env lookups only.
pub fn resolve_socket_path() -> SocketResolution {
    if let Ok(val) = std::env::var(ENV_OVERRIDE) {
        if val == DISABLED_TOKEN {
            return SocketResolution::Disabled;
        }
        if !val.is_empty() {
            return SocketResolution::Path(PathBuf::from(val));
        }
    }
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR")
        && !xdg.is_empty()
    {
        return SocketResolution::Path(PathBuf::from(xdg).join(FALLBACK_BASENAME));
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    SocketResolution::Path(
        PathBuf::from(home)
            .join(".ironclaw")
            .join(FALLBACK_BASENAME),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Env mutations are process-global; serialize them across tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved: Vec<_> = vars
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            // SAFETY: env access is single-threaded under ENV_LOCK.
            unsafe {
                match v {
                    Some(value) => std::env::set_var(k, value),
                    None => std::env::remove_var(k),
                }
            }
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        for (k, v) in saved {
            unsafe {
                match v {
                    Some(value) => std::env::set_var(k, value),
                    None => std::env::remove_var(k),
                }
            }
        }
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    #[test]
    fn env_override_explicit_path() {
        with_env(
            &[
                ("IRONCLAW_LOCAL_SOCKET", Some("/tmp/jarvis-test.sock")),
                ("XDG_RUNTIME_DIR", Some("/run/user/1000")),
            ],
            || {
                assert_eq!(
                    resolve_socket_path(),
                    SocketResolution::Path(PathBuf::from("/tmp/jarvis-test.sock"))
                );
            },
        );
    }

    #[test]
    fn env_override_disabled() {
        with_env(
            &[("IRONCLAW_LOCAL_SOCKET", Some("disabled"))],
            || {
                assert_eq!(resolve_socket_path(), SocketResolution::Disabled);
            },
        );
    }

    #[test]
    fn xdg_runtime_dir_fallback() {
        with_env(
            &[
                ("IRONCLAW_LOCAL_SOCKET", None),
                ("XDG_RUNTIME_DIR", Some("/run/user/1000")),
            ],
            || {
                assert_eq!(
                    resolve_socket_path(),
                    SocketResolution::Path(PathBuf::from("/run/user/1000/ironclaw.sock"))
                );
            },
        );
    }

    #[test]
    fn home_fallback_when_no_xdg() {
        with_env(
            &[
                ("IRONCLAW_LOCAL_SOCKET", None),
                ("XDG_RUNTIME_DIR", None),
                ("HOME", Some("/home/jarvis")),
            ],
            || {
                assert_eq!(
                    resolve_socket_path(),
                    SocketResolution::Path(PathBuf::from(
                        "/home/jarvis/.ironclaw/ironclaw.sock"
                    ))
                );
            },
        );
    }
}
```

- [ ] **Step 2: Descomentar la re-exportación en `mod.rs`**

```rust
pub use socket::resolve_socket_path;
```

Y añade también:

```rust
pub use socket::SocketResolution;
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p ironclaw --lib channels::local_ipc::socket::tests
```

Expected: 4 passed.

- [ ] **Step 4: Commit**

```bash
git add src/channels/local_ipc/socket.rs src/channels/local_ipc/mod.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): add resolve_socket_path with env > XDG > HOME order

Pure function returning a SocketResolution (Disabled | Path). Tests
serialize env mutations through a Mutex so the four scenarios (env
override, disabled token, XDG fallback, HOME fallback) run safely
under cargo test's parallel default.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task C2: `cleanup_orphan_socket()`

**Files:**
- Modify: `src/channels/local_ipc/socket.rs`

- [ ] **Step 1: Append a `socket.rs`**

```rust
use std::time::Duration;
use tokio::net::UnixStream;
use tokio::time::timeout;

use crate::channels::local_ipc::error::LocalIpcError;

/// Inspect an existing socket file and remove it if no live IronClaw
/// instance is listening. Returns `Ok(true)` if the orphan was cleaned
/// (or never existed), `Ok(false)` if a live instance currently owns
/// it, and `Err` on cleanup failure.
pub async fn cleanup_orphan_socket(path: &std::path::Path) -> Result<bool, LocalIpcError> {
    if !tokio::fs::try_exists(path).await? {
        return Ok(true);
    }
    // Try to connect. A live owner replies; an orphan errors out.
    match timeout(Duration::from_millis(100), UnixStream::connect(path)).await {
        Ok(Ok(_)) => Ok(false), // live owner — caller must abort startup
        Ok(Err(_)) | Err(_) => {
            tokio::fs::remove_file(path).await.map_err(|e| {
                LocalIpcError::CleanupFailed {
                    path: path.to_path_buf(),
                    reason: e.to_string(),
                }
            })?;
            Ok(true)
        }
    }
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn missing_path_is_a_clean_orphan() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.sock");
        assert!(cleanup_orphan_socket(&path).await.unwrap());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn dead_socket_file_gets_unlinked() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dead.sock");
        // Create a regular file at the socket path to simulate an orphan
        // left over from a crashed process.
        tokio::fs::write(&path, b"orphan").await.unwrap();
        assert!(path.exists());
        assert!(cleanup_orphan_socket(&path).await.unwrap());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn live_listener_blocks_cleanup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("live.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        // Spawn an accept loop so connect() actually succeeds.
        let _accept_task = tokio::spawn(async move {
            let _ = listener.accept().await;
        });
        // Give the listener a moment to be ready.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let result = cleanup_orphan_socket(&path).await.unwrap();
        assert!(!result, "live owner must block cleanup");
        assert!(path.exists(), "live socket must NOT be unlinked");
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p ironclaw --lib channels::local_ipc::socket::cleanup_tests
```

Expected: 3 passed.

- [ ] **Step 3: Commit**

```bash
git add src/channels/local_ipc/socket.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): add cleanup_orphan_socket with connect-test

100ms timeout connect probe distinguishes a live IronClaw owner from
an orphan socket file left over from a crash. Returns Ok(true) for
"safe to bind", Ok(false) for "another instance owns it".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Track D · Sesión per-cliente + control commands

### Task D1: `control.rs` — convertir comandos a `Submission`

**Files:**
- Modify: `src/channels/local_ipc/control.rs`

- [ ] **Step 1: Reemplazar el stub**

Diseño: usar el sideband `IncomingMessage::with_structured_submission(Submission)` en vez de serializar a JSON en `content`. Es el path tipado (verificado en `src/channels/channel.rs:251-257` y consumido por `src/agent/agent_loop.rs:1412`). El web channel usa el patrón legacy JSON-en-content por razones históricas; channels nuevos deben preferir el sideband — evita una serialización + un parse y mantiene el `Submission` tipado a lo largo del path.

```rust
use uuid::Uuid;

use crate::agent::submission::Submission;
use crate::channels::IncomingMessage;
use crate::channels::local_ipc::protocol::{ApprovalAction, ClientCommand, ClientId};

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("invalid request_id (expected UUID): {0}")]
    InvalidRequestId(String),
}

/// Translate a non-Message ClientCommand into the IncomingMessage the
/// agent loop expects. Uses the typed `with_structured_submission`
/// sideband instead of serializing JSON into `content` (cleaner than
/// the legacy web-channel pattern; agent_loop reads the sideband first
/// at src/agent/agent_loop.rs:1412 before any content parsing).
///
/// Returns `Ok(None)` for `Ping` (no submission — handled at the writer
/// task as a wire-only no-op) and for `Message` (built directly in the
/// reader task with the user's text as `content`).
pub fn build_control_submission(
    cmd: &ClientCommand,
    user_id: &str,
    client_id: &ClientId,
) -> Result<Option<IncomingMessage>, ControlError> {
    let submission = match cmd {
        ClientCommand::Approval { request_id, action } => {
            let rid = Uuid::parse_str(request_id)
                .map_err(|_| ControlError::InvalidRequestId(request_id.clone()))?;
            Submission::ExecApproval {
                request_id: rid,
                approved: matches!(action, ApprovalAction::Approve),
                always: false,
            }
        }
        ClientCommand::Cancel { step_id: _ } => {
            // step_id is informational; engine v2 Interrupt cancels the
            // current turn for this user_id regardless.
            Submission::Interrupt
        }
        ClientCommand::Message { .. } | ClientCommand::Ping => return Ok(None),
    };
    let metadata = serde_json::json!({ "client_id": client_id.as_str() });
    Ok(Some(
        IncomingMessage::new("local_ipc", user_id, "")
            .with_structured_submission(submission)
            .with_metadata(metadata),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cid() -> ClientId {
        ClientId::new("c1").unwrap()
    }

    #[test]
    fn approval_builds_exec_approval_sideband() {
        let req_id = Uuid::new_v4();
        let cmd = ClientCommand::Approval {
            request_id: req_id.to_string(),
            action: ApprovalAction::Approve,
        };
        let msg = build_control_submission(&cmd, "owner", &cid())
            .unwrap()
            .expect("approval must produce a submission");
        assert_eq!(msg.channel, "local_ipc");
        assert_eq!(msg.content, "", "sideband used; content stays empty");
        match msg.structured_submission.expect("sideband set") {
            Submission::ExecApproval {
                request_id,
                approved,
                always,
            } => {
                assert_eq!(request_id, req_id);
                assert!(approved);
                assert!(!always);
            }
            other => panic!("expected ExecApproval, got {other:?}"),
        }
    }

    #[test]
    fn approval_deny_sideband_marks_not_approved() {
        let cmd = ClientCommand::Approval {
            request_id: Uuid::new_v4().to_string(),
            action: ApprovalAction::Deny,
        };
        let msg = build_control_submission(&cmd, "owner", &cid())
            .unwrap()
            .expect("must produce");
        assert!(matches!(
            msg.structured_submission.unwrap(),
            Submission::ExecApproval { approved: false, .. }
        ));
    }

    #[test]
    fn invalid_request_id_rejected() {
        let cmd = ClientCommand::Approval {
            request_id: "not-a-uuid".into(),
            action: ApprovalAction::Approve,
        };
        let res = build_control_submission(&cmd, "owner", &cid());
        assert!(matches!(res, Err(ControlError::InvalidRequestId(_))));
    }

    #[test]
    fn cancel_builds_interrupt_sideband() {
        let cmd = ClientCommand::Cancel { step_id: None };
        let msg = build_control_submission(&cmd, "owner", &cid())
            .unwrap()
            .unwrap();
        assert!(matches!(
            msg.structured_submission.unwrap(),
            Submission::Interrupt
        ));
    }

    #[test]
    fn ping_yields_no_submission() {
        let res = build_control_submission(&ClientCommand::Ping, "owner", &cid()).unwrap();
        assert!(res.is_none());
    }

    #[test]
    fn message_yields_no_submission() {
        let cmd = ClientCommand::Message {
            content: "hi".into(),
            thread_id: None,
        };
        let res = build_control_submission(&cmd, "owner", &cid()).unwrap();
        assert!(res.is_none(), "Message routing happens in the reader task");
    }

    #[test]
    fn metadata_carries_client_id() {
        let cmd = ClientCommand::Cancel { step_id: None };
        let msg = build_control_submission(&cmd, "owner", &cid())
            .unwrap()
            .unwrap();
        // IncomingMessage.metadata is serde_json::Value (Null by default,
        // not Option). Verified at src/channels/channel.rs:101.
        assert_eq!(msg.metadata["client_id"], "c1");
    }
}
```

- [ ] **Step 2: Confirmación de signatures (verificadas al escribir el plan)**

```bash
grep -n "pub fn new\|pub fn with_metadata\|pub fn with_structured_submission\|pub metadata:" src/channels/channel.rs | head -10
```

Expected:
- `IncomingMessage::new(channel: impl Into<String>, user_id: impl Into<String>, content: impl Into<String>)` — `src/channels/channel.rs:133`.
- `with_metadata(metadata: serde_json::Value)` — `src/channels/channel.rs:239`.
- `with_structured_submission(submission: Submission)` — `src/channels/channel.rs:251`.
- `pub metadata: serde_json::Value` (NO `Option<Value>` — default `Value::Null`) — `src/channels/channel.rs:101`.

- [ ] **Step 3: Run tests**

```bash
cargo test -p ironclaw --lib channels::local_ipc::control::tests
```

Expected: 7 passed.

- [ ] **Step 4: Commit**

```bash
git add src/channels/local_ipc/control.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): add build_control_submission via typed sideband

Approval/Cancel build a Submission and attach it to IncomingMessage via
with_structured_submission (the typed sideband at channel.rs:251) — NOT
serialized as JSON in content. The agent loop reads the sideband first
at agent_loop.rs:1412, before any content parsing, so this stays tipo-
seguro end-to-end. Metadata carries client_id for per-client routing on
respond(). Cleaner than the legacy web-channel JSON-in-content pattern.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task D2: `client.rs` — `ClientSession` reader/writer

**Files:**
- Modify: `src/channels/local_ipc/client.rs`

- [ ] **Step 1: Reemplazar el stub completo**

Diseño: un único enum `WireMessage = App(AppEvent) | Transport(TransportEvent)` cruza el writer mpsc, así los `transport-error` events del spec §5.1 efectivamente llegan al cliente (no se pierden en un debug log). El writer multiplexa una sola serialización + write con un solo `match`.

```rust
use std::sync::Arc;

use futures_util::StreamExt;
use ironclaw_common::event::AppEvent;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, warn};

use crate::channels::IncomingMessage;
use crate::channels::local_ipc::control::{build_control_submission, ControlError};
use crate::channels::local_ipc::protocol::{
    ClientCommand, ClientId, IpcErrorKind, IpcHello, TransportEvent, PROTOCOL_VERSION,
};
use crate::channels::web::platform::sse::SseManager;

const MAX_LINE_BYTES: usize = 64 * 1024;
pub const DEFAULT_WRITER_BUFFER: usize = 256;

/// Single envelope for everything the writer task emits to the client.
/// Unifying AppEvent + TransportEvent lets the writer have one serialize
/// + write path and a single mpsc, and lets the reader push transport
/// errors (malformed line, oversized command) to the same writer.
#[derive(Debug, Clone)]
pub enum WireMessage {
    App(AppEvent),
    Transport(TransportEvent),
}

/// Owner half of a per-client session. Held by `LocalIpcChannel` so it
/// can route `respond()` / `send_status()` to the right writer.
#[derive(Debug)]
pub struct ClientHandle {
    pub client_id: ClientId,
    pub tx: mpsc::Sender<WireMessage>,
}

/// Run a fresh client session. Spawns reader + writer tasks and returns
/// the `ClientHandle` so the caller can register it before either task
/// ever yields. The session ends when the client closes the socket; both
/// tasks then terminate and the caller is expected to remove the handle.
pub async fn spawn_session(
    stream: UnixStream,
    client_id: ClientId,
    user_id: String,
    sse: Arc<SseManager>,
    inject_tx: mpsc::Sender<IncomingMessage>,
    writer_buffer: usize,
) -> ClientHandle {
    let (read_half, write_half) = stream.into_split();
    let (event_tx, event_rx) = mpsc::channel::<WireMessage>(writer_buffer);

    let writer_user_id = user_id.clone();
    let writer_sse = Arc::clone(&sse);
    let writer_id = client_id.clone();
    let writer_tx_for_reader = event_tx.clone();

    tokio::spawn(async move {
        run_writer_task(
            write_half,
            event_rx,
            writer_id,
            writer_user_id,
            writer_sse,
        )
        .await;
    });

    let reader_id = client_id.clone();
    tokio::spawn(async move {
        run_reader_task(read_half, reader_id, user_id, inject_tx, writer_tx_for_reader).await;
        // When the reader exits (client closed the socket), our extra
        // sender drops along with this future and the writer task
        // observes channel close (if it was the last sender) and ends.
    });

    ClientHandle {
        client_id,
        tx: event_tx,
    }
}

async fn run_reader_task(
    read_half: tokio::net::unix::OwnedReadHalf,
    client_id: ClientId,
    user_id: String,
    inject_tx: mpsc::Sender<IncomingMessage>,
    error_event_tx: mpsc::Sender<WireMessage>,
) {
    let mut buf = BufReader::new(read_half);
    let mut line = String::new();
    loop {
        line.clear();
        let read = buf.read_line(&mut line).await;
        match read {
            Ok(0) => {
                debug!(client = %client_id, "ipc client closed");
                break;
            }
            Ok(n) if n > MAX_LINE_BYTES => {
                emit_transport_error(
                    &error_event_tx,
                    IpcErrorKind::CommandTooLarge,
                    "command line exceeded 64 KiB",
                )
                .await;
                continue;
            }
            Ok(_) => {}
            Err(e) => {
                warn!(client = %client_id, error = %e, "ipc client read error");
                break;
            }
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.is_empty() {
            continue; // silent-ok: empty line, continue session
        }
        let cmd: ClientCommand = match serde_json::from_str(trimmed) {
            Ok(c) => c,
            Err(e) => {
                warn!(client = %client_id, error = %e, "ipc command parse failed");
                emit_transport_error(
                    &error_event_tx,
                    IpcErrorKind::CommandInvalid,
                    "could not parse command",
                )
                .await;
                continue; // silent-ok: malformed line, continue session
            }
        };
        if let Err(e) = dispatch_command(cmd, &user_id, &client_id, &inject_tx).await {
            warn!(client = %client_id, error = %e, "ipc command dispatch failed");
            emit_transport_error(
                &error_event_tx,
                IpcErrorKind::CommandInvalid,
                "command dispatch failed",
            )
            .await;
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum DispatchError {
    #[error("control error: {0}")]
    Control(#[from] ControlError),
    #[error("inject channel closed")]
    InjectClosed,
}

async fn dispatch_command(
    cmd: ClientCommand,
    user_id: &str,
    client_id: &ClientId,
    inject_tx: &mpsc::Sender<IncomingMessage>,
) -> Result<(), DispatchError> {
    match cmd {
        ClientCommand::Message { content, thread_id } => {
            let metadata = serde_json::json!({
                "client_id": client_id.as_str(),
                "thread_id": thread_id,
            });
            let msg = IncomingMessage::new("local_ipc", user_id, content)
                .with_metadata(metadata);
            inject_tx
                .send(msg)
                .await
                .map_err(|_| DispatchError::InjectClosed)?;
            Ok(())
        }
        ClientCommand::Ping => Ok(()),
        ClientCommand::Approval { .. } | ClientCommand::Cancel { .. } => {
            if let Some(msg) = build_control_submission(&cmd, user_id, client_id)? {
                inject_tx
                    .send(msg)
                    .await
                    .map_err(|_| DispatchError::InjectClosed)?;
            }
            Ok(())
        }
    }
}

async fn run_writer_task(
    mut write_half: tokio::net::unix::OwnedWriteHalf,
    mut event_rx: mpsc::Receiver<WireMessage>,
    client_id: ClientId,
    user_id: String,
    sse: Arc<SseManager>,
) {
    // Emit the synthetic ipc_hello before anything else.
    let hello = WireMessage::Transport(TransportEvent::IpcHello(IpcHello {
        protocol_version: PROTOCOL_VERSION,
        local_user_id: user_id.clone(),
    }));
    if !write_wire(&mut write_half, &hello).await {
        return;
    }

    // Subscribe to the SseManager scoped to this user. None means we hit
    // the global max_connections; the writer keeps serving direct
    // respond()/send_status traffic on event_rx only.
    let mut sse_stream = sse.subscribe_raw(Some(user_id), false);

    loop {
        let wire_opt: Option<WireMessage> = tokio::select! {
            biased;
            Some(msg) = event_rx.recv() => Some(msg),
            sse_event = async {
                match sse_stream.as_mut() {
                    Some(s) => s.next().await,
                    None => {
                        // Park forever — fall through to event_rx only.
                        std::future::pending::<()>().await;
                        None
                    }
                }
            } => sse_event.map(WireMessage::App),
            else => None,
        };
        let Some(wire) = wire_opt else { break; };
        if !write_wire(&mut write_half, &wire).await {
            break;
        }
    }
    debug!(client = %client_id, "ipc writer terminated");
}

async fn write_wire(
    write_half: &mut tokio::net::unix::OwnedWriteHalf,
    msg: &WireMessage,
) -> bool {
    let bytes_result = match msg {
        WireMessage::App(ev) => serde_json::to_vec(ev),
        WireMessage::Transport(ev) => serde_json::to_vec(ev),
    };
    match bytes_result {
        Ok(mut bytes) => {
            bytes.push(b'\n');
            if let Err(e) = write_half.write_all(&bytes).await {
                debug!(error = %e, "ipc writer write failed");
                return false;
            }
            true
        }
        Err(e) => {
            // Serialization bug shouldn't kill the session — log and
            // skip the offending event.
            debug!(error = %e, "ipc writer serialize failed");
            true
        }
    }
}

/// Push a sanitized transport-error event back to the client. `try_send`
/// (not `send().await`) so a wedged writer mpsc can't backpressure the
/// reader. Drop is acceptable — the client will see protocol drift on
/// the next valid command anyway.
async fn emit_transport_error(
    tx: &mpsc::Sender<WireMessage>,
    kind: IpcErrorKind,
    detail: &str,
) {
    let ev = WireMessage::Transport(TransportEvent::Error {
        kind,
        detail: detail.to_string(),
    });
    if let Err(e) = tx.try_send(ev) {
        debug!(error = %e, "transport error event dropped (writer backpressured)");
    }
}

/// Holder used by the listener loop to remember active clients keyed by
/// id, so the channel impl can fan-out by `client_id`.
pub type ClientMap = Arc<Mutex<std::collections::HashMap<String, ClientHandle>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use ironclaw_common::event::AppEvent;
    use tokio::io::AsyncReadExt;
    use tokio::net::UnixListener;
    use tempfile::tempdir;

    async fn pair_unix() -> (UnixStream, UnixStream) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let connect_handle = tokio::spawn({
            let path = path.clone();
            async move { UnixStream::connect(path).await.unwrap() }
        });
        let (server, _addr) = listener.accept().await.unwrap();
        let client = connect_handle.await.unwrap();
        // Keep dir alive by leaking it to the client side — closes when
        // both stream sides drop and the test ends.
        std::mem::forget(dir);
        (server, client)
    }

    #[tokio::test]
    async fn writer_emits_hello_first() {
        let (server, client) = pair_unix().await;
        let sse = Arc::new(SseManager::new());
        let (inject_tx, _inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let _handle = spawn_session(
            server,
            ClientId::new("c1").unwrap(),
            "owner".into(),
            sse,
            inject_tx,
            DEFAULT_WRITER_BUFFER,
        )
        .await;

        let mut reader = BufReader::new(client);
        let mut first = String::new();
        reader.read_line(&mut first).await.unwrap();
        assert!(first.contains("\"type\":\"ipc_hello\""));
        assert!(first.contains("\"protocol_version\":1"));
        assert!(first.contains("\"local_user_id\":\"owner\""));
    }

    #[tokio::test]
    async fn writer_forwards_direct_event() {
        let (server, client) = pair_unix().await;
        let sse = Arc::new(SseManager::new());
        let (inject_tx, _inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let handle = spawn_session(
            server,
            ClientId::new("c2").unwrap(),
            "owner".into(),
            sse,
            inject_tx,
            DEFAULT_WRITER_BUFFER,
        )
        .await;
        // Drain the hello.
        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        // Push a direct event via the per-client mpsc.
        handle
            .tx
            .send(WireMessage::App(AppEvent::Heartbeat))
            .await
            .expect("send heartbeat");
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert!(line.contains("\"type\":\"heartbeat\""));
    }

    #[tokio::test]
    async fn malformed_line_emits_transport_error_to_client() {
        let (server, client) = pair_unix().await;
        let sse = Arc::new(SseManager::new());
        let (inject_tx, _inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let _handle = spawn_session(
            server,
            ClientId::new("c-err").unwrap(),
            "owner".into(),
            sse,
            inject_tx,
            DEFAULT_WRITER_BUFFER,
        )
        .await;
        // Split the client side so we can read and write concurrently
        // without aliasing &mut.
        let (client_r, mut client_w) = client.into_split();
        let mut reader = BufReader::new(client_r);
        let mut hello = String::new();
        reader.read_line(&mut hello).await.unwrap();
        client_w.write_all(b"this is not json\n").await.unwrap();
        let mut err_line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            reader.read_line(&mut err_line),
        )
        .await
        .expect("transport error timeout")
        .unwrap();
        assert!(err_line.contains("\"type\":\"error\""));
        assert!(err_line.contains("\"kind\":\"command_invalid\""));
    }

    #[tokio::test]
    async fn reader_routes_message_to_inject_tx() {
        let (server, mut client) = pair_unix().await;
        let sse = Arc::new(SseManager::new());
        let (inject_tx, mut inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let _handle = spawn_session(
            server,
            ClientId::new("c3").unwrap(),
            "owner".into(),
            sse,
            inject_tx,
            DEFAULT_WRITER_BUFFER,
        )
        .await;

        let payload = b"{\"type\":\"message\",\"content\":\"hola\"}\n";
        client.write_all(payload).await.unwrap();
        let msg = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            inject_rx.recv(),
        )
        .await
        .expect("inject_rx timed out")
        .expect("inject channel closed");
        assert_eq!(msg.channel, "local_ipc");
        assert_eq!(msg.content, "hola");
        assert_eq!(msg.metadata.unwrap()["client_id"], "c3");
    }
}
```

- [ ] **Step 2: Verificar `AppEvent::Heartbeat`** (verificado al escribir el plan)

```bash
grep -n "Heartbeat\|Response" crates/ironclaw_common/src/event.rs | head -5
```

Expected: `AppEvent::Heartbeat` existe (sin payload) y serializa como `{"type":"heartbeat"}` por el `#[serde(rename = "heartbeat")]` adyacente. Si difiere, cambiar el test (NO mockear AppEvent).

- [ ] **Step 3: Run tests**

```bash
cargo test -p ironclaw --lib channels::local_ipc::client::tests
```

Expected: 4 passed (hello, direct event, reader routing, malformed → transport error).

- [ ] **Step 4: Commit**

```bash
git add src/channels/local_ipc/client.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): add per-client session with WireMessage envelope

WireMessage = App(AppEvent) | Transport(TransportEvent) is the single
type the writer mpsc carries, so transport-error events for malformed
lines and oversized commands actually reach the client (spec §5.1) —
not just a debug log on the server.

Reader: line-buffered NDJSON, 64KiB cap, malformed lines emit a
transport `error` event back to the writer mpsc and continue (silent-ok).
Message → IncomingMessage on inject_tx; Approval/Cancel → typed sideband
via control::build_control_submission.

Writer: emits ipc_hello first, then multiplexes per-client mpsc with a
SseManager.subscribe_raw(Some(user_id)) stream — both targeted respond()
traffic and global broadcast traffic land on the same write_wire path.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task D3: `socket.rs` — listener loop con shutdown

**Files:**
- Modify: `src/channels/local_ipc/socket.rs`

- [ ] **Step 1: Append a `socket.rs`**

```rust
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::UnixListener;
use tokio::sync::{Notify, mpsc};
use tracing::{debug, warn};

use crate::channels::IncomingMessage;
use crate::channels::local_ipc::client::{
    spawn_session, ClientHandle, ClientMap, DEFAULT_WRITER_BUFFER,
};
use crate::channels::local_ipc::protocol::ClientId;
use crate::channels::web::platform::sse::SseManager;

const SOFT_CLIENT_CAP: u64 = 32;
const HARD_CLIENT_CAP: u64 = 256;

pub struct ListenerConfig {
    pub user_id: String,
    pub sse: Arc<SseManager>,
    pub inject_tx: mpsc::Sender<IncomingMessage>,
    pub writer_buffer: usize,
    pub clients: ClientMap,
    pub shutdown: Arc<Notify>,
}

/// Bind, set 0600 perms, and run accept loop until shutdown.notified.
/// Removes the socket file on exit.
pub async fn run_listener(
    path: std::path::PathBuf,
    cfg: ListenerConfig,
) -> Result<(), super::error::LocalIpcError> {
    let listener = UnixListener::bind(&path).map_err(|e| {
        super::error::LocalIpcError::BindFailed {
            path: path.clone(),
            reason: e.to_string(),
        }
    })?;
    // 0600 — POSIX permission gate is the auth model.
    let perms = std::fs::Permissions::from_mode(0o600);
    if let Err(e) = std::fs::set_permissions(&path, perms) {
        warn!(path = %path.display(), error = %e, "failed to chmod 0600 on local IPC socket");
    }
    let active = Arc::new(AtomicU64::new(0));
    let next_id = Arc::new(AtomicU64::new(1));

    loop {
        tokio::select! {
            _ = cfg.shutdown.notified() => {
                debug!("local_ipc listener shutdown notified");
                break;
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _addr)) => {
                        let count = active.fetch_add(1, Ordering::Relaxed) + 1;
                        if count > HARD_CLIENT_CAP {
                            warn!(count, "rejecting local IPC client: hard cap reached");
                            active.fetch_sub(1, Ordering::Relaxed);
                            drop(stream);
                            continue;
                        }
                        if count == SOFT_CLIENT_CAP + 1 {
                            warn!(count, "local IPC clients exceeded soft cap");
                        }
                        let id_num = next_id.fetch_add(1, Ordering::Relaxed);
                        let client_id = match ClientId::new(format!("ipc-{id_num}")) {
                            Ok(c) => c,
                            Err(e) => {
                                warn!(error = %e, "could not mint ClientId");
                                active.fetch_sub(1, Ordering::Relaxed);
                                drop(stream);
                                continue;
                            }
                        };
                        let active_for_session = Arc::clone(&active);
                        let clients = Arc::clone(&cfg.clients);
                        let sse = Arc::clone(&cfg.sse);
                        let inject = cfg.inject_tx.clone();
                        let user = cfg.user_id.clone();
                        let buf = cfg.writer_buffer;
                        let cid_for_remove = client_id.clone();
                        tokio::spawn(async move {
                            let handle = spawn_session(
                                stream, client_id, user, sse, inject, buf,
                            )
                            .await;
                            register(&clients, handle).await;
                            // No await for completion — both tasks live
                            // independently; the registry entry will be
                            // removed when respond() finds it gone (via
                            // a periodic sweep in v2). For v1 the entry
                            // leaks until shutdown, which is bounded by
                            // HARD_CLIENT_CAP. v2 follow-up: track per-
                            // session JoinHandle and unregister on exit.
                            let _ = cid_for_remove;
                            active_for_session.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                    Err(e) => {
                        warn!(error = %e, "local IPC accept failed");
                    }
                }
            }
        }
    }
    if let Err(e) = std::fs::remove_file(&path) {
        debug!(path = %path.display(), error = %e, "remove_file on shutdown failed");
    }
    Ok(())
}

async fn register(clients: &ClientMap, handle: ClientHandle) {
    let mut map = clients.lock().await;
    map.insert(handle.client_id.as_str().to_string(), handle);
}

#[cfg(test)]
mod listener_tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn listener_accepts_one_client_and_emits_hello() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("li.sock");
        let sse = Arc::new(SseManager::new());
        let (inject_tx, _inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let clients: ClientMap = Arc::new(tokio::sync::Mutex::new(Default::default()));
        let shutdown = Arc::new(Notify::new());

        let path_clone = path.clone();
        let sd = Arc::clone(&shutdown);
        let task = tokio::spawn(async move {
            run_listener(
                path_clone,
                ListenerConfig {
                    user_id: "owner".into(),
                    sse,
                    inject_tx,
                    writer_buffer: DEFAULT_WRITER_BUFFER,
                    clients,
                    shutdown: sd,
                },
            )
            .await
        });

        // Wait for the bind.
        for _ in 0..50 {
            if path.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(path.exists(), "socket file must exist after bind");

        let stream = UnixStream::connect(&path).await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            reader.read_line(&mut line),
        )
        .await
        .expect("hello timeout")
        .unwrap();
        assert!(line.contains("\"type\":\"ipc_hello\""));

        shutdown.notify_waiters();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("listener did not exit on shutdown");
        assert!(!path.exists(), "socket file must be removed on shutdown");
    }

    #[tokio::test]
    async fn listener_chmods_0600() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("perm.sock");
        let sse = Arc::new(SseManager::new());
        let (inject_tx, _inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let clients: ClientMap = Arc::new(tokio::sync::Mutex::new(Default::default()));
        let shutdown = Arc::new(Notify::new());

        let path_clone = path.clone();
        let sd = Arc::clone(&shutdown);
        let task = tokio::spawn(async move {
            run_listener(
                path_clone,
                ListenerConfig {
                    user_id: "owner".into(),
                    sse,
                    inject_tx,
                    writer_buffer: DEFAULT_WRITER_BUFFER,
                    clients,
                    shutdown: sd,
                },
            )
            .await
        });

        for _ in 0..50 {
            if path.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let meta = std::fs::metadata(&path).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "socket must be chmod 0600 (got {mode:o})");
        shutdown.notify_waiters();
        let _ = task.await;
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p ironclaw --lib channels::local_ipc::socket::listener_tests
```

Expected: 2 passed.

- [ ] **Step 3: Commit**

```bash
git add src/channels/local_ipc/socket.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): add accept loop with chmod 0600 + shutdown unlink

select! between accept and shutdown.notified. Soft cap 32 (warn but
accept), hard cap 256 (reject + drop). On shutdown, remove the socket
file. Per-session ClientHandle is registered in the shared ClientMap
so the channel impl can route respond()/send_status by client_id.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Track E · `Channel` trait impl + factory

### Task E1: `LocalIpcChannel` struct + `Channel` impl

**Files:**
- Modify: `src/channels/local_ipc/channel_impl.rs`
- Modify: `src/channels/local_ipc/mod.rs` (descomentar `pub use channel_impl::LocalIpcChannel`)

- [ ] **Step 1: Reemplazar el stub completo**

Tipos reales del codebase (verificados al escribir el plan):

- `AppEvent::Response { content: String, thread_id: String }` — `thread_id` es **String**, no `Option`. `crates/ironclaw_common/src/event.rs:203`.
- `OutgoingResponse.thread_id: Option<ExternalThreadId>` — newtype envuelve un String. `src/channels/channel.rs:331`.
- `IncomingMessage.metadata: serde_json::Value` — directo, NO `Option<Value>`. Default `Value::Null`. `src/channels/channel.rs:101`.
- `ChannelError::HealthCheckFailed { name: String }` (NO `Unhealthy(String)`). `src/error.rs:141`.
- `ChannelError::StartupFailed { name: String, reason: String }` (struct variant). `src/error.rs:117`.

```rust
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, Notify, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, warn};

use crate::channels::local_ipc::client::{ClientMap, DEFAULT_WRITER_BUFFER, WireMessage};
use crate::channels::local_ipc::socket::{run_listener, ListenerConfig};
use crate::channels::web::platform::sse::SseManager;
use crate::channels::{
    Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate,
};
use crate::error::ChannelError;

pub struct LocalIpcChannel {
    socket_path: PathBuf,
    user_id: String,
    sse: Arc<SseManager>,
    writer_buffer: usize,
    clients: ClientMap,
    shutdown: Arc<Notify>,
    // mpsc::Sender that the reader tasks will use to inject messages
    // into the agent loop. Materialized in `start()`.
    inject_tx: Mutex<Option<mpsc::Sender<IncomingMessage>>>,
}

impl LocalIpcChannel {
    pub fn new(
        socket_path: PathBuf,
        user_id: String,
        sse: Arc<SseManager>,
        writer_buffer: usize,
    ) -> Self {
        Self {
            socket_path,
            user_id,
            sse,
            writer_buffer,
            clients: Arc::new(Mutex::new(Default::default())),
            shutdown: Arc::new(Notify::new()),
            inject_tx: Mutex::new(None),
        }
    }

    fn build_response_event(response: OutgoingResponse) -> ironclaw_common::event::AppEvent {
        ironclaw_common::event::AppEvent::Response {
            content: response.content,
            // OutgoingResponse.thread_id is Option<ExternalThreadId>;
            // AppEvent::Response.thread_id is plain String. Empty string
            // when the caller didn't pin a thread (matches the web
            // channel's behavior at src/bridge/router.rs Response sites).
            thread_id: response
                .thread_id
                .map(|t| t.as_str().to_string())
                .unwrap_or_default(),
        }
    }

    fn extract_client_id(msg: &IncomingMessage) -> &str {
        // metadata is a serde_json::Value (default Value::Null), not
        // Option. Direct .get() on Null returns None safely.
        msg.metadata
            .get("client_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }
}

#[async_trait]
impl Channel for LocalIpcChannel {
    fn name(&self) -> &str {
        "local_ipc"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let (tx, rx) = mpsc::channel::<IncomingMessage>(64);
        {
            let mut guard = self.inject_tx.lock().await;
            *guard = Some(tx.clone());
        }
        let cfg = ListenerConfig {
            user_id: self.user_id.clone(),
            sse: Arc::clone(&self.sse),
            inject_tx: tx,
            writer_buffer: self.writer_buffer,
            clients: Arc::clone(&self.clients),
            shutdown: Arc::clone(&self.shutdown),
        };
        let path = self.socket_path.clone();
        tokio::spawn(async move {
            if let Err(e) = run_listener(path, cfg).await {
                warn!(error = %e, "local_ipc listener exited with error");
            }
        });
        let stream: MessageStream = Box::pin(ReceiverStream::new(rx));
        Ok(stream)
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let client_id = Self::extract_client_id(msg);
        let map = self.clients.lock().await;
        if let Some(handle) = map.get(client_id) {
            let wire = WireMessage::App(Self::build_response_event(response));
            if handle.tx.send(wire).await.is_err() {
                debug!(client_id, "respond: writer mpsc closed");
            }
        } else {
            debug!(client_id, "respond: client_id not registered");
        }
        Ok(())
    }

    async fn send_status(
        &self,
        _status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        // Writers also subscribe to SseManager directly; status events
        // routed through there reach the same client. respond() is the
        // only "directed" path we honour explicitly. Default no-op.
        Ok(())
    }

    async fn broadcast(
        &self,
        user_id: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        if user_id != self.user_id {
            return Ok(());
        }
        let event = Self::build_response_event(response);
        let map = self.clients.lock().await;
        for handle in map.values() {
            let _ = handle.tx.send(WireMessage::App(event.clone())).await;
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        if !self.socket_path.exists() {
            return Err(ChannelError::HealthCheckFailed {
                name: "local_ipc".into(),
            });
        }
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        self.shutdown.notify_waiters();
        Ok(())
    }
}

// Suppress unused import warning when no DEFAULT_WRITER_BUFFER consumer
// in this file uses it (LocalIpcChannel takes writer_buffer at new()).
#[allow(unused_imports)]
const _: () = { let _ = DEFAULT_WRITER_BUFFER; };
```

- [ ] **Step 2: Verificación rápida de signatures**

```bash
grep -n "    Response\s*{\|HealthCheckFailed\|StartupFailed\s*{" \
    crates/ironclaw_common/src/event.rs src/error.rs | head -5
```

Expected (verificado al escribir el plan): `AppEvent::Response { content: String, thread_id: String }` en `event.rs:203`; `ChannelError::HealthCheckFailed { name: String }` y `ChannelError::StartupFailed { name, reason }` en `src/error.rs:117,141`. Si difieren, ajustar las construcciones (NO añadir variantes nuevas a `ChannelError`).

- [ ] **Step 3: Descomentar la re-exportación en `mod.rs`**

```rust
pub use channel_impl::LocalIpcChannel;
```

- [ ] **Step 4: Verificar compilación**

```bash
cargo check
```

Expected: ok.

- [ ] **Step 5: Commit**

```bash
git add src/channels/local_ipc/channel_impl.rs src/channels/local_ipc/mod.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): impl Channel for LocalIpcChannel

start() spawns the listener and returns a ReceiverStream backed by an
mpsc the reader tasks inject into. respond() reads client_id from
metadata (serde_json::Value, not Option), looks it up in the client
map, and writes WireMessage::App(AppEvent::Response { ... }) on the
per-client mpsc — converting OutgoingResponse.thread_id (Option<...>)
to AppEvent::Response.thread_id (plain String) by unwrap_or_default.
broadcast() fans out to all clients when user_id matches the local
user. health_check returns HealthCheckFailed if the socket file
disappeared (NOT Unhealthy — that variant doesn't exist).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task E2: `mod.rs` `create()` factory

**Files:**
- Modify: `src/channels/local_ipc/mod.rs`

- [ ] **Step 1: Reemplazar `mod.rs` para añadir `create()`**

```rust
//! Local UNIX-socket IPC channel.
//!
//! Reemplaza `crates/jarvis_ui_bridge/` exponiendo un UNIX socket NDJSON
//! directamente en el core IronClaw para que voice daemon y Quickshell UI
//! consuman eventos y manden comandos sin pasar por el gateway HTTP/WS.
//!
//! Ver `docs/superpowers/specs/2026-04-30-jarvis-os-local-ipc-design.md`.

mod channel_impl;
mod client;
mod control;
mod error;
mod protocol;
mod socket;

use std::sync::Arc;

pub use channel_impl::LocalIpcChannel;
pub use error::LocalIpcError;
pub use socket::{resolve_socket_path, SocketResolution};

use crate::channels::web::platform::sse::SseManager;

/// Build a `LocalIpcChannel` ready to be added to `ChannelManager`, or
/// `Ok(None)` if `IRONCLAW_LOCAL_SOCKET=disabled`.
///
/// Performs orphan-socket cleanup before the channel binds in
/// `start()`. The bind itself happens lazily on `start()` so the
/// caller can wire the channel into `ChannelManager` synchronously.
pub async fn create(
    user_id: String,
    sse: Arc<SseManager>,
    writer_buffer: usize,
) -> Result<Option<LocalIpcChannel>, LocalIpcError> {
    let path = match resolve_socket_path() {
        SocketResolution::Disabled => {
            tracing::debug!("local_ipc disabled by IRONCLAW_LOCAL_SOCKET=disabled");
            return Ok(None);
        }
        SocketResolution::Path(p) => p,
    };
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            LocalIpcError::BindFailed {
                path: path.clone(),
                reason: format!("create parent dir: {e}"),
            }
        })?;
    }
    let cleaned = socket::cleanup_orphan_socket(&path).await?;
    if !cleaned {
        return Err(LocalIpcError::SocketBusy { path });
    }
    Ok(Some(LocalIpcChannel::new(path, user_id, sse, writer_buffer)))
}
```

- [ ] **Step 2: Verificar compilación**

```bash
cargo check
```

Expected: ok.

- [ ] **Step 3: Test smoke unit**

Añade al final de `mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_returns_none_when_disabled() {
        // SAFETY: env mutation is process-global; this test relies on no
        // other test in the same process touching IRONCLAW_LOCAL_SOCKET
        // concurrently. The socket::tests use an internal Mutex; we rely
        // on that lock being unrelated to this one. If parallel test
        // collisions appear, gate this test with the same lock.
        let prev = std::env::var("IRONCLAW_LOCAL_SOCKET").ok();
        unsafe {
            std::env::set_var("IRONCLAW_LOCAL_SOCKET", "disabled");
        }
        let sse = Arc::new(SseManager::new());
        let result = create("owner".into(), sse, 256).await.unwrap();
        unsafe {
            match prev {
                Some(v) => std::env::set_var("IRONCLAW_LOCAL_SOCKET", v),
                None => std::env::remove_var("IRONCLAW_LOCAL_SOCKET"),
            }
        }
        assert!(result.is_none());
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p ironclaw --lib channels::local_ipc
```

Expected: todos los tests del módulo pasan.

- [ ] **Step 5: Commit**

```bash
git add src/channels/local_ipc/mod.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): add create() factory

Resolves the path, ensures the parent directory exists, runs the
orphan-socket cleanup, and returns Ok(None) when explicitly disabled.
Bind happens lazily in Channel::start(); create() is the activation
boundary where any startup-fatal error surfaces.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Track F · Wiring en main.rs + config

### Task F1: Añadir `LocalIpcConfig` a config

**Files:**
- Modify: `src/config/channels.rs`

- [ ] **Step 1: Localizar struct ChannelsConfig**

```bash
grep -n "pub struct ChannelsConfig\|pub struct LocalIpcConfig\|pub struct CliConfig" src/config/channels.rs | head -5
```

- [ ] **Step 2: Añadir `LocalIpcConfig` y campo en `ChannelsConfig`**

Después de la última struct de subconfig en `src/config/channels.rs`, añade:

```rust
#[derive(Debug, Clone)]
pub struct LocalIpcConfig {
    /// Per-client mpsc buffer for writer tasks. Defaults to 256.
    pub writer_buffer: usize,
}

impl Default for LocalIpcConfig {
    fn default() -> Self {
        Self { writer_buffer: 256 }
    }
}

impl LocalIpcConfig {
    pub fn from_env() -> Self {
        let writer_buffer = std::env::var("IRONCLAW_LOCAL_IPC_BUFFER")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(256);
        Self { writer_buffer }
    }
}
```

Y en la struct `ChannelsConfig`, añade el campo:

```rust
pub local_ipc: LocalIpcConfig,
```

Inicializa el campo en el constructor / `from_env` de `ChannelsConfig`:

```rust
local_ipc: LocalIpcConfig::from_env(),
```

(Buscar el constructor existente — `impl ChannelsConfig { pub fn from_env() -> ... }` o equivalente — y añadir esa línea junto a las demás.)

- [ ] **Step 3: Verificar compilación**

```bash
cargo check
```

Expected: ok.

- [ ] **Step 4: Commit**

```bash
git add src/config/channels.rs
git commit -m "$(cat <<'EOF'
feat(config): add LocalIpcConfig for IRONCLAW_LOCAL_IPC_BUFFER

Single knob — writer mpsc buffer per client (default 256, env
IRONCLAW_LOCAL_IPC_BUFFER overrides). Path resolution lives in
src/channels/local_ipc/socket.rs since it is consumed by both
server and clients.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task F2: Wire en `main.rs`

**Files:**
- Modify: `src/main.rs`

**Constraint clave:** `GatewayChannel::new()` construye su `Arc<SseManager>` internamente en `src/channels/web/mod.rs:149-152` y NO acepta uno externo. El `agent.deps.sse_tx` (línea 1258 de main.rs hoy: `sse_tx: sse_manager`) recibe ese mismo Arc del gateway. Por lo tanto el `local_ipc` debe **reusar** `gw.state().sse` y construirse DESPUÉS del gateway block — no antes. Si el gateway está desactivado, local_ipc construye su propio `Arc<SseManager>` y lo asigna también a `sse_manager` para que el agent loop lo alimente.

- [ ] **Step 1: Localizar el bloque exacto del SseManager**

Lee main.rs líneas 830-1040. El layout actual es:

```rust
// línea 833
let mut sse_manager: Option<std::sync::Arc<ironclaw::channels::web::sse::SseManager>> = None;

// línea ~835
let mut gw = GatewayChannel::new(gw_config.clone(), config.owner_id.clone());
// ... múltiples gw.with_*(...) ...

// línea 1036 (dentro del bloque del gateway)
sse_manager = Some(Arc::clone(&gw.state().sse));
```

- [ ] **Step 2: Insertar el bloque local_ipc DESPUÉS del gateway block, garantizando SseManager disponible**

Localiza el sitio donde el gateway block termina (después de la asignación `sse_manager = Some(Arc::clone(&gw.state().sse));` en ≈ línea 1036). Inserta inmediatamente después, FUERA del `if let Some(ref gateway_config)` o equivalente que envuelve al gateway:

```rust
    // ─── Local UNIX-socket IPC channel (replaces jarvis_ui_bridge) ───
    //
    // Reuses gateway's SseManager when present; otherwise materializes
    // its own and assigns it to `sse_manager` so the agent_loop's
    // sse_tx (set further down at the AgentDependencies init) sees the
    // same Arc and feeds it. Without that assignment, local_ipc
    // subscribers would never receive AppEvents.
    if enable_non_cli {
        let sse_for_local = match sse_manager.as_ref() {
            Some(existing) => Arc::clone(existing),
            None => {
                let fresh = Arc::new(
                    ironclaw::channels::web::sse::SseManager::new(),
                );
                sse_manager = Some(Arc::clone(&fresh));
                fresh
            }
        };
        match ironclaw::channels::local_ipc::create(
            config.owner_id.clone(),
            sse_for_local,
            config.channels.local_ipc.writer_buffer,
        )
        .await
        {
            Ok(Some(channel)) => {
                channel_names.push("local_ipc".to_string());
                channels.add(Box::new(channel)).await;
                tracing::debug!(
                    "local_ipc channel enabled (writer_buffer={})",
                    config.channels.local_ipc.writer_buffer
                );
            }
            Ok(None) => {
                tracing::debug!("local_ipc channel disabled by env");
            }
            Err(e) => {
                tracing::warn!("local_ipc channel failed to initialize: {e}");
            }
        }
    }
```

Notas:

- Usar `tracing::debug!` (no `info!`) — el REPL/TUI todavía no inicializó cuando esto corre, pero la regla de CLAUDE.md sobre `info!` desaconseja `info!` en background salvo "user-facing status que el REPL intencionalmente renderiza". Aquí el debug! basta; el operador puede subir el nivel para verlo.
- La rama `None => fresh` es necesaria para el setup CLI-only (sin gateway): el agent loop sigue necesitando un SseManager en `sse_tx` para que `bridge::router` pueda hacer `broadcast_for_user`.

- [ ] **Step 3: Verificar compilación**

```bash
cargo check
```

Expected: ok. Si rompe por imports faltantes (`std::sync::Arc` en este scope), añadir el import al top del archivo o al bloque enclosing.

- [ ] **Step 4: Verificación visual del ordering**

```bash
grep -n "sse_manager = Some\|local_ipc::create\|sse_tx: sse_manager" src/main.rs
```

Expected: el orden debe ser `sse_manager = Some(...)` (línea ~1036) → `local_ipc::create(...)` (insertado en Step 2) → `sse_tx: sse_manager` (línea ~1258). Si `local_ipc::create` queda ANTES del `sse_manager = Some(...)`, el SseManager local nunca se asigna a sse_manager y el agent loop no lo alimenta. Mover el bloque hasta que el ordering sea correcto.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "$(cat <<'EOF'
feat(local_ipc): wire LocalIpcChannel into main.rs

Inserted after the gateway block so it can reuse gw.state().sse — the
SseManager that the agent loop's sse_tx will be fed (the same Arc set
at the AgentDependencies init below). When the gateway is disabled
(CLI-only mode), local_ipc materializes its own SseManager and assigns
it to `sse_manager` so the agent loop still has a bus to broadcast to.

Honours IRONCLAW_LOCAL_SOCKET=disabled by silently producing Ok(None).
Bind failures log a warning and skip the channel — they don't abort
startup.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Track G · Borrar el bridge

### Task G1: Verificar que voice daemon no usa el bridge

**Files:** read-only

- [ ] **Step 1: Búsqueda exhaustiva**

```bash
grep -rn "jarvis-ui-bridge\|jarvis_ui_bridge\|gateway_host\|gateway_port\|JARVIS_UI_BRIDGE" crates/jarvis_voice_daemon/
```

Expected: cero matches. Si aparece alguno, antes de proceder con el borrado del bridge, abre un issue / nota en el plan ("voice_daemon depende de bridge X — reapuntar a UNIX socket").

- [ ] **Step 2: Confirmar que voice daemon habla solo a ElevenLabs**

```bash
grep -rn "elevenlabs\|convai\|wss://api.elevenlabs" crates/jarvis_voice_daemon/src/ | head -5
```

Expected: matches en `ws_client.rs` o equivalente. Si no hay nada de bridge ni gateway, el voice daemon ya es independiente — la integración con el nuevo IPC quedará como follow-up (ver Track I, opcional).

- [ ] **Step 3: Documentar en commit del Step 4 de Task G2 que el voice daemon queda intocado en este PR**

(Sin commit aquí, sólo verificación.)

---

### Task G2: Update EventBus.qml para apuntar al nuevo socket

**Files:**
- Modify: `ui/jarvis-os/core/EventBus.qml`

- [ ] **Step 1: Cambiar `socketPath`**

En `ui/jarvis-os/core/EventBus.qml` línea 30, reemplaza:

```qml
    readonly property string socketPath: "/run/user/1000/jarvis-ui-bridge.sock"
```

por:

```qml
    readonly property string socketPath: "/run/user/1000/ironclaw.sock"
```

- [ ] **Step 2: Actualizar el comentario adyacente**

Líneas 27-30 actualmente dicen "The bridge service writes the socket here". Reemplaza por:

```qml
    // Bus socket. systemd-user puts XDG_RUNTIME_DIR=/run/user/<uid>;
    // for the loopback case the UID is the user's (typically 1000 on
    // single-user Arch). The IronClaw core writes the socket here as
    // part of its startup (src/channels/local_ipc/).
```

- [ ] **Step 3: Eliminar el manejo de `bridge_online` / `bridge_offline`**

En `_handleLine`, las líneas 71-81 del archivo original manejan eventos sintéticos `bridge_online` / `bridge_offline` que el bridge inyectaba. El nuevo IPC NO emite esos. En su lugar emite `ipc_hello` una sola vez al conectar. Reemplaza:

```qml
        // Bridge synthetic events update connection state.
        if (ev.type === "bridge_online") {
            bus.connected = true;
            bus.lastError = "";
            return;
        }
        if (ev.type === "bridge_offline") {
            bus.connected = false;
            bus.lastError = "bridge offline";
            return;
        }
```

por:

```qml
        // Local IPC handshake: ipc_hello fires once on connect.
        if (ev.type === "ipc_hello") {
            bus.connected = true;
            bus.lastError = "";
            return;
        }
        if (ev.type === "error") {
            console.warn("[EventBus] ipc error:", ev.kind, ev.detail);
            return;
        }
```

`bridge_offline` no tiene reemplazo directo; la desconexión la detecta `onExited` de socat (líneas 38-42), que ya pone `bus.connected = false`. Eso es suficiente.

- [ ] **Step 4: Smoke test manual** (sin CI; documentar en el commit como pendiente de verificar en Asus)

(No hay test automatizado de QML aquí; reservamos la validación manual para el final del plan, Track J.)

- [ ] **Step 5: Commit**

```bash
git add ui/jarvis-os/core/EventBus.qml
git commit -m "$(cat <<'EOF'
fix(ui): point EventBus to the new local_ipc socket

socketPath: /run/user/1000/jarvis-ui-bridge.sock → /run/user/1000/ironclaw.sock.
bridge_online / bridge_offline synthetic events replaced with the new
ipc_hello handshake + error event handling. Disconnection is still
detected by socat's onExited (no bridge_offline equivalent needed).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task G3: Eliminar `crates/jarvis_ui_bridge/`

**Files:**
- Delete: `crates/jarvis_ui_bridge/` (recursivo)
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Eliminar el directorio**

```bash
git rm -r crates/jarvis_ui_bridge/
```

- [ ] **Step 2: Quitar de `Cargo.toml` workspace members**

Edit `Cargo.toml` línea 2. Cambia:

```toml
members = [".", "crates/ironclaw_common", "crates/ironclaw_safety", "crates/ironclaw_skills", "crates/ironclaw_engine", "crates/ironclaw_gateway", "crates/ironclaw_tui", "crates/jarvis_policies", "crates/jarvis_system_tools", "crates/jarvis_ui_bridge", "crates/jarvis_voice_daemon"]
```

por:

```toml
members = [".", "crates/ironclaw_common", "crates/ironclaw_safety", "crates/ironclaw_skills", "crates/ironclaw_engine", "crates/ironclaw_gateway", "crates/ironclaw_tui", "crates/jarvis_policies", "crates/jarvis_system_tools", "crates/jarvis_voice_daemon"]
```

- [ ] **Step 3: Verificar compilación del workspace**

```bash
cargo check --workspace
```

Expected: ok. Si algún otro crate todavía referenciaba `jarvis_ui_bridge` en sus deps, surge aquí. Investiga (`grep -rn "jarvis_ui_bridge" Cargo.toml crates/*/Cargo.toml`) y elimina la dep.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml
git commit -m "$(cat <<'EOF'
chore: drop jarvis_ui_bridge crate, superseded by local_ipc

The bridge wrapped the gateway WS and re-exported it as a UNIX socket
to absorb auth/port concerns. With src/channels/local_ipc/ the core
exposes the same socket directly, so the wrapper is dead code. Workspace
member entry removed; cargo check passes across the workspace.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task G4: Eliminar systemd unit + actualizar install.sh

**Files:**
- Delete: `arch/systemd-user/jarvis-ui-bridge.service`
- Modify: `arch/install.sh`

- [ ] **Step 1: Eliminar la unit**

```bash
git rm arch/systemd-user/jarvis-ui-bridge.service
```

- [ ] **Step 2: Editar `arch/install.sh`**

Localiza las líneas que mencionan `jarvis-ui-bridge` (3 lugares según el grep previo: línea 92 `cargo build`, línea 94 `for bin`, líneas 130-138 install + enable).

Aplica estos edits exactos:

- Línea 92 (`cargo build --release -p jarvis_ui_bridge --bin jarvis-ui-bridge`) → eliminar la línea entera.
- Línea 94 (`for bin in ironclaw jarvis-voice-daemon jarvis-ui-bridge; do`) → cambiar a `for bin in ironclaw jarvis-voice-daemon; do`.
- Líneas 130-132 (bloque que copia `jarvis-ui-bridge.service` a `~/.config/systemd/user/`) → eliminar esas 3 líneas (el `cp` + comentario adyacente).
- Línea 137 (`systemctl --user enable jarvis-ui-bridge.service`) → eliminar.
- Línea 138 (`log "  jarvis-ui.service + jarvis-ui-bridge.service enabled"`) → cambiar a `log "  jarvis-ui.service enabled"`.
- Línea 140 (`log "   systemctl --user start jarvis-ui-bridge jarvis-ui)"`) → cambiar a `log "   systemctl --user start jarvis-ui)"`.

- [ ] **Step 3: Verificar que install.sh no tiene mentions residuales**

```bash
grep -n "jarvis-ui-bridge\|jarvis_ui_bridge" arch/install.sh
```

Expected: cero matches.

- [ ] **Step 4: Commit**

```bash
git add arch/install.sh arch/systemd-user/jarvis-ui-bridge.service
git commit -m "$(cat <<'EOF'
chore(arch): drop jarvis-ui-bridge from install.sh + systemd

Removes the build/install/enable lines from arch/install.sh (3 spots)
and deletes the systemd-user unit. The IronClaw core now binds the
socket directly during its own startup, no separate daemon needed.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task G5: Update `.env.example` y `CLAUDE.md`

**Files:**
- Modify: `.env.example`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Verificar y actualizar `.env.example`**

```bash
grep -n "JARVIS_UI_BRIDGE\|IRONCLAW_LOCAL_SOCKET\|IRONCLAW_LOCAL_IPC" .env.example
```

Si hay líneas `JARVIS_UI_BRIDGE_*`, eliminarlas. Añade al final del archivo (o en una sección "Local IPC" agrupada):

```dotenv
# Local IPC channel (UNIX socket for voice daemon + Quickshell UI)
# Path resolution: env var > $XDG_RUNTIME_DIR/ironclaw.sock > $HOME/.ironclaw/ironclaw.sock
# Set to "disabled" to opt out (gateway HTTP/WS stays available).
# IRONCLAW_LOCAL_SOCKET=

# Per-client writer mpsc buffer (default 256, must be > 0)
# IRONCLAW_LOCAL_IPC_BUFFER=256
```

NO eliminar `GATEWAY_AUTH_TOKEN` ni `GATEWAY_ENABLED` — esos siguen vivos para el uso remoto del gateway.

- [ ] **Step 2: Actualizar `CLAUDE.md`**

Localiza el bloque de Project Structure (`crates/...` tree). Reemplaza la línea:

```
└── jarvis_voice_daemon/ # Rust + cpal + tokio-tungstenite + ElevenLabs Convai cloud
```

…asegurándote de que `jarvis_ui_bridge/` ya no aparece en el listado. Si aparece (busca con `grep -n "jarvis_ui_bridge" CLAUDE.md`), eliminar esa línea.

Añade en la sección de `src/channels/` del tree (después de `webhook_server.rs`):

```
├── local_ipc/          # Native UNIX socket IPC for jarvis-os UI + voice daemon (NDJSON)
│   ├── mod.rs          # create() factory + LocalIpcChannel re-exports
│   ├── error.rs        # LocalIpcError
│   ├── protocol.rs     # ClientCommand, ClientId, IpcErrorKind, TransportEvent
│   ├── socket.rs       # resolve_socket_path, cleanup_orphan_socket, run_listener
│   ├── client.rs       # ClientSession reader + writer tasks
│   ├── control.rs      # build_control_submission (Approval/Cancel → Submission)
│   └── channel_impl.rs # impl Channel for LocalIpcChannel
```

- [ ] **Step 3: Verificar**

```bash
grep -n "jarvis_ui_bridge\|jarvis-ui-bridge" CLAUDE.md .env.example
```

Expected: cero matches.

- [ ] **Step 4: Commit**

```bash
git add .env.example CLAUDE.md
git commit -m "$(cat <<'EOF'
docs: replace jarvis_ui_bridge references with local_ipc

.env.example documents IRONCLAW_LOCAL_SOCKET + IRONCLAW_LOCAL_IPC_BUFFER
with the path-resolution order and the "disabled" opt-out token. CLAUDE.md
Project Structure tree now lists src/channels/local_ipc/ and drops the
jarvis_ui_bridge crate.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Track H · Integration tests

Crea `tests/local_ipc_integration.rs` con los 11 escenarios de la §11.2 del spec. Cada test es independiente (no shared state); usar `tempfile::tempdir` para sockets aislados.

### Task H1: Test harness + bind/connect/hello/ping

**Files:**
- Create: `tests/local_ipc_integration.rs`

- [ ] **Step 1: Esqueleto del test file**

```rust
//! Integration tests for src/channels/local_ipc/.
//! See docs/superpowers/specs/2026-04-30-jarvis-os-local-ipc-design.md §11.2.

#![cfg(feature = "integration")]

use std::sync::Arc;
use std::time::Duration;

use ironclaw::channels::local_ipc::LocalIpcChannel;
use ironclaw::channels::web::platform::sse::SseManager;
use ironclaw::channels::Channel;
use ironclaw_common::event::AppEvent;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Wait for the listener to bind, polling the socket path. Caps at 2s
/// (100 × 20ms). Caller passes the same path used at construction.
async fn wait_for_bind(path: &std::path::Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("listener did not bind {} within 2s", path.display());
}

async fn spawn_channel(
    socket_path: std::path::PathBuf,
) -> (Arc<LocalIpcChannel>, Arc<SseManager>) {
    let sse = Arc::new(SseManager::new());
    let chan = Arc::new(LocalIpcChannel::new(
        socket_path.clone(),
        "owner".into(),
        Arc::clone(&sse),
        16,
    ));
    let _stream = chan.start().await.expect("start");
    wait_for_bind(&socket_path).await;
    (chan, sse)
}

#[tokio::test]
async fn test_bind_connect_hello_ping() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h1.sock");
    let (_chan, _sse) = spawn_channel(path.clone()).await;
    assert!(path.exists(), "socket file must exist after spawn_channel returns");

    let stream = UnixStream::connect(&path).await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut hello = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut hello))
        .await
        .expect("hello timeout")
        .unwrap();
    assert!(hello.contains("\"type\":\"ipc_hello\""));
    assert!(hello.contains("\"local_user_id\":\"owner\""));
    assert!(hello.contains("\"protocol_version\":1"));
}
```

Nota: `LocalIpcChannel::new` y `start()` ya devuelven el stream pero no lo necesitamos en los tests porque el flujo agente→IPC va por SseManager. La función `spawn_channel` deja el `_stream` colgado intencionadamente (cae fuera del scope, no rompe nada porque el `start()` interno spawnea el listener task).

- [ ] **Step 2: Run con feature**

```bash
cargo test --features integration --test local_ipc_integration test_bind_connect_hello_ping
```

Expected: passed.

- [ ] **Step 3: Commit**

```bash
git add tests/local_ipc_integration.rs
git commit -m "$(cat <<'EOF'
test(local_ipc): integration test #1 — bind/connect/hello

Drives Channel::start() end-to-end against a temp UNIX socket. Asserts
the file exists after bind and that the first NDJSON line is the
ipc_hello handshake. Gated under the integration feature.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task H2: Tests #2-#3 — múltiples clientes + filtrado por user_id

**Files:**
- Modify: `tests/local_ipc_integration.rs`

- [ ] **Step 1: Append los dos tests**

```rust
async fn drain_hello(reader: &mut BufReader<UnixStream>) {
    let mut hello = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut hello))
        .await
        .expect("hello timeout")
        .unwrap();
    assert!(hello.contains("ipc_hello"));
}

#[tokio::test]
async fn test_two_clients_receive_same_broadcast() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h2.sock");
    let (_chan, sse) = spawn_channel(path.clone()).await;
    // Bind already awaited in spawn_channel/spawn_channel_with_stream.
    let mut a = BufReader::new(UnixStream::connect(&path).await.unwrap());
    let mut b = BufReader::new(UnixStream::connect(&path).await.unwrap());
    drain_hello(&mut a).await;
    drain_hello(&mut b).await;

    sse.broadcast(AppEvent::Heartbeat);
    let mut la = String::new();
    let mut lb = String::new();
    tokio::time::timeout(Duration::from_secs(2), a.read_line(&mut la))
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), b.read_line(&mut lb))
        .await
        .unwrap()
        .unwrap();
    assert!(la.contains("heartbeat"));
    assert!(lb.contains("heartbeat"));
}

#[tokio::test]
async fn test_scoped_event_for_other_user_filtered() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h3.sock");
    let (_chan, sse) = spawn_channel(path.clone()).await;
    // Bind already awaited in spawn_channel/spawn_channel_with_stream.
    let mut a = BufReader::new(UnixStream::connect(&path).await.unwrap());
    drain_hello(&mut a).await;

    // Push a scoped event for a DIFFERENT user.
    sse.broadcast_for_user("not-owner", AppEvent::Heartbeat);
    // Then push a global event we DO want to see.
    sse.broadcast(AppEvent::Heartbeat);

    let mut la = String::new();
    tokio::time::timeout(Duration::from_secs(2), a.read_line(&mut la))
        .await
        .unwrap()
        .unwrap();
    // The line we receive should be the global heartbeat — we can't
    // easily distinguish it from the scoped one shape-wise, but the
    // count proves the filter worked: only ONE line should be in the
    // pipe (the global one). Read with a short timeout to confirm.
    let mut second = String::new();
    let res = tokio::time::timeout(Duration::from_millis(300), a.read_line(&mut second)).await;
    assert!(res.is_err(), "second read must time out (no extra event)");
    assert!(la.contains("heartbeat"));
}
```

- [ ] **Step 2: Run**

```bash
cargo test --features integration --test local_ipc_integration
```

Expected: 3 passed.

- [ ] **Step 3: Commit**

```bash
git add tests/local_ipc_integration.rs
git commit -m "$(cat <<'EOF'
test(local_ipc): integration tests #2-#3 — fan-out + user filter

#2 connects two clients and asserts both receive the same global
SseManager broadcast. #3 sends a scoped event to a different user
plus a global one; the local client must only see the global one
(read-with-timeout proves no extra event arrived).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task H3: Tests #4-#5 — Approval + Cancel through the caller

**Files:**
- Modify: `tests/local_ipc_integration.rs`

Estos son los tests **test through the caller** (regla `testing.md`). Drivean el flujo desde el wire del cliente hasta la `Submission` que llega al `MessageStream`.

- [ ] **Step 1: Append**

```rust
use futures_util::StreamExt;
use ironclaw::agent::submission::Submission;
use ironclaw::channels::IncomingMessage;
use uuid::Uuid;

async fn spawn_channel_with_stream(
    socket_path: std::path::PathBuf,
) -> (
    Arc<LocalIpcChannel>,
    Arc<SseManager>,
    std::pin::Pin<Box<dyn futures_util::Stream<Item = IncomingMessage> + Send>>,
) {
    let sse = Arc::new(SseManager::new());
    let chan = Arc::new(LocalIpcChannel::new(
        socket_path.clone(),
        "owner".into(),
        Arc::clone(&sse),
        16,
    ));
    let stream = chan.start().await.expect("start");
    wait_for_bind(&socket_path).await;
    (chan, sse, stream)
}

#[tokio::test]
async fn test_approval_routes_through_to_inject_stream() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h4.sock");
    let (_chan, _sse, mut stream) = spawn_channel_with_stream(path.clone()).await;

    let client = UnixStream::connect(&path).await.unwrap();
    let (client_r, mut client_w) = client.into_split();
    let mut reader = BufReader::new(client_r);
    let mut hello = String::new();
    reader.read_line(&mut hello).await.unwrap();

    let req_id = Uuid::new_v4();
    let payload = format!(
        "{{\"type\":\"approval\",\"request_id\":\"{req_id}\",\"action\":\"approve\"}}\n"
    );
    client_w.write_all(payload.as_bytes()).await.unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("stream timeout")
        .expect("stream ended");
    assert_eq!(msg.channel, "local_ipc");
    // Sideband path: Submission is on structured_submission, NOT in
    // content (content stays empty for control commands).
    assert_eq!(msg.content, "");
    match msg.structured_submission.expect("sideband set") {
        Submission::ExecApproval {
            request_id,
            approved,
            ..
        } => {
            assert_eq!(request_id, req_id);
            assert!(approved);
        }
        other => panic!("expected ExecApproval, got {other:?}"),
    }
}

#[tokio::test]
async fn test_cancel_routes_through_to_inject_stream() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h5.sock");
    let (_chan, _sse, mut stream) = spawn_channel_with_stream(path.clone()).await;

    let client = UnixStream::connect(&path).await.unwrap();
    let (client_r, mut client_w) = client.into_split();
    let mut reader = BufReader::new(client_r);
    let mut hello = String::new();
    reader.read_line(&mut hello).await.unwrap();

    client_w.write_all(b"{\"type\":\"cancel\"}\n").await.unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("stream timeout")
        .expect("stream ended");
    assert!(matches!(
        msg.structured_submission.expect("sideband set"),
        Submission::Interrupt
    ));
}
```

- [ ] **Step 2: Run**

```bash
cargo test --features integration --test local_ipc_integration
```

Expected: 5 passed.

- [ ] **Step 3: Commit**

```bash
git add tests/local_ipc_integration.rs
git commit -m "$(cat <<'EOF'
test(local_ipc): integration tests #4-#5 — approval + cancel callers

Drives the call site (Channel::start → reader task → inject_tx →
MessageStream) instead of unit-testing build_control_submission alone.
Required by testing.md "Test Through the Caller, Not Just the Helper":
the helper has multiple inputs (cmd, user_id, client_id) and gates a
side effect (injecting into the agent loop), so a unit test on the
helper is insufficient regression coverage.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task H4: Test #6 — Message metadata.client_id

**Files:**
- Modify: `tests/local_ipc_integration.rs`

- [ ] **Step 1: Append**

```rust
#[tokio::test]
async fn test_message_carries_client_id_metadata() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h6.sock");
    let (_chan, _sse, mut stream) = spawn_channel_with_stream(path.clone()).await;
    let client = UnixStream::connect(&path).await.unwrap();
    let (client_r, mut client_w) = client.into_split();
    let mut reader = BufReader::new(client_r);
    let mut hello = String::new();
    reader.read_line(&mut hello).await.unwrap();
    client_w
        .write_all(b"{\"type\":\"message\",\"content\":\"hi\"}\n")
        .await
        .unwrap();
    let msg = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(msg.content, "hi");
    // metadata is serde_json::Value (not Option). Direct indexing.
    let cid = msg.metadata["client_id"].as_str().expect("client_id string");
    assert!(cid.starts_with("ipc-"), "got client_id={cid}");
}
```

- [ ] **Step 2: Run + commit**

```bash
cargo test --features integration --test local_ipc_integration
```

Expected: 6 passed.

```bash
git add tests/local_ipc_integration.rs
git commit -m "$(cat <<'EOF'
test(local_ipc): integration test #6 — Message → IncomingMessage metadata

Verifies the reader task tags the synthesized IncomingMessage with the
per-client id under metadata.client_id, which respond() looks up to
route the response back to the correct writer mpsc.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task H5: Tests #7-#10 — disconnect, malformed, cleanup, reconnect

**Files:**
- Modify: `tests/local_ipc_integration.rs`

- [ ] **Step 1: Append los cuatro**

```rust
#[tokio::test]
async fn test_client_disconnect_releases_resources() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h7.sock");
    let (_chan, _sse) = spawn_channel(path.clone()).await;
    {
        let client = UnixStream::connect(&path).await.unwrap();
        let mut reader = BufReader::new(client);
        let mut h = String::new();
        reader.read_line(&mut h).await.unwrap();
        // Drop reader → underlying stream closes → server reader sees EOF.
    }
    // Give the reader task a moment to wind down, then assert we can
    // still connect a new client successfully (no panic surfaced — test
    // would have aborted).
    tokio::time::sleep(Duration::from_millis(200)).await;
    let client2 = UnixStream::connect(&path).await.unwrap();
    let mut reader = BufReader::new(client2);
    let mut h2 = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut h2))
        .await
        .unwrap()
        .unwrap();
    assert!(h2.contains("ipc_hello"));
}

#[tokio::test]
async fn test_socket_file_cleanup_on_shutdown() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h8.sock");
    let sse = Arc::new(SseManager::new());
    let chan = LocalIpcChannel::new(path.clone(), "owner".into(), sse, 16);
    let _ = chan.start().await.unwrap();
    wait_for_bind(&path).await;
    assert!(path.exists());
    chan.shutdown().await.unwrap();
    // Listener consumes the shutdown notification and removes the file.
    for _ in 0..50 {
        if !path.exists() { break; }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    assert!(!path.exists(), "socket file must be removed on shutdown");
}

#[tokio::test]
async fn test_malformed_line_does_not_kill_session() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h9.sock");
    let (_chan, _sse, mut stream) = spawn_channel_with_stream(path.clone()).await;
    let client = UnixStream::connect(&path).await.unwrap();
    let (client_r, mut client_w) = client.into_split();
    let mut reader = BufReader::new(client_r);
    let mut h = String::new();
    reader.read_line(&mut h).await.unwrap();
    // Send garbage, then a valid command.
    client_w.write_all(b"this is not json\n").await.unwrap();
    client_w
        .write_all(b"{\"type\":\"message\",\"content\":\"after-garbage\"}\n")
        .await
        .unwrap();
    let msg = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("stream timeout")
        .expect("stream ended");
    assert_eq!(msg.content, "after-garbage");
}

#[tokio::test]
async fn test_reconnect_after_client_drop_yields_fresh_hello() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h10.sock");
    let (_chan, _sse) = spawn_channel(path.clone()).await;
    {
        let c1 = UnixStream::connect(&path).await.unwrap();
        let mut r1 = BufReader::new(c1);
        let mut h = String::new();
        r1.read_line(&mut h).await.unwrap();
        assert!(h.contains("ipc_hello"));
    }
    // New connection — fresh hello.
    let c2 = UnixStream::connect(&path).await.unwrap();
    let mut r2 = BufReader::new(c2);
    let mut h = String::new();
    tokio::time::timeout(Duration::from_secs(2), r2.read_line(&mut h))
        .await
        .unwrap()
        .unwrap();
    assert!(h.contains("ipc_hello"));
}
```

- [ ] **Step 2: Run + commit**

```bash
cargo test --features integration --test local_ipc_integration
```

Expected: 10 passed.

```bash
git add tests/local_ipc_integration.rs
git commit -m "$(cat <<'EOF'
test(local_ipc): integration tests #7-#10 — failure modes

#7  Client drop releases resources (reconnect succeeds afterwards).
#8  shutdown() removes the socket file.
#9  Garbage line does not terminate the session; a follow-up valid
    Message still routes to the inject stream (silent-ok per the
    rule in error-handling.md).
#10 Reconnect after disconnect yields a fresh ipc_hello.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task H6: Test #11 — backpressure (slow client)

**Files:**
- Modify: `tests/local_ipc_integration.rs`

- [ ] **Step 1: Append**

```rust
#[tokio::test]
async fn test_slow_client_does_not_block_others() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h11.sock");
    let (_chan, sse) = spawn_channel(path.clone()).await;
    // Bind already awaited in spawn_channel/spawn_channel_with_stream.
    // Client A: never reads (slow). Client B: reads normally.
    let _slow = UnixStream::connect(&path).await.unwrap();
    let mut fast = BufReader::new(UnixStream::connect(&path).await.unwrap());
    drain_hello(&mut fast).await;

    // Push enough events to overflow the slow client's mpsc (cap 16
    // per spawn_channel) but well within the SseManager broadcast
    // buffer.
    for _ in 0..32 {
        sse.broadcast(AppEvent::Heartbeat);
    }
    // The fast client must still receive at least one event despite
    // the slow client falling behind.
    let mut line = String::new();
    let got = tokio::time::timeout(Duration::from_secs(2), fast.read_line(&mut line))
        .await;
    assert!(got.is_ok(), "fast client starved by slow client");
    assert!(line.contains("heartbeat"));
}
```

- [ ] **Step 2: Run + commit**

```bash
cargo test --features integration --test local_ipc_integration
```

Expected: 11 passed.

```bash
git add tests/local_ipc_integration.rs
git commit -m "$(cat <<'EOF'
test(local_ipc): integration test #11 — slow client does not block

Per-client mpsc isolates each writer's pace from the SseManager
broadcast bus. A subscriber that never reads from the socket only
fills its own 16-slot buffer; the broadcast tx drops the lagged
events for that subscriber and other clients keep flowing.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Track I · Validación final + smoke

### Task I1: Quality gate completo

**Files:** none (verificación)

- [ ] **Step 1: Format**

```bash
cargo fmt
git diff --stat
```

Expected: cero diff (o si hay diff, commit "style: cargo fmt").

- [ ] **Step 2: Clippy zero warnings**

```bash
cargo clippy --all --benches --tests --examples --all-features
```

Expected: `Finished` sin warnings. Si surgen, fix en sitio (no `#[allow(...)]` salvo justificación documentada en comentario adjacente).

- [ ] **Step 3: Test suite completa**

```bash
cargo test
cargo test --features integration
```

Expected: ambas verdes.

- [ ] **Step 4: Boundaries + safety scripts**

```bash
bash scripts/check-boundaries.sh
bash scripts/pre-commit-safety.sh
```

Expected: ambas verdes. Si `pre-commit-safety.sh` flaggea el `respond()` de `channel_impl.rs` por usar `state.sse.*` directamente — releer; el módulo NO usa `state.sse.broadcast`, solo `subscribe_raw`, y solo escribe al `tx: mpsc::Sender<AppEvent>` per-cliente. Si falsea como falso positivo, agregar el comentario `// projection-exempt: bridge dispatcher, local IPC writer fan-out` en la línea correspondiente.

- [ ] **Step 5: Verificación grep — gateway-events rule**

```bash
grep -rn "broadcast\|broadcast_for_user" src/channels/local_ipc/
```

Expected: cero matches a `SseManager::broadcast` o `SseManager::broadcast_for_user`. Solo `subscribe_raw`. Si aparece `broadcast`, es un bug que viola `gateway-events.md`.

- [ ] **Step 6: Verificación grep — silent-failure anti-patterns**

```bash
grep -nE "unwrap_or_default\(\)|\.ok\(\)\?|let Ok\(.*\) = .* else \{ return" src/channels/local_ipc/
```

Expected: cero matches sin un `// silent-ok: <reason>` comment en la misma línea. Los dos `// silent-ok` ya documentados son los `continue` por línea vacía/malformada en el reader task.

- [ ] **Step 7: Commit del fix-up si hubo edits**

(Si pasos 1-6 introdujeron edits, commit "chore: post-quality-gate cleanup".)

---

### Task I2: Smoke manual en una shell

**Files:** none (validación humana)

Este paso requiere correr IronClaw en una shell. NO es CI. Si el agente no puede ejecutar interactivo, dejar como TODO en el handoff al usuario.

- [ ] **Step 1: Limpiar runtime previo**

```bash
rm -f /run/user/$(id -u)/ironclaw.sock
```

- [ ] **Step 2: Levantar IronClaw**

```bash
cargo run --release -- run
```

Espera a ver el log line `local_ipc channel enabled (writer_buffer=...)`.

- [ ] **Step 3: Conectar con socat**

En otra shell:

```bash
socat -u UNIX-CONNECT:/run/user/$(id -u)/ironclaw.sock -
```

Expected: la primera línea es `{"type":"ipc_hello","protocol_version":1,"local_user_id":"<owner>"}`.

- [ ] **Step 4: Mandar un mensaje**

En otra shell:

```bash
echo '{"type":"message","content":"hola"}' | socat - UNIX-CONNECT:/run/user/$(id -u)/ironclaw.sock
```

Expected: el agente procesa el turno; la primera shell (la que está leyendo) muestra `thinking`, `tool_started`/`tool_completed` si aplica, y `response`.

- [ ] **Step 5: Verificar que NO hay daemons huérfanos**

```bash
systemctl --user status jarvis-ui-bridge 2>&1 | grep -i "could not be found\|not loaded"
```

Expected: confirma que la unit ya no existe.

- [ ] **Step 6: Documentar el resultado en el commit final**

(Sin commit propio; este es un acceptance check. Si pasa todo, el plan está completo.)

---

### Task I3: Update spec con notas implementativas

**Files:**
- Modify: `docs/superpowers/specs/2026-04-30-jarvis-os-local-ipc-design.md`

El spec mencionaba `gate_manager` y `cancel_handle` que no existen como tipos. Añadir nota al final.

- [ ] **Step 1: Append a la sección 14 ("Decisiones pendientes")**

Después del último item, añade:

```markdown
- **Implementación real, post-spec (2026-05-01):** durante la fase de plan se descubrieron cuatro divergencias respecto al spec original. Todas se resolvieron en `docs/superpowers/plans/2026-05-01-jarvis-os-local-ipc.md` y se cristalizan así:
  1. Los tipos `GateManager` y `CancelHandle` mencionados en §4.2 NO existen en el repo. Approval/Cancel usan el sideband tipado `IncomingMessage::with_structured_submission(Submission)` (no JSON-en-content como hace el web channel legacy), inyectado por el mismo `MessageStream` que `Channel::start()` devuelve.
  2. El SseManager NO se hoistea antes del bloque del gateway. `GatewayChannel::new()` construye uno propio internamente sin permitir inyección, y el agent loop's `sse_tx` se alimenta de ése. Por tanto el bloque local_ipc se inserta DESPUÉS del gateway, reusando `gw.state().sse`; cuando el gateway está apagado, materializa uno propio y lo asigna a `sse_manager` para que el agent loop tenga bus.
  3. `AppEvent::Response.thread_id` es `String` (no `Option`); `OutgoingResponse.thread_id` es `Option<ExternalThreadId>`. La conversión es explícita en `build_response_event` (`.map(\|t\| t.as_str().to_string()).unwrap_or_default()`).
  4. Los eventos sintéticos `ipc_hello` + `error` (transport-only) viajan por el mismo writer mpsc que los `AppEvent` mediante un envelope `WireMessage = App(AppEvent) | Transport(TransportEvent)`. Esto permite que los `error` events del §5.1 efectivamente lleguen al cliente cuando una línea es malformada, en vez de solo loggearlos.
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/specs/2026-04-30-jarvis-os-local-ipc-design.md
git commit -m "$(cat <<'EOF'
docs(spec): note four post-plan implementation refinements

The spec referenced types and SseManager wiring that don't match the
codebase as-is. Plan-time review surfaced four divergences:

  1. Approval/Cancel via with_structured_submission sideband, not the
     legacy JSON-in-content pattern.
  2. Local IPC reuses gateway's SseManager (the gateway constructs it
     internally; can't inject).
  3. AppEvent::Response.thread_id is String, not Option — explicit
     conversion from OutgoingResponse.
  4. Transport ipc_hello + error events ride the same writer mpsc as
     AppEvent via a WireMessage envelope so errors actually reach the
     client.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-review

**Spec coverage check** (mapeo §-by-§ del spec a tareas):

| Spec § | Cobertura |
|---|---|
| §4.1 Estructura del módulo | Tracks A, B, C, D, E |
| §4.2 mod.rs / socket.rs / client.rs / protocol.rs / channel_impl.rs / control.rs / error.rs | A2, B1-B4, C1-C2, D1-D2, D3, E1-E2 |
| §4.3 Lo que NO hace el módulo | Asegurado por I1 Step 5 (grep gateway-events.md) y I1 Step 6 (silent-failure) |
| §5.1 Encoding (UTF-8, NDJSON, 64KiB cap) | D2 reader task `MAX_LINE_BYTES`, escapa el cap; smoke I2 |
| §5.2 Server→Client (AppEvent + ipc_hello + error) | B4, D2 writer task |
| §5.3 Client→Server commands | B3, D2 reader task |
| §5.4 Versionado | B4 `PROTOCOL_VERSION` |
| §6 Errores + sanitización | A2, D2 (transport-error sanitizado en `emit_transport_error`, no expone paths) |
| §7.1 Path resolution | C1 |
| §7.2 Bring-up sequence (cleanup + bind + chmod) | C2, D3, E2 |
| §7.3 Shutdown | D3 listener select! + remove_file; E1 shutdown() notify |
| §7.4 Discovery vs activación | E2 `create()` retorna fatal sin retry, no discovery loop |
| §7.5 Discovery del path por clientes | C1 helper público en mod.rs |
| §8.1 Envvars | F1 |
| §8.2 Bounded resources | D2 `MAX_LINE_BYTES`, D3 `SOFT_CLIENT_CAP`/`HARD_CLIENT_CAP`, F1 buffer |
| §9 Data flow | Cubierto end-to-end por H4 (Message), H3 (Approval/Cancel), H2 (multi-cliente) |
| §10 Borrado de código basura | Track G entera |
| §11 Testing | Tracks B+C+D unit, Track H integration |
| §11.3 Test through the caller | H3 (Approval/Cancel drive el call site, no helper aislado) |
| §12 Reglas aplicadas | I1 verifica clippy + boundaries + safety + grep gateway-events |
| §13 Criterios de aceptación | I1 (1-6, 9-12) + I2 (7-8) |

**Placeholder scan:** ningún paso usa "TBD" / "implement later" / "fill in details". Las verificaciones que sobreviven al ejecutor (E1 Step 2 y D2 Step 2) ya pre-verifican los tipos reales del codebase y citan línea exacta — el `grep` es defensivo, no especulativo. No queda ningún `// ajustar si difiere` para tipos.

**Type consistency:** `ClientId` newtype consistente en `ClientHandle`, reader task, `dispatch_command`, `ClientMap` key. `WireMessage = App(AppEvent) | Transport(TransportEvent)` es el envelope único del writer mpsc, así `respond()`, `broadcast()` y `emit_transport_error()` comparten un único `write_wire()`. `Submission::ExecApproval`/`Interrupt` espejea `src/agent/submission.rs:296,328`. `IncomingMessage::new + with_structured_submission + with_metadata` es el patrón tipado del sideband (`src/channels/channel.rs:133, 251, 239`).

**Real-codebase signatures usadas (verificadas al escribir el plan, no asumidas):**

- `ChannelError::StartupFailed { name, reason }` y `HealthCheckFailed { name }` — struct variants. `src/error.rs:117,141`. NO existen `Unhealthy(String)` ni `Io(io::Error)`; los IO errors colapsan a `StartupFailed` con la string del error.
- `AppEvent::Response { content: String, thread_id: String }` — `thread_id` es plain String, NO `Option`. `crates/ironclaw_common/src/event.rs:203`. La conversión desde `OutgoingResponse.thread_id: Option<ExternalThreadId>` se hace explícitamente con `.map(\|t\| t.as_str().to_string()).unwrap_or_default()` en `LocalIpcChannel::build_response_event`.
- `IncomingMessage.metadata: serde_json::Value` directo (default `Value::Null`), NO `Option<Value>`. Acceso vía `msg.metadata["client_id"]` o `msg.metadata.get(...).and_then(...)`.
- `GatewayChannel::new()` no acepta SseManager externo (construye uno propio en `src/channels/web/mod.rs:149-152`). F2 está estructurada para insertarse DESPUÉS del gateway block y reusar `gw.state().sse`, materializando uno propio solo cuando el gateway está apagado.

**Spec → no-task gaps:** ninguno detectado. La única deviation explícita está en las "Notas implementativas" del header del plan y se commitea de vuelta al spec en I3.

---

## Plan complete and saved to `docs/superpowers/plans/2026-05-01-jarvis-os-local-ipc.md`.
