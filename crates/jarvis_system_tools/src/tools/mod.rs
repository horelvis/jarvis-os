//! Tool implementations. Each module exposes one `impl Tool` struct
//! that gates `execute()` through `jarvis_policies::DefaultPolicy`.

pub mod btrfs_snapshot;
pub mod journal_query;
pub mod network_status;
pub mod policy_evaluate;
pub mod polkit_check;
pub mod process_list;
pub mod systemd_unit_status;
