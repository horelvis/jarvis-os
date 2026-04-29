//! Tool: `btrfs.snapshot` — gestiona snapshots btrfs (list/create/delete).
//!
//! Categorías por operación:
//!   - `list` → ReadSystem (consulta).
//!   - `create` / `delete` → MutateSystem (modifica filesystem).
//!
//! En v11 declaramos la categoría de la tool como `MutateSystem` para que
//! el guardian fuerce CONFIRM en list TAMBIÉN — más conservador, evita
//! sorpresas al diferenciar action types. v12 podrá refinar splitting en
//! tools separadas (`btrfs.list_snapshots` ReadSystem, `btrfs.create_snapshot`
//! MutateSystem) cuando el flujo del HUD lo justifique.

use crate::{
    adapter::btrfs::BtrfsAdapter,
    error::{Error, Result},
    tool::{Tool, ToolMetadata, ToolOutput},
};
use jarvis_policies::ActionCategory;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Args {
    /// Lista subvolúmenes en `mount_path` (default `/`).
    List {
        #[serde(default = "default_mount")]
        mount_path: String,
    },
    /// Crea un snapshot read-only de `source` en `dest`.
    Create { source: String, dest: String },
    /// Elimina un subvolume/snapshot.
    Delete { path: String },
}

fn default_mount() -> String {
    "/".to_string()
}

pub struct BtrfsSnapshotTool {
    adapter: BtrfsAdapter,
    metadata: ToolMetadata,
}

impl BtrfsSnapshotTool {
    pub fn new(adapter: BtrfsAdapter) -> Self {
        let metadata = ToolMetadata {
            name: "btrfs.snapshot".to_string(),
            description: "Manage btrfs subvolumes and snapshots: list existing, \
                          create read-only snapshots, or delete. Operations on the \
                          filesystem require CONFIRM/ALLOW per jarvis_policies. \
                          Returns empty list if mount_path is not on btrfs."
                .to_string(),
            // MutateSystem porque create/delete sí mutan. List per se sería
            // ReadSystem; v12 dividirá en dos tools distintas si conviene.
            category: ActionCategory::MutateSystem,
            args_schema: serde_json::json!({
                "type": "object",
                "required": ["op"],
                "oneOf": [
                    {
                        "properties": {
                            "op": { "const": "list" },
                            "mount_path": {
                                "type": "string",
                                "default": "/",
                                "description": "Path to a btrfs mount point"
                            }
                        }
                    },
                    {
                        "properties": {
                            "op": { "const": "create" },
                            "source": {
                                "type": "string",
                                "description": "Path to source subvolume"
                            },
                            "dest": {
                                "type": "string",
                                "description": "Path where snapshot will be created"
                            }
                        },
                        "required": ["source", "dest"]
                    },
                    {
                        "properties": {
                            "op": { "const": "delete" },
                            "path": {
                                "type": "string",
                                "description": "Path of subvolume/snapshot to delete"
                            }
                        },
                        "required": ["path"]
                    }
                ]
            }),
        };
        Self { adapter, metadata }
    }
}

#[async_trait::async_trait]
impl Tool for BtrfsSnapshotTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    async fn invoke(&self, args: &serde_json::Value) -> Result<ToolOutput> {
        let parsed: Args = serde_json::from_value(args.clone())
            .map_err(|e| Error::InvalidArguments(format!("btrfs.snapshot args: {e}")))?;

        match parsed {
            Args::List { mount_path } => {
                let subvols = self.adapter.list(&mount_path).await?;
                let count = subvols.len();
                let data = serde_json::to_value(&subvols)?;
                Ok(ToolOutput::new(data)
                    .with_user_message(format!("{count} subvolume(s) at {mount_path}")))
            }
            Args::Create { source, dest } => {
                self.adapter.snapshot_readonly(&source, &dest).await?;
                Ok(ToolOutput::new(serde_json::json!({
                    "created": dest.clone(),
                    "source": source.clone(),
                    "readonly": true,
                }))
                .with_user_message(format!("snapshot created: {dest}")))
            }
            Args::Delete { path } => {
                self.adapter.delete(&path).await?;
                Ok(ToolOutput::new(serde_json::json!({ "deleted": path.clone() }))
                    .with_user_message(format!("deleted: {path}")))
            }
        }
    }
}
