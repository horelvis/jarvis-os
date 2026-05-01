//! Adapter Btrfs vía CLI shell-out.
//!
//! v0 (F1.5 / v11 MVP): wrapping de `btrfs subvolume list/show/snapshot`.
//! v1 (F3+): bindings nativos vía ioctls (crate `btrfsutil-rs` o similar)
//! para acceso directo al kernel sin proceso intermediario, alineado con
//! preferencia kernel-level del proyecto.
//!
//! Limitación importante v11: el sistema vivo (live USB stateless) NO tiene
//! filesystem btrfs montado en disco. Los métodos funcionarán cuando jarvis-os
//! esté instalado en disco persistente con `/` o `/home` en btrfs.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subvolume {
    pub id: u64,
    pub path: String,
    /// btrfs "generation" (transaction id when subvol was modified).
    /// Renombrado de `gen` porque es reserved keyword en Rust 2024.
    pub generation: u64,
    pub parent_uuid: Option<String>,
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BtrfsAdapter;

impl BtrfsAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Lista subvolúmenes del filesystem btrfs montado en `mount_path`.
    /// Retorna lista vacía si el path no es btrfs (no es error fatal).
    pub async fn list(&self, mount_path: &str) -> Result<Vec<Subvolume>> {
        let output = Command::new("btrfs")
            .args(["subvolume", "list", "-pucq", mount_path])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Distingue "no es btrfs" de error real.
            if stderr.contains("not a btrfs filesystem") || stderr.contains("No such file") {
                return Ok(Vec::new());
            }
            return Err(Error::Internal(format!(
                "btrfs subvolume list exited {}: {stderr}",
                output.status.code().unwrap_or(-1)
            )));
        }

        // Parsea output tipo: ID 256 gen 234 cgen 5 parent 5 top level 5 parent_uuid - uuid abcd... path @home
        let stdout = String::from_utf8_lossy(&output.stdout);
        let subvols = stdout
            .lines()
            .filter_map(|line| parse_subvolume_line(line))
            .collect();
        Ok(subvols)
    }

    /// Crea un snapshot read-only de un subvolume.
    /// `source` es el path al subvolume origen, `dest` el path destino del snapshot.
    pub async fn snapshot_readonly(&self, source: &str, dest: &str) -> Result<()> {
        let output = Command::new("btrfs")
            .args(["subvolume", "snapshot", "-r", source, dest])
            .output()
            .await?;

        if !output.status.success() {
            return Err(Error::Internal(format!(
                "btrfs snapshot {} -> {}: {}",
                source,
                dest,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }

    /// Borra un subvolume (incluye snapshots).
    pub async fn delete(&self, path: &str) -> Result<()> {
        let output = Command::new("btrfs")
            .args(["subvolume", "delete", path])
            .output()
            .await?;

        if !output.status.success() {
            return Err(Error::Internal(format!(
                "btrfs delete {}: {}",
                path,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }
}

fn parse_subvolume_line(line: &str) -> Option<Subvolume> {
    // Formato: ID <id> gen <gen> cgen <cgen> parent <parent> top level <top> parent_uuid <puuid> uuid <uuid> path <path>
    let mut id = None;
    let mut generation = None;
    let mut parent_uuid = None;
    let mut uuid = None;
    let mut path = None;

    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "ID" if i + 1 < tokens.len() => {
                id = tokens[i + 1].parse().ok();
                i += 2;
            }
            "gen" if i + 1 < tokens.len() => {
                generation = tokens[i + 1].parse().ok();
                i += 2;
            }
            "parent_uuid" if i + 1 < tokens.len() => {
                let v = tokens[i + 1];
                parent_uuid = if v == "-" { None } else { Some(v.to_string()) };
                i += 2;
            }
            "uuid" if i + 1 < tokens.len() => {
                let v = tokens[i + 1];
                uuid = if v == "-" { None } else { Some(v.to_string()) };
                i += 2;
            }
            "path" if i + 1 < tokens.len() => {
                path = Some(tokens[i + 1..].join(" "));
                break;
            }
            _ => i += 1,
        }
    }

    Some(Subvolume {
        id: id?,
        generation: generation?,
        path: path?,
        parent_uuid,
        uuid,
    })
}
