# jarvis-os Local IPC — Spec

**Fecha:** 2026-04-30
**Autor:** Horelvis Castillo Mendoza (con asistencia de Claude)
**Branch objetivo:** `jarvis-arch-os`
**Reemplaza a:** el wrapper `crates/jarvis_ui_bridge/` y su unidad systemd

## 1. Contexto y motivación

jarvis-os corre en la misma máquina que IronClaw. Hoy la UI Quickshell consume eventos del core a través de un bridge (`crates/jarvis_ui_bridge/`) que hace `WS gateway → UNIX socket NDJSON`. El gateway HTTP/WS está diseñado para acceso remoto multi-cliente (auth tokens, CORS, tunnel providers, multi-tenant) — meterlo entre dos procesos del mismo host añade superficie de configuración (env vs DB vs TOML, tokens, ports, feature flags) que ya causó un bug bloqueante el 2026-04-30 (gateway no levanta en Asus pese a flags OK).

La solución estructural es un **UNIX socket nativo del core IronClaw** — IPC local sin auth (permisos del filesystem), sin tokens, sin tunnel — para que los procesos del lado-Jarvis (voice daemon + Quickshell UI) consuman eventos y manden comandos sin pasar por el gateway. El gateway HTTP/WS sigue vivo y disponible para acceso remoto, web channel, mobile.

**Definición arquitectónica de fondo (no debate):** Jarvis es la voz y oídos de IronClaw. `jarvis_voice_daemon` (STT/TTS vía ElevenLabs Convai) y Quickshell UI son la interfaz humano-máquina exclusiva del usuario en jarvis-os. Ambos son procesos del lado-Jarvis y ambos son clientes locales del IPC del core.

## 2. Objetivos

1. Eliminar la dependencia del gateway HTTP/WS para uso local.
2. Permitir múltiples clientes locales concurrentes (voice daemon + Quickshell UI hoy; otros mañana sin cambios al server).
3. Soportar bidireccionalidad: eventos del core hacia los clientes y comandos de los clientes al core (`Message`, `Approval`, `Cancel`, `Ping`).
4. Reusar la maquinaria de canales existente — el módulo implementa el `Channel` trait, queda ortogonal con CLI, HTTP webhook, web gateway, telegram, signal.
5. Eliminar el código que la nueva arquitectura deja obsoleto en el mismo cambio.

## 3. No-objetivos

- Reemplazar el gateway HTTP/WS. Sigue vivo para acceso remoto.
- Soportar `AuthSubmit` de tokens OAuth desde el IPC. Web onboarding sigue siendo el path para esos flujos.
- Replay/resume de eventos perdidos durante una caída del server. Eventos in-flight se pierden; la DB sigue siendo el source of truth para auditoría.
- Multi-tenant local. Single-user implícito, resuelto al startup.
- Acceso remoto. Si en el futuro entra mobile/web remoto, va por el gateway HTTP/WS, no por aquí.
- Protocolo binario. NDJSON debug-friendly es suficiente.

## 4. Arquitectura

### 4.1 Posición en el repo

Nuevo módulo en `src/channels/local_ipc/` (no crate aparte; código local del core, no se reusa fuera). Implementa el `Channel` trait existente en `src/channels/channel.rs:865-930`.

Estructura:

```
src/channels/local_ipc/
├── mod.rs           ~80 LoC    LocalIpcChannel struct + create() factory
├── socket.rs        ~120 LoC   UnixListener, accept loop, cleanup del socket file
├── client.rs        ~150 LoC   ClientSession: read commands, write events
├── protocol.rs      ~80 LoC    ClientCommand enum, ClientId newtype, IpcErrorKind enum
├── channel_impl.rs  ~120 LoC   impl Channel for LocalIpcChannel
├── control.rs       ~80 LoC    process_control_command(): Approval, Cancel, Ping
└── error.rs         ~40 LoC    LocalIpcError (thiserror)
```

Total estimado: ~670 LoC. Si crece más, es señal de que el módulo está haciendo demasiado.

### 4.2 Componentes y responsabilidades

**`mod.rs`** — superficie pública mínima.
- `pub async fn create(ctx: Arc<AppCtx>) -> Result<Option<LocalIpcChannel>, LocalIpcError>`. Retorna `Ok(None)` si `IRONCLAW_LOCAL_SOCKET=disabled`.
- `LocalIpcChannel` struct contiene `path: PathBuf`, `local_user_id: UserId`, `sse_subscriber: SseManagerHandle`, `gate_manager: Arc<GateManager>`, `cancel_handle: CancelHandle`, `clients: Arc<RwLock<HashMap<ClientId, ClientHandle>>>`.

**`socket.rs`** — listener.
- `pub async fn run_listener(...)` con `select!` entre `listener.accept()` y `shutdown.notified()`.
- Cleanup de socket huérfano al startup: si existe, intentar `connect()` con timeout 100ms — si responde, otro IronClaw vivo, abortar; si no, `unlink()`.
- `chmod 0600` post-bind.

**`client.rs`** — sesión per-cliente.
- `pub async fn handle(stream: UnixStream, ctx, channel: Arc<LocalIpcChannel>)`.
- Splittea en read/write halves, spawnea reader y writer tasks.
- Reader: `BufReader` line-by-line, parse `ClientCommand`, despacho según variante.
- Writer: `mpsc::Receiver<AppEvent>` cap 256, también suscribe `SseManager.broadcast` filtrando por `local_user_id`. Serializa NDJSON.
- Drop: al cerrar la conexión, remueve `ClientId` del map.

**`protocol.rs`** — tipos del wire.
- `ClientId` newtype con `validate()` y `::new()` (regla `types.md`).
- `ClientCommand` enum con `#[serde(tag = "type", rename_all = "snake_case")]`.
- `ApprovalAction` enum.
- `IpcErrorKind` enum wire-stable (CommandInvalid, RateLimit, InternalError, CommandTooLarge).
- `IpcHello` struct para el evento de handshake.

**`channel_impl.rs`** — `Channel` trait.
- `name()` → `"local_ipc"`.
- `start()` → spawn del listener, devuelve `MessageStream` que solo emite `IncomingMessage`s convertidos desde `ClientCommand::Message`. Comandos de control (Approval/Cancel/Ping) los maneja el reader internamente sin cruzar el stream.
- `respond(msg, resp)` → busca `ClientId` en `msg.metadata`, envía al writer correspondiente. Si el cliente desapareció, log warn + Ok.
- `send_status(status, metadata)` → mismo path que respond, identificación por metadata. **Nota**: el writer también suscribe al SseManager directo, así que `send_status` es redundante para clientes ya conectados; lo implementamos como safety net y para consistencia con el trait.
- `broadcast(user_id, resp)` → si `user_id == local_user_id`, fan-out a todos los writers conectados.
- `health_check()` → verifica que el listener task sigue vivo.
- `shutdown()` → notify shutdown, `unlink()` socket file.

**`control.rs`** — comandos no-mensaje.
- `process_control_command(cmd, ctx) -> Result<(), LocalIpcError>` para `Approval` y `Cancel`.
- `Approval` → `ctx.gate_manager.resolve(request_id, action).await`.
- `Cancel` → `ctx.cancel_handle.cancel(step_id).await`.
- `Ping` → no-op (gestionado en `client.rs` directamente, no llega aquí).

**`error.rs`** — errores tipados.
- `LocalIpcError` con `thiserror`. Variantes documentadas en sección 6.

### 4.3 Lo que NO hace este módulo

- No autentica clientes (single-user implícito por permisos POSIX del socket).
- No rate-limita (cliente local de confianza).
- No expone tools/skills/memory como API. El `ToolDispatcher` ya hace esa función; el IPC sólo enruta `Message`/`Approval`/`Cancel`/`Ping`.
- No replaya histórico al reconectar.
- No llama `SseManager::broadcast` (regla `gateway-events.md`). Sólo subscribe.

### 4.4 Diagrama mental

```
voice daemon ──┐
               ├─► UNIX socket NDJSON ─► src/channels/local_ipc/ ─► ChannelManager
Quickshell UI ─┘                                                  └─► SseManager (subscribe)

(remote/web/mobile) ──► HTTPS/WSS ──► src/channels/web/ ──► (mismo ChannelManager + SseManager)
```

Dos puertas a la misma casa. Una para vecinos del piso (local), otra para visitas (remote). Ambas llegan a la misma sala — `ChannelManager` y `SseManager` no saben ni les importa la diferencia.

## 5. Protocolo wire

### 5.1 Encoding y framing

- UTF-8, una línea por mensaje, terminada en `\n`. Sin BOM.
- Líneas vacías ignoradas. Líneas malformadas → log warn + skip + emit `error` event con `kind: command_invalid`.
- Cap de 64 KiB por línea. Líneas más largas → emit `error` con `kind: command_too_large`, conexión sigue.

### 5.2 Server → Client (eventos)

Reuso directo del `AppEvent` ya existente en `crates/ironclaw_common/src/event.rs` con `#[serde(tag = "type", rename_all = "snake_case")]`. Las 30+ variantes salen tal cual.

Eventos sintéticos del transporte (no son `AppEvent` del core):

- `ipc_hello` — emitido UNA VEZ apenas el cliente conecta, antes de cualquier otro evento. Lleva `protocol_version: u32` y `local_user_id: String`.
- `error` — emitido cuando un comando del cliente falla validación. Lleva `kind: IpcErrorKind` y `detail: String` (sanitizado, nunca paths absolutos).

Filtrado por `local_user_id`: el writer task descarta `ScopedEvent` cuyo `user_id == Some(other)`. Eventos sin scope (heartbeat global, system) pasan a todos.

### 5.3 Client → Server (comandos)

`ClientCommand` con `#[serde(tag = "type", rename_all = "snake_case")]`:

```rust
pub enum ClientCommand {
    Message { content: String, thread_id: Option<ThreadId> },
    Approval { request_id: String, action: ApprovalAction },
    Cancel { step_id: Option<String> },
    Ping,
}

pub enum ApprovalAction {
    Approve,
    Deny,
}
```

Reglas:

- `message.content` máx 64 KiB.
- `message.thread_id` opcional. `null` o ausente = thread por defecto / nuevo según engine.
- `cancel.step_id` opcional. Sin step_id = cancelar lo que esté en curso para `local_user_id`.
- Ningún comando lleva `user_id`. El server lo resolvió al startup; el cliente no puede suplantar.

### 5.4 Versionado

`ipc_hello.protocol_version: u32` empieza en `1`. Cambios compatibles (añadir variantes de `AppEvent`, añadir campos opcionales) no incrementan. Cambios breaking bumpean a `2`.

## 6. Errores

```rust
#[derive(Debug, thiserror::Error)]
pub enum LocalIpcError {
    #[error("socket bind failed at {path}: {reason}")]
    BindFailed { path: PathBuf, reason: String },
    #[error("another IronClaw instance owns the socket at {path}")]
    SocketBusy { path: PathBuf },
    #[error("socket file at {path} could not be cleaned up: {reason}")]
    CleanupFailed { path: PathBuf, reason: String },
    #[error("unable to resolve local user id: {reason}")]
    LocalUserResolve { reason: String },
    #[error("client {client_id} command parse error: {reason}")]
    CommandParse { client_id: ClientId, reason: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
```

`impl From<LocalIpcError> for ChannelError` colapsa errores no-recuperables a `StartupFailed` o `SendFailed`. Mensajes que cruzan al cliente vía evento `error` son sanitizados — nunca paths absolutos, traceback, ni nombres de archivos internos (regla `error-handling.md`).

**Anti-patterns prohibidos**:

- Cero `.unwrap_or_default()` sobre `Result` en este módulo.
- Cero `let Ok(x) = ... else { return None }` que silencien errores de bind o de DB.
- Errores de parse de líneas → log + skip está permitido, anotado con `// silent-ok: malformed line, continue session`.

## 7. Lifecycle

### 7.1 Path del socket

Resolución en orden:

1. `IRONCLAW_LOCAL_SOCKET=/path/explicit` — usado tal cual. Valor `disabled` desactiva el módulo.
2. `$XDG_RUNTIME_DIR/ironclaw.sock` (típicamente `/run/user/<uid>/ironclaw.sock`) si la env existe.
3. Fallback: `~/.ironclaw/ironclaw.sock`.

Permisos `0600` aplicados con `chmod` post-bind.

### 7.2 Bring-up

En `app.rs`, durante el bootstrap, **antes** de `channel_manager.start_all()`:

```rust
if let Some(local_ipc) = local_ipc::create(&ctx).await? {
    channel_manager.add(Box::new(local_ipc)).await;
}
```

`create()`:
1. Resuelve `local_user_id` leyendo de la DB el primary user / single-user config.
2. Resuelve path del socket (sección 7.1).
3. Si `disabled`, retorna `Ok(None)`.
4. Limpia socket file huérfano (connect-test → unlink si no responde).
5. Bind del listener.
6. `chmod 0600`.
7. Construye `LocalIpcChannel`, retorna `Ok(Some(_))`.

`start()` del trait (llamado desde `start_all`) hace el accept loop y devuelve el `MessageStream`.

### 7.3 Shutdown

1. SIGTERM/SIGINT → `ChannelManager::shutdown_all()` → `LocalIpcChannel::shutdown()`.
2. `Notify::notify_waiters()` al accept loop.
3. Accept loop sale del `select!`.
4. Cada client task ve el shutdown y termina.
5. `fs::remove_file(socket_path)`.
6. Listener drop.

Timeout de 5 segundos. Tasks que no responden se abortan forzado.

### 7.4 Discovery vs activación (regla `lifecycle.md`)

`create()` es activación, no discovery. No hay paso "discover" separado. Si auth/setup falla (no se pudo resolver `local_user_id`), retorna error fatal — no reintentea en loop. Auth rejection es terminal hasta cambio de config (mismo principio que WASM/MCP channels).

### 7.5 Descubrimiento del path por los clientes

Convención fija. Cliente y server usan la misma resolución (sección 7.1). Helper `resolve_socket_path()` que vive en `src/channels/local_ipc/socket.rs` se exporta como `pub` para que voice_daemon y otros consumidores lo importen y compartan la lógica de resolución. Cero archivos de coordinación.

## 8. Configuración

### 8.1 Envvars

| Envvar | Default | Propósito |
|---|---|---|
| `IRONCLAW_LOCAL_SOCKET` | (no set → resolver automático) | Path explícito del socket o `disabled` |
| `IRONCLAW_LOCAL_IPC_BUFFER` | `256` | Capacidad mpsc del writer task por cliente |

Documentadas en `.env.example` con nota: "Para uso local; gateway HTTP/WS sigue siendo independiente".

### 8.2 Bounded resources (regla `safety-and-sandbox.md`)

- Línea de comando: cap 64 KiB.
- Buffer mpsc por cliente: 256 (configurable por env).
- Clientes simultáneos: cap suave 32 (warn pero accept), hard cap 256 (reject + close).
- Tasks por cliente: 2 (reader + writer). 32 × 2 = 64 max.

## 9. Data flow (resumen)

### 9.1 Escenario base — voice daemon hace una pregunta

1. Voice daemon → ElevenLabs Convai → STT → "qué tengo en la agenda".
2. Voice daemon escribe `{"type":"message","content":"..."}` al socket.
3. Reader task parsea → convierte a `IncomingMessage { channel: "local_ipc", metadata: { client_id }, ... }`.
4. Reader empuja al `MessageStream` que devolvió `start()`.
5. `ChannelManager::start_all` merge de streams → agent loop recoge.
6. Agente procesa, emite `AppEvent::Thinking/ToolStarted/ToolCompleted` → `SseManager.broadcast`.
7. Writer tasks de TODOS los clientes IPC (filtrados por `local_user_id`) los reciben y escriben NDJSON.
8. Agente produce `AppEvent::Response` → ChannelManager + SseManager broadcast.
9. Voice daemon lee `response`, lo manda a Convai TTS, suena por speakers, ring reacciona.

### 9.2 Quickshell en paralelo

Quickshell es OTRO cliente del mismo socket. Su writer task suscribe al mismo `SseManager.broadcast`. Recibe TODOS los eventos relevantes para `local_user_id`, no solo los de mensajes que originó. HUD orbe pulsa, widgets reaccionan, notificaciones de heartbeat aparecen.

### 9.3 Aprobación de gate

1. Tool requiere gate → `AppEvent::GateRequired { request_id }` al broadcast.
2. Quickshell muestra modal de aprobación.
3. Usuario click "approve" → Quickshell escribe `{"type":"approval","request_id":"...","action":"approve"}`.
4. Reader task parsea → `control::process_approval` → `gate_manager.resolve()`.
5. Agente sigue, emite `AppEvent::GateResolved + ToolCompleted + Response`.
6. Todos los clientes (incluyendo voice daemon que NO inició la aprobación) reciben los eventos posteriores. Quickshell cierra el modal automáticamente.

### 9.4 Casos de fallo

- **Cliente desconecta a mitad**: reader/writer tasks terminan, sesión drop, recursos liberados. Si el agente intenta `respond()` después, busca el client_id, no lo encuentra, log warn + Ok.
- **Server cae**: cliente lee EOF, reconecta con backoff. Mismo patrón que el bridge actual contra WS.
- **Socket file huérfano**: bind falla con EADDRINUSE → connect-test → unlink si no responde.

## 10. Borrado de código basura

Lista exacta de lo que se elimina (regla `feedback_no_dead_code`):

**Crate completo:**
- `crates/jarvis_ui_bridge/` — todo el directorio (Cargo.toml, src/main.rs, src/gateway.rs, src/socket.rs, src/error.rs, tests, README si existe).
- Entrada `members` correspondiente en el `Cargo.toml` workspace.

**Systemd unit:**
- `arch/systemd-user/jarvis-ui-bridge.service`.

**QML EventBus** (modificado, no borrado):
- `ui/jarvis-os/core/EventBus.qml` — la línea de `Quickshell.Io.Process` que invoca `socat` cambia de `/run/user/<uid>/jarvis-ui-bridge.sock` a `$XDG_RUNTIME_DIR/ironclaw.sock`.

**Documentación:**
- Cualquier sección sobre el bridge en `arch/recipes/jarvis-os.md` o equivalentes.
- Bridge mencionado en docs M7-M8 del spec v0.3 — añadir nota de superseded.

**Envvars en `.env.example`:**
- Eliminar `JARVIS_UI_BRIDGE_*` si existieran.
- **NO** eliminar `GATEWAY_AUTH_TOKEN` ni `GATEWAY_ENABLED` — siguen vivos para uso remoto.

**Voice daemon** (`crates/jarvis_voice_daemon/`):
- Si el voice daemon hablaba al gateway WS o al bridge, reapuntar al socket UNIX. Verificar al implementar; reescritura trivial (mismo NDJSON, distinto transporte).

**Lo que NO se borra (lista explícita):**
- `crates/ironclaw_gateway/` — sigue vivo para acceso remoto.
- `src/channels/web/` — sigue siendo el web channel.
- `GatewayChannel`, WS handler, REST endpoints, tunnel providers — todos vivos.
- `SseManager` — el IPC local es OTRO consumidor, no su reemplazo.

## 11. Testing

### 11.1 Unit (tier 1, `cargo test`)

- Round-trip serde de `ClientCommand` para cada variante.
- `ClientId::new()` valid/invalid.
- `IpcErrorKind` snake_case wire format (regla `types.md`).
- `resolve_socket_path()` con env override, XDG, fallback (usar `tempfile`).
- `cleanup_orphan_socket()` cuando existe pero no responde.

### 11.2 Integration (tier 2, `cargo test --features integration`)

`tests/local_ipc_integration.rs` cubre:

1. **Bind + connect end-to-end**: levanta channel, conecta, recibe `ipc_hello`, manda `ping`.
2. **Múltiples clientes ven el mismo evento**: dos clientes conectados, broadcast del SseManager, ambos reciben.
3. **Filtrado por user_id**: scoped event de otro user_id → cliente local NO lo recibe.
4. **`Approval` desbloquea gate manager** (test through the caller, regla `testing.md`).
5. **`Cancel` propaga al cancel_handle**.
6. **`Message` empuja al inject channel**: cliente manda message, MessageStream emite IncomingMessage con metadata.client_id correcta.
7. **Cliente desconecta a mitad**: server detecta cierre, libera recursos, no panickea, no leak.
8. **Socket file cleanup**: shutdown elimina el socket file.
9. **Server caído + reconexión**: cliente recibe EOF, reconecta, recibe nuevo `ipc_hello`.
10. **Comando malformado no mata conexión**: línea no-JSON → log warn, siguiente línea procesada normal.
11. **Backpressure**: cliente lento, broadcast no se bloquea, otros clientes no afectados.

### 11.3 Test through the caller (regla `testing.md`)

Helpers que gateían side effects + tests de su caller:

| Helper | Caller | Test |
|---|---|---|
| `process_approval(cmd)` → `gate_manager.resolve()` | `client::reader_task` | Test #4 |
| `process_cancel(cmd)` → `cancel_handle.cancel()` | `client::reader_task` | Test #5 |
| `Channel::respond()` → enrutar a writer correcto | `LocalIpcChannel` | Test que crea dos clientes, manda mensaje desde uno, verifica que `respond()` solo escribe al cliente originador |

### 11.4 E2E

Fuera de scope para v1. Se cubren con los integration tests Rust.

## 12. Reglas aplicadas

| Regla | Aplicación en este módulo |
|---|---|
| `types.md` | `ClientId` newtype, `ApprovalAction` enum, `IpcErrorKind` enum wire-stable. `ThreadId` newtype reusado para `message.thread_id`. |
| `error-handling.md` | Cero `unwrap_or_default()` sobre Result. Errores sanitizados antes de cruzar al cliente. |
| `gateway-events.md` | El módulo NO llama `SseManager::broadcast`. Solo subscribe. Verificación: `grep` debe retornar 0. |
| `lifecycle.md` | Discovery vs activación: `create()` es activación, sin retry loops por auth fail. |
| `tools.md` | Comandos del cliente que producen acciones (Message → engine, Approval → gate manager) van por las APIs sancionadas. No bypass al `ToolDispatcher` desde handlers — el IPC es channel, no handler. |
| `testing.md` | Test through the caller para Approval y Cancel. No mocks, real `gate_manager` y `cancel_handle`. |
| `safety-and-sandbox.md` | Bounded resources: cap de línea, cap de clientes, cap de buffer mpsc. |
| `feedback_local_ipc_not_gateway` | Razón principal del módulo. |
| `feedback_no_dead_code` | Sección 10 lista exacta de lo que se borra. |

## 13. Criterios de aceptación

1. `cargo fmt` sin diff.
2. `cargo clippy --all --benches --tests --examples --all-features` zero warnings.
3. `cargo test` pasa todo.
4. `cargo test --features integration` pasa los 11 integration tests de la sección 11.2.
5. `bash scripts/check-boundaries.sh` pasa.
6. `bash scripts/pre-commit-safety.sh` pasa.
7. Voice daemon en Asus se conecta al socket y recibe al menos un `ipc_hello`.
8. Quickshell UI en Asus vía `socat UNIX-CONNECT:$XDG_RUNTIME_DIR/ironclaw.sock -` muestra eventos cuando el agente procesa un mensaje desde la CLI.
9. `crates/jarvis_ui_bridge/` completamente eliminado del repo + del Cargo.toml workspace.
10. `arch/systemd-user/jarvis-ui-bridge.service` eliminado.
11. Tests caller-level para Approval y Cancel presentes.
12. Mapeo de errores no expone paths absolutos al cliente.

## 14. Decisiones pendientes (fuera de scope de este spec)

- **Convai como agente principal de IronClaw** (interpretación α/β/γ): si en el futuro Convai pasa a ser el agente principal, este IPC sigue siendo válido — lo que cambia es el conjunto de comandos que los clientes envían, no el transporte. NDJSON + comandos discretos toleran el cambio sin rediseño.
- **Soporte de comandos extendidos** (`AuthSubmit`, control UI bidireccional): v2 si surgen consumidores concretos.
- **Suscripciones filtradas por tipo de evento**: hoy el cliente recibe todo y filtra él. Si en v2 hay clientes de bajo recurso que necesitan filtrar server-side, se añade un comando `subscribe { event_types: [...] }`.
- **Implementación real, post-spec (2026-05-01):** durante la fase de plan se descubrieron cuatro divergencias respecto al spec original. Todas se resolvieron en `docs/superpowers/plans/2026-05-01-jarvis-os-local-ipc.md` y se cristalizan así:
  1. Los tipos `GateManager` y `CancelHandle` mencionados en §4.2 NO existen en el repo. Approval/Cancel usan el sideband tipado `IncomingMessage::with_structured_submission(Submission)` (no JSON-en-content como hace el web channel legacy), inyectado por el mismo `MessageStream` que `Channel::start()` devuelve.
  2. El SseManager NO se hoistea antes del bloque del gateway. `GatewayChannel::new()` construye uno propio internamente sin permitir inyección, y el agent loop's `sse_tx` se alimenta de ése. Por tanto el bloque local_ipc se inserta DESPUÉS del gateway, reusando `gw.state().sse`; cuando el gateway está apagado, materializa uno propio y lo asigna a `sse_manager` para que el agent loop tenga bus.
  3. `AppEvent::Response.thread_id` es `String` (no `Option`); `OutgoingResponse.thread_id` es `Option<ExternalThreadId>`. La conversión es explícita en `build_response_event` (`.map(|t| t.as_str().to_string()).unwrap_or_default()`).
  4. Los eventos sintéticos `ipc_hello` + `error` (transport-only) viajan por el mismo writer mpsc que los `AppEvent` mediante un envelope `WireMessage = App(AppEvent) | Transport(TransportEvent)`. Esto permite que los `error` events del §5.1 efectivamente lleguen al cliente cuando una línea es malformada, en vez de solo loggearlos.

## 15. Referencias

- `CLAUDE.md` § "Extension/Auth Invariants", § "Job State Machine", § "Module-owned initialization".
- `.claude/rules/types.md`, `error-handling.md`, `gateway-events.md`, `lifecycle.md`, `tools.md`, `testing.md`, `safety-and-sandbox.md`.
- Memorias: `feedback_local_ipc_not_gateway`, `feedback_no_dead_code`, `project_jarvis_role`.
- Spec v0.3 antecedente: `docs/superpowers/specs/2026-04-30-jarvis-os-v0.3-ui-architecture-design.md`.
- Resume 2026-04-30 con bug del gateway: `~/.claude/projects/-home-nexus-git-jarvis-os/memory/project_resume_2026_04_30.md`.
