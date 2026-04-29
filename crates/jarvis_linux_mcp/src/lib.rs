//! # jarvis_linux_mcp
//!
//! Servidor MCP nativo para jarvis-os. Expone capacidades del sistema
//! operativo Linux (systemd, polkit, D-Bus, btrfs, AT-SPI2) como tools
//! "host-trusted" — fuera del sandbox WASM de IronClaw, bajo políticas
//! estrictas de `jarvis_policies`.
//!
//! ## Arquitectura
//!
//! - **Adapters** (`adapter::*`): wrappers async sobre las APIs nativas
//!   (zbus para D-Bus, ioctls para btrfs, etc.). Sin lógica de negocio.
//! - **Tools** (`tools::*`): cada una implementa `Tool` y consume uno o
//!   varios adapters. Son el contrato hacia el cliente MCP (IronClaw).
//! - **Registry** (`tool::ToolRegistry`): inscribe las tools al arrancar
//!   el servidor para servirlas vía `tools/list`.
//! - **Server** (F1.2.b): traduce protocolo MCP (JSON-RPC sobre stdio o
//!   HTTP) a llamadas al registry, aplicando `jarvis_policies` antes de
//!   cada `invoke`.
//!
//! ## Política de seguridad
//!
//! Las tools NO comprueban política. El dispatcher (futuro `server.rs`)
//! consulta `jarvis_policies::PolicyEngine::evaluate` antes de llamar a
//! `Tool::invoke`. Si la decisión es `Decision::Deny`, devuelve error MCP
//! sin tocar el adapter. Si es `Decision::Confirm`, espera al HUD/usuario.
//! Solo si es `Decision::Allow` (o el usuario confirmó) se invoca la tool.

pub mod adapter;
pub mod error;
pub mod mcp_server;
pub mod tool;
pub mod tools;

pub use error::{Error, Result};
pub use tool::{Tool, ToolMetadata, ToolOutput, ToolRegistry};
