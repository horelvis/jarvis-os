#!/usr/bin/env bash
# arch/update.sh — actualizar una instalación jarvis-os existente.
#
# Equivalente conceptual a `nixos-rebuild switch` para nuestro setup Arch.
# Idempotente. Aplica cambios sin reinstalar todo.
#
# USO:
#   cd /opt/jarvis-os
#   git pull                       # opcional: bajar última versión del repo
#   ./arch/update.sh               # aplica cambios al sistema instalado
#
# QUÉ HACE (todo idempotente):
#   1. Verifica precondiciones (Arch, no-root, repo válido).
#   2. Recompila crates Rust modificados (cargo decide qué).
#   3. Reinstala binarios cambiados a /usr/local/bin/.
#   4. Sincroniza configs versionados de arch/configs/ → destinos.
#   5. Sincroniza systemd-user units; reload del daemon si cambian.
#   6. Reinicia servicios afectados.
#   7. Verifica integridad: ironclaw mcp test jarvis-linux.
#
# QUÉ NO HACE:
#   - NO actualiza paquetes pacman (`sudo pacman -Syu` es independiente).
#   - NO reinstala dots-hyprland (vive en ~/dots-hyprland, refresca con
#     `cd ~/dots-hyprland && git pull && ./setup install`).
#   - NO toca ~/.ironclaw/.env del usuario (sus API keys son sagradas).

set -euo pipefail

JARVIS_OS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_FILE="/tmp/jarvis-os-update-$(date +%Y%m%d-%H%M%S).log"

log()   { printf '\033[36m[update]\033[0m %s\n' "$*" | tee -a "$LOG_FILE"; }
warn()  { printf '\033[33m[update] WARN:\033[0m %s\n' "$*" | tee -a "$LOG_FILE"; }
fail()  { printf '\033[31m[update] FAIL:\033[0m %s\n' "$*" | tee -a "$LOG_FILE" >&2; exit 1; }

# ─── Precondiciones ───
[ -f /etc/arch-release ] || fail "No estás en Arch Linux."
[ "$EUID" -ne 0 ] || fail "No correr como root. sudo se invoca puntualmente."
[ -f "$JARVIS_OS_DIR/Cargo.toml" ] || fail "No parece directorio jarvis-os: falta Cargo.toml."

log "jarvis-os update iniciado en $(date -Iseconds)"
log "Repo: $JARVIS_OS_DIR"
log "Branch: $(cd "$JARVIS_OS_DIR" && git branch --show-current 2>/dev/null || echo 'desconocida')"

##############################################
# Step 1: Recompila crates Rust              #
##############################################

log "Compilando crates Rust (cargo decide qué rebuild)..."
cd "$JARVIS_OS_DIR"
cargo build --release -p jarvis_linux_mcp --bin jarvis-linux-mcp

##############################################
# Step 2: Reinstala binarios                 #
##############################################

# Compara hash; reinstala solo si difiere para evitar restarts innecesarios.
NEW_BIN="$JARVIS_OS_DIR/target/release/jarvis-linux-mcp"
DST_BIN="/usr/local/bin/jarvis-linux-mcp"

if [ ! -f "$NEW_BIN" ]; then
    fail "Compilación no produjo $NEW_BIN."
fi

NEW_HASH=$(sha256sum "$NEW_BIN" | awk '{print $1}')
DST_HASH=$(sudo sha256sum "$DST_BIN" 2>/dev/null | awk '{print $1}' || echo "absent")

if [ "$NEW_HASH" != "$DST_HASH" ]; then
    log "jarvis-linux-mcp cambió ($NEW_HASH != $DST_HASH); reinstalando..."
    sudo install -Dm755 "$NEW_BIN" "$DST_BIN"
else
    log "jarvis-linux-mcp sin cambios (sha256 idéntico); skip."
fi

# jarvis-chat wrapper.
sudo install -Dm755 "$JARVIS_OS_DIR/arch/scripts/jarvis-chat" /usr/local/bin/jarvis-chat

##############################################
# Step 3: Sync configs versionados           #
##############################################

# arch/configs/ contiene templates que se copian a destinos del usuario.
# Estructura espejo: arch/configs/hyprland/custom.conf → ~/.config/hypr/custom/jarvis-os.conf, etc.
# v20.0 deja la estructura preparada pero VACÍA — se irá llenando cuando
# necesitemos overrides puntuales (por ejemplo el patch del finger-count).

if [ -d "$JARVIS_OS_DIR/arch/configs/hyprland" ]; then
    log "Sincronizando overrides de Hyprland..."
    mkdir -p "$HOME/.config/hypr/custom"
    cp -u "$JARVIS_OS_DIR/arch/configs/hyprland/"*.conf \
        "$HOME/.config/hypr/custom/" 2>/dev/null || true
fi

# Wallpaper jarvis-os: solo refresca si cambió en el repo.
mkdir -p "$HOME/Pictures/jarvis-os"
if [ "$JARVIS_OS_DIR/assets/wallpaper.jpg" -nt "$HOME/Pictures/jarvis-os/wallpaper.jpg" ] \
   || [ ! -f "$HOME/Pictures/jarvis-os/wallpaper.jpg" ]; then
    cp "$JARVIS_OS_DIR/assets/wallpaper.jpg" "$HOME/Pictures/jarvis-os/wallpaper.jpg"
    log "Wallpaper actualizado."
fi

##############################################
# Step 4: SystemD user units                 #
##############################################

UNIT_CHANGED=false
mkdir -p "$HOME/.config/systemd/user"
for unit in "$JARVIS_OS_DIR/arch/systemd-user/"*.service; do
    [ -f "$unit" ] || continue
    DST="$HOME/.config/systemd/user/$(basename "$unit")"
    if [ ! -f "$DST" ] || ! cmp -s "$unit" "$DST"; then
        cp "$unit" "$DST"
        log "Unit actualizada: $(basename "$unit")"
        UNIT_CHANGED=true
    fi
done

if [ "$UNIT_CHANGED" = "true" ]; then
    systemctl --user daemon-reload
    log "systemctl daemon-reload ejecutado."
fi

##############################################
# Step 5: Restart servicios afectados        #
##############################################

if systemctl --user is-active jarvis-mcp-register.service >/dev/null 2>&1; then
    log "Restart jarvis-mcp-register para que recoja binary actualizado..."
    systemctl --user restart jarvis-mcp-register.service || \
        warn "Restart falló; revisar logs con journalctl --user -u jarvis-mcp-register"
fi

##############################################
# Step 6: Verificación de integridad         #
##############################################

log "Verificando integración con IronClaw..."
if command -v ironclaw >/dev/null 2>&1; then
    if ironclaw mcp list 2>&1 | grep -q "jarvis-linux"; then
        log "✓ jarvis-linux registrado en IronClaw."
        if ironclaw mcp test jarvis-linux 2>&1 | tail -10 | grep -q "Available tools"; then
            log "✓ jarvis-linux responde a tools/list."
        else
            warn "jarvis-linux registrado pero `mcp test` no devolvió tools."
        fi
    else
        warn "jarvis-linux NO registrado en IronClaw. Ejecuta:"
        warn "  ironclaw mcp add jarvis-linux --transport stdio --command jarvis-linux-mcp --arg mcp-server"
    fi
else
    warn "ironclaw no disponible en PATH. ¿Está instalado?"
fi

log "═══════════════════════════════════════════════════"
log "Update completo. Log: $LOG_FILE"
log "═══════════════════════════════════════════════════"
