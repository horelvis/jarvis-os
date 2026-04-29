//! Adapter para consultar journald.
//!
//! v0 (F1.2 / v8 MVP): shell-out a `journalctl` con argumentos seguros.
//! v1 (próxima iteración): librería `libsystemd-rs` o `systemd-rs` para
//! lectura directa del binario de journald sin proceso intermedio,
//! alineado con preferencia kernel-level.
//!
//! Nota: shell-out a `journalctl` no es kernel-level pero es el camino
//! más directo para v0 — y journalctl ES el cliente oficial, no un wrapper
//! de terceros. Sustituible cuando el coste/valor de migrar a sd-journal
//! lo justifique.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

/// Una entrada del journal, simplificada para output al agente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub timestamp: String,
    pub priority: u8,
    pub unit: String,
    pub message: String,
    pub hostname: String,
    pub pid: Option<u32>,
}

/// Filtros de prioridad estándar syslog.
/// Mapeo a `journalctl --priority`:
///   0 emerg, 1 alert, 2 crit, 3 err, 4 warning, 5 notice, 6 info, 7 debug
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Emerg,
    Alert,
    Crit,
    Err,
    Warning,
    Notice,
    Info,
    Debug,
}

impl Priority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::Emerg => "emerg",
            Priority::Alert => "alert",
            Priority::Crit => "crit",
            Priority::Err => "err",
            Priority::Warning => "warning",
            Priority::Notice => "notice",
            Priority::Info => "info",
            Priority::Debug => "debug",
        }
    }
}

/// Adapter funcional sobre journalctl.
#[derive(Debug, Clone, Copy, Default)]
pub struct JournalAdapter;

impl JournalAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Consulta el journal con filtros opcionales.
    ///
    /// Args:
    ///   - `since`: rango temporal en sintaxis journalctl (`5min ago`, `1h ago`, `today`, `2026-04-28`).
    ///   - `priority`: filtra por nivel mínimo (`err` = err+crit+alert+emerg).
    ///   - `unit`: filtra por unidad systemd específica.
    ///   - `limit`: máximo de líneas a devolver.
    pub async fn query(
        &self,
        since: Option<&str>,
        priority: Option<Priority>,
        unit: Option<&str>,
        limit: usize,
    ) -> Result<Vec<JournalEntry>> {
        let mut cmd = Command::new("journalctl");
        cmd.arg("--output=json").arg("--no-pager").arg("--reverse");

        if let Some(s) = since {
            cmd.arg(format!("--since={s}"));
        }
        if let Some(p) = priority {
            cmd.arg(format!("--priority={}", p.as_str()));
        }
        if let Some(u) = unit {
            cmd.arg(format!("--unit={u}"));
        }
        cmd.arg(format!("--lines={limit}"));

        let output = cmd.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Internal(format!(
                "journalctl exited {}: {stderr}",
                output.status.code().unwrap_or(-1)
            )));
        }

        // journalctl --output=json emite UN objeto JSON por línea.
        let mut entries = Vec::new();
        for line in output.stdout.split(|b| *b == b'\n') {
            if line.is_empty() {
                continue;
            }
            let raw: serde_json::Value = serde_json::from_slice(line)?;
            entries.push(JournalEntry {
                timestamp: raw
                    .get("__REALTIME_TIMESTAMP")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                priority: raw
                    .get("PRIORITY")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(6), // info default
                unit: raw
                    .get("_SYSTEMD_UNIT")
                    .or_else(|| raw.get("UNIT"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                message: raw
                    .get("MESSAGE")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                hostname: raw
                    .get("_HOSTNAME")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                pid: raw
                    .get("_PID")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok()),
            });
        }
        Ok(entries)
    }
}
