#!/usr/bin/env bash
# test-f4-asus.sh — smoke test de F4 (B1+B2+B4) en Asus.
#
# Valida que la consolidación in-process funciona: ironclaw arranca solo,
# el daemon legacy ya no existe, y la conversación end-to-end queda lista
# para test humano. NO valida B3 (el bug del feedback loop sigue presente
# hasta que B3 cierre con el AEC propio).
#
# Uso:
#   bash arch/scripts/test-f4-asus.sh             # check + build + restart + smoke
#   bash arch/scripts/test-f4-asus.sh --no-build  # sólo restart + smoke (iterar tunning)
#   bash arch/scripts/test-f4-asus.sh --b3        # post-B3: espera echo-cancel descargado
#
# Salida:
#   exit 0 → todo OK, listo para test humano (5 min de conversación).
#   exit ≠0 → algún check falló, ver mensaje.

set -euo pipefail

# ────────────────────────── helpers ───────────────────────────────────
RED=$'\033[31m'; GRN=$'\033[32m'; YLW=$'\033[33m'; CYA=$'\033[36m'; RST=$'\033[0m'
log()  { printf "%s[F4]%s %s\n" "$CYA" "$RST" "$*"; }
ok()   { printf "%s[OK]%s  %s\n"  "$GRN" "$RST" "$*"; }
warn() { printf "%s[!!]%s  %s\n"  "$YLW" "$RST" "$*"; }
fail() { printf "%s[FAIL]%s %s\n" "$RED" "$RST" "$*"; exit 1; }

DO_BUILD=1
B3_MODE=0
for arg in "$@"; do
    case "$arg" in
        --no-build) DO_BUILD=0 ;;
        --b3)       B3_MODE=1 ;;
        *) fail "unknown flag: $arg (usa --no-build o --b3)" ;;
    esac
done

# Resolver repo root sin asumir el path del usuario (regla:
# .claude/memory feedback_avoid_os_specific_prefixes).
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

log "repo: $REPO_ROOT"
log "modo: $([ "$DO_BUILD" -eq 1 ] && echo build+restart+smoke || echo restart+smoke) $([ "$B3_MODE" -eq 1 ] && echo '(post-B3: espera AEC propio)' || echo '(pre-B3: espera AEC PipeWire cargado)')"

# ─────────────────── 1) Pre-flight: rama y deps ───────────────────────
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
[ "$BRANCH" = "jarvis-arch-os" ] || warn "rama actual = $BRANCH (esperaba jarvis-arch-os)"

if [ -n "$(git status --porcelain)" ]; then
    warn "working tree con cambios sin commitear (continúo de todas formas)"
fi

command -v cargo >/dev/null    || fail "cargo no está en PATH"
command -v systemctl >/dev/null || fail "systemctl no está en PATH"
command -v pactl >/dev/null    || fail "pactl no está en PATH (¿pipewire-pulse instalado?)"

# B3 mode también requiere cmake+clang para que el build de
# webrtc-audio-processing pase.
if [ "$B3_MODE" -eq 1 ] && [ "$DO_BUILD" -eq 1 ]; then
    command -v cmake >/dev/null || fail "cmake no está en PATH (sudo pacman -S cmake)"
    command -v clang >/dev/null || fail "clang no está en PATH (sudo pacman -S clang)"
fi
ok "deps de sistema presentes"

# ─────────────────── 2) Daemon legacy debe estar muerto ───────────────
if systemctl --user is-active jarvis-voice-daemon.service >/dev/null 2>&1; then
    log "jarvis-voice-daemon.service activo — desactivando..."
    systemctl --user disable --now jarvis-voice-daemon.service || true
fi
if systemctl --user list-unit-files 2>/dev/null | grep -q jarvis-voice-daemon; then
    warn "jarvis-voice-daemon.service todavía registrado (lo trae update.sh; OK si es legacy)"
fi
if pgrep -f jarvis-voice-daemon >/dev/null; then
    fail "proceso jarvis-voice-daemon todavía vivo — kill manual antes de seguir"
fi
ok "daemon legacy inactivo"

# ─────────────────── 3) Estado del módulo PipeWire echo-cancel ────────
ECHO_LOADED=0
if pactl list short modules 2>/dev/null | grep -q module-echo-cancel; then
    ECHO_LOADED=1
fi

if [ "$B3_MODE" -eq 1 ]; then
    if [ "$ECHO_LOADED" -eq 1 ]; then
        log "module-echo-cancel cargado — descargando para test B3..."
        pactl unload-module module-echo-cancel 2>/dev/null || true
        rm -f "$HOME/.config/pipewire/pipewire.conf.d/jarvis-echo-cancel.conf"
        systemctl --user restart pipewire pipewire-pulse wireplumber 2>/dev/null || true
        if pactl list short modules 2>/dev/null | grep -q module-echo-cancel; then
            fail "module-echo-cancel sigue cargado tras unload — revisar a mano"
        fi
    fi
    ok "module-echo-cancel descargado (B3: AEC propio)"
else
    if [ "$ECHO_LOADED" -eq 0 ]; then
        warn "module-echo-cancel NO cargado — pre-B3 espera el AEC viejo activo"
        log "intentando recargar via install.sh paso 7a..."
        if [ -f arch/configs/pipewire/echo-cancel.conf ]; then
            mkdir -p "$HOME/.config/pipewire/pipewire.conf.d"
            cp arch/configs/pipewire/echo-cancel.conf \
               "$HOME/.config/pipewire/pipewire.conf.d/jarvis-echo-cancel.conf"
            systemctl --user restart pipewire pipewire-pulse wireplumber 2>/dev/null || true
            sleep 1
        else
            warn "arch/configs/pipewire/echo-cancel.conf no existe (¿ya en B3?)"
        fi
    fi
    pactl list short modules 2>/dev/null | grep -q module-echo-cancel \
        && ok "module-echo-cancel cargado (pre-B3)" \
        || warn "module-echo-cancel sigue sin cargar — feedback loop esperado"
fi

# ─────────────────── 4) Env vars críticas ─────────────────────────────
ENV_FILE="$HOME/.ironclaw/.env"
if [ ! -f "$ENV_FILE" ]; then
    fail "$ENV_FILE no existe — necesita ELEVENLABS_AGENT_ID + ELEVENLABS_API_KEY"
fi

# Carga el .env en un subshell para no contaminar el padre.
# shellcheck disable=SC1090
( set -a; . "$ENV_FILE"; set +a
  [ -n "${ELEVENLABS_AGENT_ID:-}" ] || { echo "ELEVENLABS_AGENT_ID vacío en $ENV_FILE" >&2; exit 1; }
  [ -n "${ELEVENLABS_API_KEY:-}"  ] || { echo "ELEVENLABS_API_KEY vacío en $ENV_FILE"  >&2; exit 1; }
  case "${JARVIS_TTS_BACKEND:-}" in
      elevenlabs_local|elevenlabs-local|elevenlabs_ipc|elevenlabs-ipc|elevenlabs|voice_in_process) : ;;
      *) echo "JARVIS_TTS_BACKEND='${JARVIS_TTS_BACKEND:-<unset>}' (esperaba elevenlabs_local)" >&2; exit 1 ;;
  esac
) || fail "$ENV_FILE inválido — corrige y reintenta"
ok "envs ElevenLabs + JARVIS_TTS_BACKEND OK"

# ─────────────────── 5) Build + install (opcional) ────────────────────
if [ "$DO_BUILD" -eq 1 ]; then
    log "compilando y reinstalando vía arch/install.sh..."
    bash arch/install.sh || fail "install.sh falló"
    ok "binario ironclaw reinstalado"
fi

# ─────────────────── 6) Restart ironclaw ──────────────────────────────
log "reiniciando ironclaw..."
systemctl --user restart ironclaw 2>/dev/null || \
    fail "systemctl --user restart ironclaw falló"

# Espera hasta 10s a que arranque.
for _ in $(seq 1 20); do
    if systemctl --user is-active ironclaw >/dev/null 2>&1; then break; fi
    sleep 0.5
done
systemctl --user is-active ironclaw >/dev/null 2>&1 \
    || fail "ironclaw no quedó activo tras 10s — journalctl --user -u ironclaw"
ok "ironclaw activo"

# ─────────────────── 7) Smoke checks runtime ──────────────────────────
sleep 2  # deja que se asiente

# Sólo el proceso ironclaw debe haber, no el daemon.
if pgrep -fa jarvis-voice-daemon >/dev/null; then
    fail "jarvis-voice-daemon vivo otra vez (¿quedó binario en /usr/local/bin?)"
fi
ok "ningún proceso jarvis-voice-daemon"

# unit file no debería existir tras un fresh install (puede existir si la
# máquina nunca corrió update.sh post-B2.7).
if systemctl --user cat jarvis-voice-daemon.service >/dev/null 2>&1; then
    warn "jarvis-voice-daemon.service todavía existe en /etc o ~/.config — borrarlo:"
    warn "  systemctl --user disable --now jarvis-voice-daemon.service"
    warn "  rm ~/.config/systemd/user/jarvis-voice-daemon.service"
fi

# Buscar errores fatales en el journal de los últimos 30s.
JOURNAL="$(journalctl --user -u ironclaw --since '30 seconds ago' --no-pager 2>/dev/null || true)"
if printf "%s" "$JOURNAL" | grep -qE 'panic|FATAL|backend start.*Err|voice config:'; then
    warn "errores en journal (últimos 30s):"
    printf "%s\n" "$JOURNAL" | grep -E 'panic|FATAL|backend start.*Err|voice config:' | head -10
    fail "ironclaw arrancó con errores — revisa journalctl --user -u ironclaw -f"
fi

# Conexión WS ElevenLabs (busca el log de ws.connecting / ws.connected).
if printf "%s" "$JOURNAL" | grep -q 'ws.conversation_initiated\|ws.connected\|tts audio pipeline started'; then
    ok "WS ElevenLabs inicializado (orquestador in-process activo)"
else
    warn "no detecté logs de WS — puede ser timing; revisa journalctl si la conversación falla"
fi

# ─────────────────── 8) Instrucciones para test humano ────────────────
cat <<'EOF'

┌────────────────────────────────────────────────────────────────────┐
│  HUMAN VALIDATION (5 min con speakerphone ABIERTO)                 │
├────────────────────────────────────────────────────────────────────┤
│  Terminal de logs:                                                 │
│    journalctl --user -u ironclaw -f                                │
│                                                                    │
│  Test interactivo:                                                 │
│    1. Abre Quickshell / jarvis-chat.                               │
│    2. Habla, espera respuesta entera, escucha.                     │
│    3. Repite 4-5 ciclos con pausas largas.                         │
│    4. Deja que jarvis hable 1 frase larga sin interrumpirlo.       │
│                                                                    │
│  Criterios de éxito:                                               │
│    [ ] Conversación end-to-end OK.                                 │
│    [ ] Orbe reacciona al TTS (cyan + bandas activas).              │
│    [ ] Bug del feedback loop: depende del modo                     │
│          - pre-B3 (AEC PipeWire): puede ocurrir, NO bloquea        │
│          - post-B3 (--b3 flag):   NO debe ocurrir en 5 min         │
│                                                                    │
│  Si todo OK → F4 cerrado. git push y actualiza el spec.            │
│  Si falla post-B3 → tunear JARVIS_VOICE_AEC_DELAY_MS (30/80/100)   │
│                    en ~/.ironclaw/.env y bash este script --no-build│
└────────────────────────────────────────────────────────────────────┘

EOF
exit 0
