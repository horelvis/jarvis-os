//! Adapter para consultar estado de NetworkManager vía D-Bus.
//!
//! NetworkManager expone una API D-Bus rica en `org.freedesktop.NetworkManager`.
//! Empezamos con propiedades del manager y enumeración de devices activos —
//! suficiente para responder "estoy conectado, en qué SSID, qué IP".
//!
//! API D-Bus relevante:
//!   service: org.freedesktop.NetworkManager
//!   path: /org/freedesktop/NetworkManager
//!   iface: org.freedesktop.NetworkManager
//!     property State (uint32: 70=connected_global, 60=connected_site, ...)
//!     property ActiveConnections (array of object paths)
//!   path: /org/freedesktop/NetworkManager/ActiveConnection/<n>
//!   iface: org.freedesktop.NetworkManager.Connection.Active
//!     property Type (string: "802-11-wireless", "ethernet")
//!     property Devices (array of object paths)
//!     property Id (string: connection name)
//!   path: /org/freedesktop/NetworkManager/Devices/<n>
//!   iface: org.freedesktop.NetworkManager.Device
//!     property Interface (string: "wlp3s0")
//!     property Ip4Config (object path)

use crate::error::Result;
use serde::{Deserialize, Serialize};
use zbus::{Connection, Proxy, zvariant::OwnedObjectPath};

/// Estado global agregado de NetworkManager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStatus {
    /// Estado del manager: "connected_global", "connected_site", "connected_local",
    /// "connecting", "disconnecting", "asleep", "unknown".
    pub state: String,
    /// Hostname del sistema según NM.
    pub hostname: String,
    /// Conexiones activas (cada una con su tipo + interfaz).
    pub active_connections: Vec<ActiveConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveConnection {
    pub id: String,
    /// `802-11-wireless`, `802-3-ethernet`, `vpn`, etc.
    pub connection_type: String,
    /// Interfaces de red asociadas (lista por si hay bonding/bridges).
    pub interfaces: Vec<String>,
}

/// Adapter al D-Bus system bus para hablar con NetworkManager.
pub struct NetworkManagerAdapter {
    connection: Connection,
}

impl NetworkManagerAdapter {
    pub async fn connect_system() -> Result<Self> {
        let connection = Connection::system().await?;
        Ok(Self { connection })
    }

    /// Construye el proxy al manager root.
    async fn manager_proxy(&self) -> Result<Proxy<'_>> {
        let proxy = Proxy::new(
            &self.connection,
            "org.freedesktop.NetworkManager",
            "/org/freedesktop/NetworkManager",
            "org.freedesktop.NetworkManager",
        )
        .await?;
        Ok(proxy)
    }

    /// Snapshot del estado de red completo.
    pub async fn status(&self) -> Result<NetworkStatus> {
        let manager = self.manager_proxy().await?;

        // State numérico → string legible. Códigos NM_STATE_*.
        let state_code: u32 = manager.get_property("State").await?;
        let state = match state_code {
            10 => "asleep",
            20 => "disconnected",
            30 => "disconnecting",
            40 => "connecting",
            50 => "connected_local",
            60 => "connected_site",
            70 => "connected_global",
            _ => "unknown",
        }
        .to_string();

        let hostname: String = manager.get_property("Hostname").await.unwrap_or_default();

        let active_paths: Vec<OwnedObjectPath> =
            manager.get_property("ActiveConnections").await?;

        let mut active_connections = Vec::new();
        for path in active_paths {
            if let Ok(conn) = self.read_active_connection(path).await {
                active_connections.push(conn);
            }
        }

        Ok(NetworkStatus {
            state,
            hostname,
            active_connections,
        })
    }

    async fn read_active_connection(&self, path: OwnedObjectPath) -> Result<ActiveConnection> {
        let proxy = Proxy::new(
            &self.connection,
            "org.freedesktop.NetworkManager",
            path,
            "org.freedesktop.NetworkManager.Connection.Active",
        )
        .await?;

        let id: String = proxy.get_property("Id").await.unwrap_or_default();
        let connection_type: String = proxy.get_property("Type").await.unwrap_or_default();
        let device_paths: Vec<OwnedObjectPath> =
            proxy.get_property("Devices").await.unwrap_or_default();

        let mut interfaces = Vec::new();
        for dpath in device_paths {
            if let Ok(name) = self.read_device_interface(dpath).await {
                interfaces.push(name);
            }
        }

        Ok(ActiveConnection {
            id,
            connection_type,
            interfaces,
        })
    }

    async fn read_device_interface(&self, path: OwnedObjectPath) -> Result<String> {
        let proxy = Proxy::new(
            &self.connection,
            "org.freedesktop.NetworkManager",
            path,
            "org.freedesktop.NetworkManager.Device",
        )
        .await?;
        let iface: String = proxy.get_property("Interface").await.unwrap_or_default();
        Ok(iface)
    }
}
