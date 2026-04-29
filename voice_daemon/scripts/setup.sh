#!/usr/bin/env bash
# voice_daemon/scripts/setup.sh — bootstrap idempotente del voice daemon.
#
# Ejecutar UNA VEZ tras clonar el repo (o tras añadir nuevas deps al
# pyproject.toml). Es seguro re-ejecutar — uv detecta lo ya instalado.
#
# Qué hace:
#   1. Verifica deps de sistema (portaudio + uv).
#   2. Instala Python 3.11 vía uv (sandbox, no toca el sistema).
#   3. uv sync (crea .venv + descarga torch + faster-whisper + onnx).
#   4. Pre-descarga modelos de openWakeWord (~50 MB) y faster-whisper base
#      (~140 MB) para que el primer arranque del daemon no se trabe.
#
# Uso desde la raíz del repo:
#   ./voice_daemon/scripts/setup.sh

set -euo pipefail

VOICE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

log()  { printf '\033[36m[voice-setup]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[voice-setup] WARN:\033[0m %s\n' "$*"; }
fail() { printf '\033[31m[voice-setup] FAIL:\033[0m %s\n' "$*" >&2; exit 1; }

# ─── Step 1: deps de sistema ───
log "Verificando deps de sistema (portaudio + uv)..."
if ! pkg-config --exists portaudio-2.0 2>/dev/null \
   && ! ldconfig -p | grep -q libportaudio; then
    warn "portaudio no detectado. Instálalo con:"
    warn "  sudo pacman -S --needed portaudio"
    fail "Aborta tras instalar portaudio."
fi
command -v uv >/dev/null 2>&1 || {
    warn "uv no está en PATH. Instálalo con:"
    warn "  sudo pacman -S --needed uv"
    fail "Aborta tras instalar uv."
}

# ─── Step 2: Python 3.11 vía uv ───
log "Asegurando Python 3.11 disponible para uv (idempotente)..."
uv python install 3.11

# ─── Step 3: uv sync ───
log "Sincronizando entorno (.venv + deps Python)..."
log "(primer run descarga ~2-3 GB: torch, faster-whisper, onnxruntime)"
cd "$VOICE_DIR"
uv sync

# ─── Step 4: Pre-descarga modelos ───
log "Pre-descargando modelos para evitar fricción en primer arranque..."

uv run python - <<'PY'
import structlog
log = structlog.get_logger("voice-setup")

# openWakeWord — bajará todos los modelos pre-trained, incluido hey_jarvis.
import openwakeword
log.info("downloading openWakeWord pre-trained models...")
openwakeword.utils.download_models()

# faster-whisper "base" — se baja solo al instanciar el modelo.
from faster_whisper import WhisperModel
log.info("loading faster-whisper base model (will download if missing)...")
WhisperModel("base", device="cpu", compute_type="int8")
log.info("done")
PY

log "═══════════════════════════════════════════════════"
log "Setup completo. Para arrancar el daemon:"
log "  cd $VOICE_DIR && uv run voice-daemon"
log "O usa el wrapper con logs filtrados:"
log "  $VOICE_DIR/scripts/run.sh"
log "═══════════════════════════════════════════════════"
