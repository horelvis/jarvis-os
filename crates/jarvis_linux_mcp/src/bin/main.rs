//! `jarvis-linux-mcp` — CLI demo del Linux MCP server.
//!
//! Modo de uso:
//!   jarvis-linux-mcp tools                                # lista herramientas
//!   jarvis-linux-mcp invoke systemd.unit_status \
//!       --args '{"unit": "bluetooth.service"}'            # invoca una tool
//!
//! Esto NO es el server MCP completo (eso es F1.2.b/F1.4). Es un atajo
//! CLI que registra el ToolRegistry y ejecuta una tool ad-hoc — útil para
//! validar la integración D-Bus desde shell, en CI, y para enseñar el
//! mapeo entre policy categories y operaciones reales del SO.

use std::sync::Arc;

use clap::{Parser, Subcommand};
use jarvis_linux_mcp::{
    ToolRegistry,
    adapter::{
        BtrfsAdapter, JournalAdapter, NetworkManagerAdapter, PolkitAdapter, ProcessAdapter,
        SystemdAdapter,
    },
    mcp_server,
    tools::{
        BtrfsSnapshotTool, FileReadSafeTool, JournalQueryTool, NetworkStatusTool,
        PolicyEvaluateTool, PolkitCheckTool, ProcessListTool, SystemdUnitStatusTool,
    },
};
use jarvis_policies::DefaultPolicy;

#[derive(Parser)]
#[command(name = "jarvis-linux-mcp", version, about = "Linux MCP server demo CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lista todas las herramientas registradas con su categoría y descripción.
    Tools,

    /// Invoca una herramienta con args JSON.
    Invoke {
        /// Nombre de la herramienta (p.ej. `systemd.unit_status`).
        tool: String,

        /// JSON con los args (p.ej. `{"unit":"bluetooth.service"}`).
        /// Si no se pasa, se asume `null`.
        #[arg(long)]
        args: Option<String>,
    },

    /// Arranca como MCP server stdio (JSON-RPC 2.0 newline-delimited).
    /// Usado por IronClaw vía `mcp add jarvis-linux --command jarvis-linux-mcp
    /// --args mcp-server`.
    McpServer,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Wire-up del registry. En F1.4 esto se desplaza al server real
    // (que inscribe todas las tools + escucha protocolo MCP).
    let systemd = Arc::new(SystemdAdapter::connect_system().await?);
    let process_adapter = ProcessAdapter::new();
    let journal_adapter = JournalAdapter::new();
    let nm_adapter = Arc::new(NetworkManagerAdapter::connect_system().await?);
    let btrfs_adapter = BtrfsAdapter::new();
    // PolkitAdapter solo se inicializa si tools/call lo necesita; conexión D-Bus
    // a polkit puede fallar en entornos donde polkit no corre (sandboxes de test).
    let polkit_adapter = match PolkitAdapter::connect_system().await {
        Ok(a) => Some(Arc::new(a)),
        Err(e) => {
            eprintln!("[jarvis-linux-mcp] polkit no disponible: {e}; polkit.check no se registrará");
            None
        }
    };
    let policy = DefaultPolicy;
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(SystemdUnitStatusTool::new(systemd.clone())));
    registry.register(Box::new(ProcessListTool::new(process_adapter)));
    registry.register(Box::new(JournalQueryTool::new(journal_adapter)));
    registry.register(Box::new(NetworkStatusTool::new(nm_adapter.clone())));
    registry.register(Box::new(PolicyEvaluateTool::new(policy)));
    registry.register(Box::new(BtrfsSnapshotTool::new(btrfs_adapter)));
    registry.register(Box::new(FileReadSafeTool::new()));
    if let Some(polkit) = polkit_adapter {
        registry.register(Box::new(PolkitCheckTool::new(polkit)));
    }

    match cli.command {
        Command::Tools => {
            println!("# Registered tools ({})", registry.len());
            for meta in registry.list() {
                println!(
                    "- {}  [{:?}]\n    {}",
                    meta.name, meta.category, meta.description
                );
            }
        }

        Command::Invoke { tool, args } => {
            let args_value: serde_json::Value = match args {
                Some(s) => serde_json::from_str(&s)?,
                None => serde_json::Value::Null,
            };

            let tool_ref = registry
                .get(&tool)
                .ok_or_else(|| format!("tool not found: {tool}"))?;

            let output = tool_ref.invoke(&args_value).await?;
            println!("{}", serde_json::to_string_pretty(&output.data)?);
            if let Some(msg) = output.user_message {
                eprintln!("[user_message] {msg}");
            }
        }

        Command::McpServer => {
            // El logger por defecto de tracing/log podría escribir a stdout
            // y romper el protocolo. mcp_server::run usa stderr para todo
            // lo que no es protocolo.
            //
            // Guardian-on: el server consulta DefaultPolicy antes de cada
            // tool.invoke. Decision::Deny bloquea, Decision::Allow procede,
            // Decision::Confirm logea y procede (UI inline en F2).
            let state = mcp_server::ServerState::new(registry, Arc::new(policy));
            mcp_server::run(state).await?;
        }
    }

    Ok(())
}
