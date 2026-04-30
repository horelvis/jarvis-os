//! jarvis-os system tools (process, journald, systemd, networkd, polkit,
//! btrfs, policy_evaluate). Implement IronClaw's `Tool` trait against the
//! adapters from the `jarvis_system_tools` crate.
//!
//! Registration entry point: [`register`].

pub mod btrfs_snapshot;
pub mod journal_query;
pub mod network_status;
pub mod policy_evaluate;
pub mod polkit_check;
pub mod process_list;
pub mod systemd_unit_status;

use std::sync::Arc;

use jarvis_system_tools::adapter;
use tracing::{info, warn};

use crate::tools::registry::ToolRegistry;

/// Register all 7 jarvis-os system tools in `registry`. Some adapters
/// require D-Bus connections that are initialized here. If required
/// D-Bus adapters (systemd, NetworkManager) fail to connect, no jarvis
/// tools are registered — IronClaw continues without them so the agent
/// stays usable on environments where D-Bus is unavailable.
pub async fn register(registry: &Arc<ToolRegistry>) {
    let process_adapter = adapter::process::ProcessAdapter::new();
    let journal_adapter = adapter::journal::JournalAdapter::new();
    let btrfs_adapter = adapter::btrfs::BtrfsAdapter::new();

    let systemd_adapter = match adapter::systemd::SystemdAdapter::connect_system().await {
        Ok(a) => Arc::new(a),
        Err(e) => {
            warn!(
                "systemd D-Bus unavailable: {e}; jarvis-os system tools will not be registered"
            );
            return;
        }
    };
    let nm_adapter = match adapter::network::NetworkManagerAdapter::connect_system().await {
        Ok(a) => Arc::new(a),
        Err(e) => {
            warn!(
                "NetworkManager D-Bus unavailable: {e}; jarvis-os system tools will not be registered"
            );
            return;
        }
    };

    let polkit_adapter = match adapter::polkit::PolkitAdapter::connect_system().await {
        Ok(a) => Some(Arc::new(a)),
        Err(e) => {
            warn!("polkit unavailable: {e}; polkit_check tool will not be registered");
            None
        }
    };

    registry
        .register(Arc::new(process_list::ProcessListTool::new(
            process_adapter,
        )))
        .await;
    registry
        .register(Arc::new(journal_query::JournalQueryTool::new(
            journal_adapter,
        )))
        .await;
    registry
        .register(Arc::new(systemd_unit_status::SystemdUnitStatusTool::new(
            systemd_adapter,
        )))
        .await;
    registry
        .register(Arc::new(network_status::NetworkStatusTool::new(nm_adapter)))
        .await;
    registry
        .register(Arc::new(btrfs_snapshot::BtrfsSnapshotTool::new(
            btrfs_adapter,
        )))
        .await;
    registry
        .register(Arc::new(policy_evaluate::PolicyEvaluateTool::new()))
        .await;

    if let Some(polkit) = polkit_adapter {
        registry
            .register(Arc::new(polkit_check::PolkitCheckTool::new(polkit)))
            .await;
    }

    info!("jarvis-os system tools registered");
}
