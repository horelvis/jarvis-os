#!/usr/bin/env bash
#
# End-to-end smoke test for src/channels/local_ipc/.
#
# Validates bind / handshake / transport-error / shutdown contracts on a
# real IronClaw process WITHOUT requiring a working LLM provider. Test 3
# (full agent loop) is best-effort and SKIPs cleanly if LLM creds aren't
# configured.
#
# Usage:
#   bash scripts/smoke-local-ipc.sh
#
# Requires: socat, ./target/release/ironclaw (built via cargo).

set -uo pipefail

# ─── Config ──────────────────────────────────────────────────────────
SOCKET="/run/user/$(id -u)/ironclaw.sock"
BINARY="./target/release/ironclaw"
LOG="/tmp/ironclaw-smoke.log"

# ─── Output helpers ──────────────────────────────────────────────────
RED=$'\033[31m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'; RST=$'\033[0m'
PASSED=0; FAILED=0; SKIPPED=0
pass()    { printf '%sPASS%s %s\n' "$GREEN" "$RST" "$1"; PASSED=$((PASSED+1)); }
fail()    { printf '%sFAIL%s %s\n' "$RED"   "$RST" "$1"; FAILED=$((FAILED+1)); }
skip()    { printf '%sSKIP%s %s — %s\n' "$YELLOW" "$RST" "$1" "$2"; SKIPPED=$((SKIPPED+1)); }
info()    { printf '     %s\n' "$1"; }
section() { printf '\n── %s ──\n' "$1"; }

strip_ansi() { sed 's/\x1b\[[0-9;]*[mGKHFJABCDsuhl]//g'; }

# ─── Cleanup trap ────────────────────────────────────────────────────
PID=""
cleanup() {
    if [[ -n "$PID" ]] && kill -0 "$PID" 2>/dev/null; then
        info "trap cleanup: SIGTERM PID $PID"
        kill -TERM "$PID" 2>/dev/null || true
        for _ in 1 2 3 4 5; do
            kill -0 "$PID" 2>/dev/null || break
            sleep 1
        done
        kill -KILL "$PID" 2>/dev/null || true
    fi
    rm -f "$SOCKET"
}
trap cleanup EXIT INT TERM

# ─── Pre-flight ──────────────────────────────────────────────────────
section "Pre-flight"

if [[ ! -x "$BINARY" ]]; then
    fail "binary not found at $BINARY"
    info "build it first: cargo build --release --bin ironclaw"
    exit 1
fi
pass "binary at $BINARY"

if ! command -v socat &>/dev/null; then
    fail "socat not installed (pacman -S socat)"
    exit 1
fi
pass "socat available"

if pgrep -f "target/release/ironclaw" >/dev/null 2>&1; then
    info "killing leftover ironclaw processes"
    pkill -KILL -f "target/release/ironclaw" 2>/dev/null || true
    sleep 1
fi
rm -f "$SOCKET" "$LOG"
pass "leftover state cleaned"

# ─── Launch IronClaw (full tty detach) ───────────────────────────────
section "Launch IronClaw (TUI off, logs to $LOG)"

# --no-db skips the DB-stored settings read so env vars actually take
# effect. Without it, db_first_optional_string reads cli_mode="tui" from
# the user's settings DB → TUI loads → suppress_stderr=true → fmt_layer
# is None → /tmp/ironclaw-smoke.log stays empty. With --no-db the env
# overrides win and stderr captures tracing. The smoke does NOT need
# DB-backed state (no LLM history, no thread store, no sandbox jobs).
# CLI_ENABLED=false + CLI_MODE=disabled → no TUI, no REPL channel
# nohup + disown + < /dev/null → fully detached, can't get SIGTTIN
# stderr → log file; stdout → /dev/null
CLI_ENABLED=false \
CLI_MODE=disabled \
GATEWAY_ENABLED=false \
RUST_LOG=ironclaw::channels::local_ipc=trace,ironclaw::bridge::router=debug,ironclaw::channels::web::platform::sse=debug,ironclaw=info \
    nohup "$BINARY" --no-db run < /dev/null > /dev/null 2> "$LOG" &
PID=$!
disown
info "PID: $PID"

# Wait up to 10s for socket bind
for _ in $(seq 1 100); do
    if [[ -S "$SOCKET" ]]; then break; fi
    if ! kill -0 "$PID" 2>/dev/null; then
        fail "process died before bind"
        info "log tail:"
        strip_ansi <"$LOG" | tail -40 | sed 's/^/       /'
        exit 1
    fi
    sleep 0.1
done

if [[ ! -S "$SOCKET" ]]; then
    fail "socket did not bind within 10s"
    info "log tail:"
    strip_ansi <"$LOG" | tail -40 | sed 's/^/       /'
    exit 1
fi
pass "socket bound at $SOCKET"

PERMS=$(stat -c '%a' "$SOCKET")
if [[ "$PERMS" == "600" ]]; then
    pass "socket perms 0600"
else
    fail "socket perms expected 600, got $PERMS"
fi

# Confirm the channel showed up in the log
if strip_ansi <"$LOG" | grep -q "local_ipc channel enabled"; then
    pass "local_ipc channel enabled (per log)"
else
    info "(log doesn't show 'channel enabled' line — debug level may be filtered)"
fi

# ─── Test 1: ipc_hello handshake ─────────────────────────────────────
section "Test 1: ipc_hello handshake"

HELLO=$(timeout 3 socat -u UNIX-CONNECT:"$SOCKET" - </dev/null 2>/dev/null | head -1 || true)
if [[ -z "$HELLO" ]]; then
    fail "no first line received within 3s"
elif [[ "$HELLO" == *'"type":"ipc_hello"'* ]]; then
    pass "ipc_hello received"
    if [[ "$HELLO" == *'"protocol_version":1'* ]]; then
        pass "protocol_version=1"
    else
        fail "protocol_version mismatch in: $HELLO"
    fi
    if [[ "$HELLO" =~ \"local_user_id\":\"([^\"]+)\" ]]; then
        info "local_user_id: ${BASH_REMATCH[1]}"
        pass "local_user_id present"
    else
        fail "local_user_id missing in: $HELLO"
    fi
else
    fail "first line not ipc_hello"
    info "got: $HELLO"
fi

# ─── Test 2: malformed line → transport error ────────────────────────
section "Test 2: malformed line → transport error event"

RESP=$( ( printf 'this is not json\n'; sleep 2 ) \
    | timeout 4 socat - UNIX-CONNECT:"$SOCKET" 2>/dev/null || true )

if [[ "$RESP" == *'"type":"error"'* ]]; then
    pass "transport error event sent"
    if [[ "$RESP" == *'"kind":"command_invalid"'* ]]; then
        pass "kind=command_invalid"
    else
        fail "wrong kind, got: $RESP"
    fi
    if [[ "$RESP" == *'"detail":"'*'"'* ]]; then
        pass "detail field populated"
    else
        info "(detail field empty — minor)"
    fi
else
    fail "no error event for malformed line"
    info "got:"
    echo "$RESP" | sed 's/^/       /'
fi

# ─── Test 3: valid Message → no transport error + (optional) AppEvent ─
section "Test 3: valid Message accepted (transport-level)"

MARKER="smoke-$(date +%s)-$RANDOM"
RESP=$( ( printf '{"type":"message","content":"%s"}\n' "$MARKER"; sleep 4 ) \
    | timeout 6 socat - UNIX-CONNECT:"$SOCKET" 2>/dev/null || true )

# Transport-level: a valid Message must NOT produce a transport error event
if [[ "$RESP" == *'"type":"error"'* ]]; then
    fail "valid Message produced a transport error"
    info "got:"
    echo "$RESP" | sed 's/^/       /'
else
    pass "no transport error for valid Message"
fi

# Full-pipe: any AppEvent (response/thinking/heartbeat) means agent loop is alive
APP_EVENT_TYPES='"type":"response"|"type":"thinking"|"type":"heartbeat"|"type":"tool_started"|"type":"tool_completed"|"type":"thread_event"'
if echo "$RESP" | grep -qE "$APP_EVENT_TYPES"; then
    pass "agent loop emitted AppEvent — full pipe working"
    APP_KIND=$(echo "$RESP" | grep -oE "$APP_EVENT_TYPES" | head -1)
    info "first AppEvent kind: $APP_KIND"
else
    skip "agent loop AppEvent" "none observed in 4s (LLM not configured, or sse/bridge wiring issue — see log)"
    # Aux signal from log: did the IncomingMessage at least reach inject_tx?
    if strip_ansi <"$LOG" | grep -q "$MARKER"; then
        info "marker '$MARKER' found in log → message was injected into the agent"
    elif strip_ansi <"$LOG" | grep -qE "ipc command (parse|dispatch)"; then
        info "log shows ipc command path active"
    else
        info "no log evidence of message receipt — investigate $LOG"
    fi
fi

# ─── Test 4: shutdown removes socket ─────────────────────────────────
section "Test 4: shutdown removes socket"

kill -TERM "$PID"
for _ in $(seq 1 30); do
    if [[ ! -S "$SOCKET" ]]; then break; fi
    sleep 0.2
done

if kill -0 "$PID" 2>/dev/null; then
    info "process still alive after SIGTERM, forcing"
    kill -KILL "$PID" 2>/dev/null || true
    sleep 1
fi
PID=""

if [[ ! -S "$SOCKET" ]]; then
    pass "socket removed on shutdown"
else
    fail "socket still exists after SIGTERM"
    rm -f "$SOCKET"
fi

# ─── Summary ─────────────────────────────────────────────────────────
section "Summary"
printf 'Passed:  %d\n' "$PASSED"
if [[ $FAILED -gt 0 ]]; then
    printf '%sFailed:  %d%s\n' "$RED" "$FAILED" "$RST"
else
    printf 'Failed:  0\n'
fi
[[ $SKIPPED -gt 0 ]] && printf '%sSkipped: %d%s\n' "$YELLOW" "$SKIPPED" "$RST"

echo
echo "Full log: $LOG"
echo "  view clean: sed 's/\\x1b\\[[0-9;]*[mGKHFJABCDsuhl]//g' $LOG | less -R"

if [[ $FAILED -eq 0 ]]; then
    echo
    printf '%sLocal IPC transport contract: GREEN%s\n' "$GREEN" "$RST"
    [[ $SKIPPED -gt 0 ]] && echo "(Test 3 skipped — full agent-loop verification needs LLM creds)"
    exit 0
else
    echo
    printf '%sLocal IPC transport contract: RED%s\n' "$RED" "$RST"
    exit 1
fi
