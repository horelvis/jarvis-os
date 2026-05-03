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
cargo build --release --bin ironclaw

##############################################
# Step 2: Reinstala binarios                 #
##############################################

# Compara hash por binario; reinstala solo si difiere para evitar restarts innecesarios.
for bin in ironclaw; do
    NEW_BIN="$JARVIS_OS_DIR/target/release/$bin"
    DST_BIN="/usr/local/bin/$bin"

    if [ ! -f "$NEW_BIN" ]; then
        fail "Compilación no produjo $NEW_BIN."
    fi

    NEW_HASH=$(sha256sum "$NEW_BIN" | awk '{print $1}')
    DST_HASH=$(sudo sha256sum "$DST_BIN" 2>/dev/null | awk '{print $1}' || echo "absent")

    if [ "$NEW_HASH" != "$DST_HASH" ]; then
        log "$bin cambió ($NEW_HASH != $DST_HASH); reinstalando..."
        sudo install -Dm755 "$NEW_BIN" "$DST_BIN"
    else
        log "$bin sin cambios (sha256 idéntico); skip."
    fi
done

# jarvis-chat wrapper.
sudo install -Dm755 "$JARVIS_OS_DIR/arch/scripts/jarvis-chat" /usr/local/bin/jarvis-chat

##############################################
# Step 3: Sync configs versionados           #
##############################################

# arch/configs/ contiene templates que se copian a destinos del usuario.
# Estructura espejo: arch/configs/hyprland/custom.conf → ~/.config/hypr/custom/jarvis-os.conf, etc.
# v20.0 deja la estructura preparada pero VACÍA — se irá llenando cuando
# necesitemos overrides puntuales (por ejemplo el patch del finger-count).

# PipeWire echo-cancel (sólo reinicia PipeWire si la config cambió).
if [ -f "$JARVIS_OS_DIR/arch/configs/pipewire/echo-cancel.conf" ]; then
    PW_DST="$HOME/.config/pipewire/pipewire.conf.d/jarvis-echo-cancel.conf"
    PW_SRC="$JARVIS_OS_DIR/arch/configs/pipewire/echo-cancel.conf"
    mkdir -p "$(dirname "$PW_DST")"
    if [ ! -f "$PW_DST" ] || ! cmp -s "$PW_SRC" "$PW_DST"; then
        cp "$PW_SRC" "$PW_DST"
        log "PipeWire echo-cancel actualizado; reiniciando PipeWire..."
        systemctl --user restart pipewire pipewire-pulse wireplumber 2>/dev/null || \
            warn "Reinicio PipeWire falló (¿no en sesión user?)"
    fi
fi

if [ -d "$JARVIS_OS_DIR/arch/configs/hyprland" ]; then
    log "Sincronizando overrides de Hyprland (append idempotente)..."
    mkdir -p "$HOME/.config/hypr/custom"
    # end-4/dots-hyprland NO carga custom/*.conf (glob); solo carga
    # nombres específicos: env, variables, execs, general, rules,
    # keybinds. Por eso appendamos al archivo existente con markers
    # delimitando nuestro bloque, y reemplazamos en cada run.
    for src in "$JARVIS_OS_DIR/arch/configs/hyprland/"*.conf; do
        [ -f "$src" ] || continue
        base=$(basename "$src")
        dst="$HOME/.config/hypr/custom/$base"
        [ -f "$dst" ] || touch "$dst"

        # Limpia bloques previos (markers actuales y los heredados de
        # iteraciones tempranas con otros nombres de marker).
        sed -i \
            -e '/# >>> jarvis-os: managed BEGIN/,/# <<< jarvis-os: managed END/d' \
            -e '/# >>> jarvis-os overrides <<</,/# <<< jarvis-os overrides >>>/d' \
            -e '/# >>> jarvis-os F1.5 inline confirm <<</,/# <<< jarvis-os F1.5 inline confirm >>>/d' \
            "$dst"

        # Append nuevo bloque (que ya viene marcado en el src).
        cat "$src" >> "$dst"
        log "  $base actualizado."
    done
fi

# Wallpaper jarvis-os: solo refresca si cambió en el repo.
mkdir -p "$HOME/Pictures/jarvis-os"
WALLPAPER_PATH="$HOME/Pictures/jarvis-os/wallpaper.jpg"
WALLPAPER_CHANGED=false
if [ "$JARVIS_OS_DIR/assets/wallpaper.jpg" -nt "$WALLPAPER_PATH" ] \
   || [ ! -f "$WALLPAPER_PATH" ]; then
    cp "$JARVIS_OS_DIR/assets/wallpaper.jpg" "$WALLPAPER_PATH"
    log "Wallpaper actualizado."
    WALLPAPER_CHANGED=true
fi

# Asegura que Quickshell apunta al wallpaper de jarvis-os (idempotente).
QS_WALL_DIR="$HOME/.local/state/quickshell/user/generated/wallpaper"
mkdir -p "$QS_WALL_DIR"
QS_WALL_FILE="$QS_WALL_DIR/path.txt"
if [ "$(cat "$QS_WALL_FILE" 2>/dev/null)" != "$WALLPAPER_PATH" ]; then
    echo "$WALLPAPER_PATH" > "$QS_WALL_FILE"
    log "Quickshell wallpaper state apuntando a $WALLPAPER_PATH."
    WALLPAPER_CHANGED=true
fi

if [ "$WALLPAPER_CHANGED" = "true" ] && command -v matugen >/dev/null 2>&1; then
    matugen image "$WALLPAPER_PATH" >/dev/null 2>&1 || \
        warn "matugen image falló (paleta Material You no regenerada)"
    log "matugen regeneró la paleta Material You."
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

# Si quedan instalaciones antiguas con la unit del daemon legacy, la
# desactivamos — el voice engine ahora vive in-process dentro de ironclaw.
if systemctl --user is-active jarvis-voice-daemon.service >/dev/null 2>&1; then
    log "Detectado jarvis-voice-daemon.service legacy — desactivando..."
    systemctl --user disable --now jarvis-voice-daemon.service 2>/dev/null || true
fi

##############################################
# Step 6: Verificación de integridad         #
##############################################

log "Verificando integración con IronClaw..."
if command -v ironclaw >/dev/null 2>&1; then
    log "✓ ironclaw disponible. Las jarvis-os system tools (process_list, journal_query,"
    log "  systemd_unit_status, network_status, btrfs_snapshot, polkit_check, policy_evaluate)"
    log "  se registran in-process al arranque del agente."
else
    warn "ironclaw no disponible en PATH. ¿Está instalado?"
fi

log "═══════════════════════════════════════════════════"
log "Update completo. Log: $LOG_FILE"
log "═══════════════════════════════════════════════════"
