//! Adapter a systemd vía D-Bus (interfaz `org.freedesktop.systemd1`).
//!
//! Spec:
//!   - System bus, destination `org.freedesktop.systemd1`
//!   - Path raíz `/org/freedesktop/systemd1`, interfaz `Manager`
//!   - Cada unit es un objeto en `/org/freedesktop/systemd1/unit/<escaped_name>`
//!     con interfaz `Unit`.
//!
//! Esta crate F1.2 contiene solo la conexión + types. Las llamadas concretas
//! (GetUnit, StartUnit, etc.) las cablearemos en F1.2.b — placeholder con
//! `Error::NotImplemented` para compilar el grafo y que las tools puedan
//! escribirse contra esta API estable.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use zbus::{Connection, Proxy, zvariant::OwnedObjectPath};

/// Estado de una unit systemd, simplificado para el output al agente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitStatus {
    /// Nombre completo (`nginx.service`, `bluetooth.service`).
    pub name: String,
    /// Estado de carga (`loaded`, `not-found`, `error`).
    pub load_state: String,
    /// Estado activo (`active`, `inactive`, `failed`, `activating`, `deactivating`).
    pub active_state: String,
    /// Subestado más fino (`running`, `dead`, `exited`).
    pub sub_state: String,
    /// Descripción humana de la unit (de `Description=` en el unit file).
    pub description: String,
}

/// Wrapper sobre la conexión D-Bus al system bus para hablar con systemd.
///
/// Una sola instancia por proceso es suficiente; zbus mantiene la conexión
/// mux-eada internamente.
pub struct SystemdAdapter {
    connection: Connection,
}

impl SystemdAdapter {
    /// Conecta al D-Bus system bus (donde vive systemd, polkit, NetworkManager).
    pub async fn connect_system() -> Result<Self> {
        let connection = Connection::system().await?;
        Ok(Self { connection })
    }

    /// Acceso de bajo nivel para tests o tools que necesiten hablar D-Bus
    /// directamente con systemd1.Manager u otros.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Construye un proxy a `org.freedesktop.systemd1.Manager`.
    /// Helper interno reusado por los métodos que llaman al manager.
    async fn manager_proxy(&self) -> Result<Proxy<'_>> {
        let proxy = Proxy::new(
            &self.connection,
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
        )
        .await?;
        Ok(proxy)
    }

    /// Obtiene el estado de una unit.
    ///
    /// Bajo el capó:
    ///   1. `Manager.GetUnit(name) -> ObjectPath` — si la unit no está
    ///      cargada, zbus devuelve `org.freedesktop.systemd1.NoSuchUnit`
    ///      como error D-Bus, que mapeamos transparente a `Error::Dbus`.
    ///   2. Properties reads sobre `org.freedesktop.systemd1.Unit` en el
    ///      path devuelto.
    pub async fn unit_status(&self, unit: &str) -> Result<UnitStatus> {
        let manager = self.manager_proxy().await?;
        let unit_path: OwnedObjectPath = manager.call("GetUnit", &(unit,)).await?;

        let unit_proxy = Proxy::new(
            &self.connection,
            "org.freedesktop.systemd1",
            unit_path,
            "org.freedesktop.systemd1.Unit",
        )
        .await?;

        let load_state: String = unit_proxy.get_property("LoadState").await?;
        let active_state: String = unit_proxy.get_property("ActiveState").await?;
        let sub_state: String = unit_proxy.get_property("SubState").await?;
        let description: String = unit_proxy.get_property("Description").await?;

        Ok(UnitStatus {
            name: unit.to_string(),
            load_state,
            active_state,
            sub_state,
            description,
        })
    }

    /// Arranca una unit. `mode` típico = "replace" (cancela jobs en cola
    /// para esa unit y mete el nuevo Start). Otros: "fail", "isolate",
    /// "ignore-dependencies", "ignore-requirements".
    ///
    /// Devuelve el `ObjectPath` del job creado (ignored aquí; podría
    /// trackearse para esperar a completion vía signal `JobRemoved`).
    pub async fn start_unit(&self, unit: &str) -> Result<()> {
        let manager = self.manager_proxy().await?;
        let _job: OwnedObjectPath = manager.call("StartUnit", &(unit, "replace")).await?;
        Ok(())
    }

    /// Para una unit. Mismo `mode` semantics que `start_unit`.
    pub async fn stop_unit(&self, unit: &str) -> Result<()> {
        let manager = self.manager_proxy().await?;
        let _job: OwnedObjectPath = manager.call("StopUnit", &(unit, "replace")).await?;
        Ok(())
    }
}
