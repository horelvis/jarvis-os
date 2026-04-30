#!/usr/bin/env bash
# arch/install.sh — bootstrap jarvis-os sobre Arch Linux instalado.
#
# PRECONDICIÓN: estás en una instalación Arch base recién hecha
# (archinstall + reboot). Este script:
#   1. Verifica que estás en Arch.
#   2. Instala dependencias base + paru (AUR helper).
#   3. Instala end-4 dots-hyprland (illogical-impulse) via su instalador upstream.
#   4. Compila los crates Rust de jarvis-os (ironclaw + jarvis_voice_daemon).
#   5. Instala binarios + systemd-user services.
#   6. Configura wallpaper + secrets + ~/.ironclaw/.env.
#   7. Configura snapper para snapshots Btrfs (rollback).
#
# USO:
#   git clone <repo-jarvis-os> /opt/jarvis-os
#   cd /opt/jarvis-os
#   ./arch/install.sh
#
# El script es IDEMPOTENTE: re-ejecutarlo es seguro.

set -euo pipefail

JARVIS_OS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JARVIS_VERSION="0.20.0"
LOG_FILE="/tmp/jarvis-os-install.log"

# ─── Logging ───
log()  { printf '\033[36m[jarvis-os]\033[0m %s\n' "$*" | tee -a "$LOG_FILE"; }
warn() { printf '\033[33m[jarvis-os] WARN:\033[0m %s\n' "$*" | tee -a "$LOG_FILE"; }
fail() { printf '\033[31m[jarvis-os] FAIL:\033[0m %s\n' "$*" | tee -a "$LOG_FILE" >&2; exit 1; }

# ─── Verificación previa ───
[ -f /etc/arch-release ] || fail "No estás en Arch Linux. /etc/arch-release no existe."
[ "$EUID" -ne 0 ] || fail "Este script NO debe correrse como root. sudo se invoca puntualmente."
command -v sudo >/dev/null 2>&1 || fail "sudo no está disponible."

log "jarvis-os $JARVIS_VERSION install iniciado en $(date -Iseconds)"
log "JARVIS_OS_DIR = $JARVIS_OS_DIR"

# ─── Step 1: Dependencias base ───
log "Instalando dependencias pacman base..."
sudo pacman -Syu --noconfirm --needed \
  base-devel git curl wget \
  rustup \
  openssh networkmanager bluez bluez-utils \
  pipewire pipewire-pulse pipewire-alsa wireplumber \
  noto-fonts noto-fonts-cjk noto-fonts-emoji ttf-fira-code \
  hyprland hyprpaper hypridle hyprlock \
  foot wofi grim slurp wl-clipboard \
  jq ripgrep htop \
  btrfs-progs snapper \
  python python-pip uv \
  portaudio alsa-lib \
  base-devel cmake pkgconf

# ─── Step 2: Rust toolchain ───
# `command -v cargo` da true porque rustup crea un stub aunque no haya
# toolchain. Lo correcto es preguntarle a rustup si tiene un default.
if ! rustup show active-toolchain 2>/dev/null | grep -q '[0-9]'; then
    log "Configurando default toolchain Rust (stable)..."
    rustup default stable
fi

# ─── Step 3: paru (AUR helper) ───
if ! command -v paru >/dev/null 2>&1; then
    log "Instalando paru (AUR helper)..."
    # Limpia restos de runs anteriores fallidos (idempotencia).
    rm -rf /tmp/paru
    git clone https://aur.archlinux.org/paru.git /tmp/paru
    cd /tmp/paru && makepkg -si --noconfirm
    cd "$JARVIS_OS_DIR"
fi

# ─── Step 4: end-4 dots-hyprland (illogical-impulse) ───
# Lo instalamos solo si NO está ya presente (idempotencia).
if [ ! -d "$HOME/dots-hyprland" ]; then
    log "Instalando illogical-impulse desde end-4/dots-hyprland..."
    git clone --depth 1 https://github.com/end-4/dots-hyprland "$HOME/dots-hyprland"
    cd "$HOME/dots-hyprland"
    # Su instalador es interactivo — para automatización, lo correremos
    # manualmente con el primer boot. Aquí solo pre-clonamos.
    log "Repositorio dots-hyprland clonado en ~/dots-hyprland"
    log "Ejecuta '~/dots-hyprland/setup install' DESPUÉS de este script para completar."
    cd "$JARVIS_OS_DIR"
fi

# ─── Step 5: Compilar binarios Rust de jarvis-os ───
log "Compilando ironclaw + jarvis_voice_daemon (release)..."
log "(primer build tarda ~10-20min, hay caché incremental para próximas veces)"
cargo build --release --bin ironclaw
cargo build --release -p jarvis_voice_daemon --bin jarvis-voice-daemon

for bin in ironclaw jarvis-voice-daemon; do
    src="$JARVIS_OS_DIR/target/release/$bin"
    if [ -f "$src" ]; then
        sudo install -Dm755 "$src" "/usr/local/bin/$bin"
        log "$bin instalado en /usr/local/bin/"
    else
        fail "Compilación no produjo $src."
    fi
done

# ─── Step 6: Wrapper jarvis-chat ───
log "Instalando wrapper jarvis-chat..."
sudo install -Dm755 "$JARVIS_OS_DIR/arch/scripts/jarvis-chat" /usr/local/bin/jarvis-chat

# ─── Step 7: Wallpaper jarvis-os ───
log "Copiando wallpaper a ~/Pictures/jarvis-os/..."
mkdir -p "$HOME/Pictures/jarvis-os"
cp "$JARVIS_OS_DIR/assets/wallpaper.jpg" "$HOME/Pictures/jarvis-os/wallpaper.jpg"

# Setea wallpaper en el state de Quickshell (illogical-impulse lo lee
# de path.txt para renderizar). Y regenera la paleta Material You.
WALLPAPER_PATH="$HOME/Pictures/jarvis-os/wallpaper.jpg"
QS_WALL_DIR="$HOME/.local/state/quickshell/user/generated/wallpaper"
mkdir -p "$QS_WALL_DIR"
echo "$WALLPAPER_PATH" > "$QS_WALL_DIR/path.txt"
log "  Quickshell wallpaper state apuntando a $WALLPAPER_PATH"

if command -v matugen >/dev/null 2>&1; then
    matugen image "$WALLPAPER_PATH" >/dev/null 2>&1 || \
        warn "matugen image falló (paleta Material You no regenerada)"
    log "  matugen regeneró la paleta Material You"
fi

# ─── Step 7a: PipeWire echo-cancel para barge-in real en jarvis-voice-daemon ───
log "Instalando módulo PipeWire echo-cancel..."
mkdir -p "$HOME/.config/pipewire/pipewire.conf.d"
cp "$JARVIS_OS_DIR/arch/configs/pipewire/echo-cancel.conf" \
    "$HOME/.config/pipewire/pipewire.conf.d/jarvis-echo-cancel.conf"
systemctl --user restart pipewire pipewire-pulse wireplumber 2>/dev/null || \
    warn "No se pudo reiniciar PipeWire (¿no estás en sesión user?). Reinicia sesión y verifica con 'pactl list short sources | grep jarvis-mic-aec'."

# ─── Step 7b: Overrides Hyprland (append idempotente) ───
# end-4 NO carga custom/*.conf (glob); solo carga nombres específicos
# (env/variables/execs/general/rules/keybinds). Appendamos a cada uno
# con markers para que update.sh pueda regenerar el bloque.
if [ -d "$JARVIS_OS_DIR/arch/configs/hyprland" ]; then
    log "Aplicando overrides Hyprland..."
    mkdir -p "$HOME/.config/hypr/custom"
    for src in "$JARVIS_OS_DIR/arch/configs/hyprland/"*.conf; do
        [ -f "$src" ] || continue
        base=$(basename "$src")
        dst="$HOME/.config/hypr/custom/$base"
        [ -f "$dst" ] || touch "$dst"

        sed -i \
            -e '/# >>> jarvis-os: managed BEGIN/,/# <<< jarvis-os: managed END/d' \
            -e '/# >>> jarvis-os overrides <<</,/# <<< jarvis-os overrides >>>/d' \
            -e '/# >>> jarvis-os F1.5 inline confirm <<</,/# <<< jarvis-os F1.5 inline confirm >>>/d' \
            "$dst"
        cat "$src" >> "$dst"
        log "  $base"
    done
fi

# ─── Step 8: Snapper para rollback Btrfs ───
if findmnt -no FSTYPE / | grep -q btrfs; then
    if ! sudo snapper -c root list >/dev/null 2>&1; then
        log "Configurando snapper para snapshots root Btrfs..."
        sudo snapper -c root create-config /
        sudo systemctl enable --now snapper-timeline.timer snapper-cleanup.timer
        # grub-btrfs para entries de boot por snapshot
        paru -S --noconfirm --needed grub-btrfs
        sudo systemctl enable --now grub-btrfsd.service
    fi
fi

# ─── Step 9: SystemD user units ───
log "Instalando systemd-user units..."
mkdir -p "$HOME/.config/systemd/user"
for unit in "$JARVIS_OS_DIR/arch/systemd-user/"*.service; do
    if [ -f "$unit" ]; then
        cp "$unit" "$HOME/.config/systemd/user/"
        log "  $(basename "$unit")"
    fi
done
systemctl --user daemon-reload || true

# ─── Step 10: ~/.ironclaw/.env stateless defaults ───
log "Configurando ~/.ironclaw/ defaults..."
mkdir -p "$HOME/.ironclaw"
if [ ! -f "$HOME/.ironclaw/.env" ]; then
    cat > "$HOME/.ironclaw/.env" <<'EOF'
DATABASE_BACKEND=libsql
LIBSQL_PATH=/home/jarvis/.ironclaw/jarvis.db
DATABASE_URL=unused://libsql
AGENT_NAME=jarvis
HEARTBEAT_ENABLED=false
IRONCLAW_PROFILE=local
# API keys del operador deben ir aquí (Anthropic, ElevenLabs):
# ANTHROPIC_API_KEY=...
# ELEVENLABS_API_KEY=...
EOF
    log "Plantilla creada en ~/.ironclaw/.env. Edita las API keys antes de usar."
fi

log "═══════════════════════════════════════════════════"
log "jarvis-os $JARVIS_VERSION instalado correctamente."
log ""
log "PRÓXIMOS PASOS MANUALES:"
log "  1. cd ~/dots-hyprland && ./setup install"
log "     (instala el shell illogical-impulse interactivamente)"
log "  2. Edita ~/.ironclaw/.env con tus API keys reales."
log "  3. logout / login para entrar a Hyprland + jarvis-os"
log "  4. jarvis-chat run                  # primera conversación"
log "                                      # (jarvis-os system tools registradas in-process)"
log "═══════════════════════════════════════════════════"
