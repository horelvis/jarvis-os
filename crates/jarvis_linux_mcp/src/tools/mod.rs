//! Tools concretas del Linux MCP server.
//!
//! Cada submódulo expone un `impl Tool` listo para registrar en un
//! `ToolRegistry`. La organización es por dominio del sistema operativo:
//! systemd, polkit, btrfs, dbus generic, etc.

pub mod btrfs_snapshot;
pub mod file_read_safe;
pub mod journal_query;
pub mod network_status;
pub mod policy_evaluate;
pub mod polkit_check;
pub mod process_list;
pub mod systemd_status;

pub use btrfs_snapshot::BtrfsSnapshotTool;
pub use file_read_safe::FileReadSafeTool;
pub use journal_query::JournalQueryTool;
pub use network_status::NetworkStatusTool;
pub use policy_evaluate::PolicyEvaluateTool;
pub use polkit_check::PolkitCheckTool;
pub use process_list::ProcessListTool;
pub use systemd_status::SystemdUnitStatusTool;
