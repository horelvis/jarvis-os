//! Adaptadores a los subsistemas Linux que jarvis-os expone.
//!
//! Cada submódulo encapsula una API nativa (D-Bus, polkit, systemd, btrfs,
//! AT-SPI2) detrás de una struct con métodos async. Las tools (en
//! `crate::tools`) consumen estos adaptadores; nunca tocan zbus / ioctls
//! directamente.
//!
//! Este patrón aísla cambios de upstream (p.ej. systemd cambia el nombre
//! de un método D-Bus) en un solo lugar, en vez de propagarlos a N tools.

pub mod btrfs;
pub mod journal;
pub mod network;
pub mod polkit;
pub mod process;
pub mod systemd;

pub use btrfs::BtrfsAdapter;
pub use journal::JournalAdapter;
pub use network::NetworkManagerAdapter;
pub use polkit::PolkitAdapter;
pub use process::ProcessAdapter;
pub use systemd::SystemdAdapter;
