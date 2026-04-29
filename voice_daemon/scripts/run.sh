#!/usr/bin/env bash
# voice_daemon/scripts/run.sh — arranca voice-daemon con logs legibles.
#
# El daemon emite JSON line-by-line. Para una sesión interactiva en la
# consola, esto extrae los campos clave (event, score, text, ...) y los
# colorea, dejando el JSON crudo en /tmp/voice-daemon-*.log para debug.
#
# Manejo de señales:
#   `exec uv run` reemplaza el shell con el daemon, así Ctrl+C envía
#   SIGINT directamente al daemon (sin un pipe `| jq` que se quede con
#   el foreground). El formateo JSON corre detrás de un process
#   substitution `> >(...)`, en un sub-proceso paralelo que no captura
#   SIGINT; cuando uv run termina, el pipe se cierra y el formatter
#   sale solo.

set -euo pipefail

VOICE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_FILE="/tmp/voice-daemon-$(date +%Y%m%d-%H%M%S).log"

cd "$VOICE_DIR"

if ! command -v jq >/dev/null 2>&1; then
    echo "[voice-run] jq no disponible — corriendo daemon con logs JSON crudos"
    exec uv run voice-daemon
fi

echo "[voice-run] log completo: $LOG_FILE"
echo "[voice-run] arrancando daemon... (Ctrl+C para parar)"
echo ""

# shellcheck disable=SC2016
JQ_FILTER='
    if .event == "wake.detected" then
        "[36m🟢 wake [0m  score=\(.score) model=\(.model)"
    elif .event == "utterance.closed" then
        "[33m📝 utterance closed[0m  duration=\(.duration_s)s reason=\(.reason)"
    elif .event == "transcript.final" then
        "[32m✓ transcript:[0m \"\(.text)\" (\(.language))"
    elif .event == "utterance.aborted_no_speech" then
        "[33m⊘ aborted: no speech[0m"
    elif .event == "transcribe.failed" then
        "[31m✗ transcribe failed: \(.error)[0m"
    elif .event == "wakeword.score_max" then
        "[90m  wake score_max=\(.score_max) (threshold=\(.threshold))[0m"
    elif .event == "audio.queue_full_dropping" then
        "[33m⚠ audio queue full, dropped=\(.total_dropped)[0m"
    elif .event | startswith("daemon.") then
        "[90m· \(.event)[0m"
    elif .event | startswith("pipeline.") then
        "[90m· \(.event)[0m"
    elif .event | endswith(".ready") then
        "[90m· \(.event)[0m"
    else
        empty
    end'

# exec reemplaza el shell con uv run; > >(...) redirige stdout a un
# tee+jq paralelo que no se mete en el job de foreground. Resultado:
# Ctrl+C llega al daemon, no al jq.
exec uv run voice-daemon > >(tee "$LOG_FILE" | jq --unbuffered -r "$JQ_FILTER") 2>&1
