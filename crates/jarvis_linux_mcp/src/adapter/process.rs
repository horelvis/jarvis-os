//! Adapter para enumerar y consultar procesos del sistema.
//!
//! Usa el crate `procfs` que lee directamente del VFS `/proc/*` expuesto
//! por el kernel — sin pasar por ps/top/pgrep userspace. Encaja con la
//! preferencia kernel-level: la fuente de verdad es el kernel, sin
//! procesos intermediarios que parsean y reinterpretan.
//!
//! Limitaciones:
//!   - Listar procesos NO se hace con eBPF/netlink (cn_proc es para
//!     EVENTOS de creación/destrucción, no para snapshot del estado).
//!   - Para "watch new processes" en F2+ usaremos cn_proc o eBPF.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Snapshot de estado de un proceso, simplificado para output al agente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: i32,
    pub ppid: i32,
    /// Nombre del binary (de `/proc/<pid>/comm`, max 16 chars).
    pub name: String,
    /// Cmdline completa (puede ser muy largo; agente lo usa para identificar).
    pub cmdline: String,
    pub state: String,
    pub user_id: u32,
    /// RSS en MB (Resident Set Size).
    pub memory_mb: u64,
    /// Tiempo de CPU acumulado en segundos.
    pub cpu_time_s: u64,
}

/// Criterios de ordenación de la lista de procesos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    Pid,
    /// RSS descendente (procesos que más memoria usan primero).
    Memory,
    /// Tiempo CPU acumulado descendente.
    Cpu,
    /// Nombre alfabético.
    Name,
}

impl Default for SortBy {
    fn default() -> Self {
        Self::Memory
    }
}

/// Adapter funcional sobre procfs. Sin estado interno (cada `list` es
/// snapshot fresh del kernel). `Copy` para poder pasarse a tareas
/// `spawn_blocking` sin Arc ni gymnastics.
#[derive(Debug, Clone, Copy)]
pub struct ProcessAdapter;

impl ProcessAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Lista procesos del sistema. `limit` cap el output para no inundar
    /// el contexto del LLM (default 25); `sort_by` ordena antes de truncar.
    pub fn list(&self, limit: usize, sort_by: SortBy) -> Result<Vec<ProcessInfo>> {
        let all = procfs::process::all_processes()
            .map_err(|e| Error::Internal(format!("procfs all_processes: {e}")))?;

        let page_size = procfs::page_size();

        let mut entries: Vec<ProcessInfo> = all
            .filter_map(|p| p.ok())
            .filter_map(|process| {
                // Algunos procesos desaparecen entre `all` y la lectura
                // de stat — los filtramos silenciosamente.
                let stat = process.stat().ok()?;
                let cmdline = process
                    .cmdline()
                    .ok()
                    .map(|parts| parts.join(" "))
                    .unwrap_or_default();
                let uid = process.uid().ok().unwrap_or(0);

                let memory_bytes = stat.rss * page_size;
                let cpu_time_s = (stat.utime + stat.stime) / procfs::ticks_per_second();

                Some(ProcessInfo {
                    pid: stat.pid,
                    ppid: stat.ppid,
                    name: stat.comm.clone(),
                    cmdline,
                    state: format!("{}", stat.state),
                    user_id: uid,
                    memory_mb: memory_bytes / (1024 * 1024),
                    cpu_time_s,
                })
            })
            .collect();

        // Sort según criterio.
        match sort_by {
            SortBy::Pid => entries.sort_by_key(|e| e.pid),
            SortBy::Memory => entries.sort_by(|a, b| b.memory_mb.cmp(&a.memory_mb)),
            SortBy::Cpu => entries.sort_by(|a, b| b.cpu_time_s.cmp(&a.cpu_time_s)),
            SortBy::Name => entries.sort_by(|a, b| a.name.cmp(&b.name)),
        }

        entries.truncate(limit);
        Ok(entries)
    }
}

impl Default for ProcessAdapter {
    fn default() -> Self {
        Self::new()
    }
}
