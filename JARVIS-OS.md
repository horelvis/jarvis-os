# jarvis-os

OS Linux integrado y agéntico construido sobre [IronClaw](README.md).
Voz envolvente, HUD circular cyan, control profundo del sistema vía MCP,
todo local con fallback cloud para LLM y TTS.

> Proyecto de estudio personal. Ver `jarvis-os-spec-v0.2.docx.md` para la
> especificación completa (arquitectura, fases F1→F10, políticas, formatos
> de auditoría, paleta visual del HUD).

## Estado actual: F1.3.b — voice input pipeline

Rama de trabajo: **`jarvis-arch-os`** (Arch Linux como base, ver
`legacy/nixos/` para el trabajo NixOS v1-v19 ya histórico).

Fases:

- **F0** ✅ — Bootstrap idempotente Arch (`arch/install.sh` + `arch/update.sh`).
- **F1.1** ✅ — `crates/jarvis_policies/` (decisión ALLOW/CONFIRM/DENY).
- **F1.2** ✅ — `crates/jarvis_linux_mcp/` con 8 tools (process, journal,
  polkit, network, systemd, btrfs, file_read_safe, policy.evaluate).
- **F1.3.a** ✅ — Scaffold `voice_daemon/` con contratos WS estables.
- **F1.3.b** 🚧 — Input pipeline (audio → VAD → wake → STT) **implementado,
  pendiente validar en Asus** (TTY 1 abierto sobre el laptop, ver
  retoma abajo).
- **F1.4** ✅ — install.sh / update.sh / systemd-user / wrappers.
- **F1.3.c** ⏳ — TTS playback ElevenLabs (stub).
- **F1.3 cabledo a IronClaw** ⏳ — `transcript_final` → `ironclaw chat` →
  `speak`.
- **F2** ⏳ — HUD widgets distribuidos (EWW + Tauri ring).

## Hardware

| Rol | Equipo | Specs |
|-----|--------|-------|
| Dev box | bestia local | i9-14900K (32 hilos), RTX 4090 24 GB, 62 GB RAM, Ubuntu 24.04 |
| Target principal | Asus ZenBook UX431FLC | i7-8565U, NVIDIA MX250 2 GB, Intel UHD 620, 16 GB RAM, mic Realtek HDA, audio PipeWire |

iMac 2014 fue target original (v1-v18) pero quedó descartado por fricción
(5K MST, AMD Tonga, applesmc, Broadcom WiFi propietaria). El iMac ya no es
prioridad — todo el desarrollo va sobre el Asus.

## Estructura del repo

```
arch/                       # Bootstrap + mantenimiento sobre Arch base
  install.sh                # 1 vez tras archinstall — idempotente
  update.sh                 # cada git pull — recompila + reinstala lo que cambió
  configs/hyprland/         # overrides que update.sh appendea a custom/*.conf
    general.conf            # input es, gestures, borde cyan
    keybinds.conf           # Super+Y/N inline confirm F1.5
  scripts/jarvis-chat       # wrapper que sourcea ~/.ironclaw/.env
  systemd-user/             # services que arrancan en sesión Hyprland
    jarvis-mcp-register.service
    jarvis-voice-daemon.service
  templates/ironclaw.env    # plantilla del .env sin keys

crates/
  jarvis_policies/          # ALLOW/CONFIRM/DENY decision engine
  jarvis_linux_mcp/         # MCP server stdio con 8 tools de Linux

voice_daemon/               # F1.3 — Python uv sandbox 3.11
  voice_daemon/
    audio_capture.py        # sounddevice 16kHz mono int16 chunks 512
    vad.py                  # silero-vad torch JIT
    wakeword.py             # openwakeword "hey_jarvis" ONNX
    stt.py                  # faster-whisper base int8 español
    tts.py                  # stub F1.3.c (ElevenLabs)
    server.py               # WebSocket :7331 JSON line-by-line
    main.py                 # FSM idle→listening→thinking→idle
  scripts/
    setup.sh                # bootstrap idempotente (uv python + uv sync + modelos)
    run.sh                  # arranca con logs filtrados/coloreados

src/                        # IronClaw core (Rust workspace)
crates/                     # IronClaw crates extraídos
assets/wallpaper.jpg        # wallpaper cyan oficial jarvis-os
```

## Cómo retomar tras una pausa larga

Suponiendo que vienes a una sesión nueva con el laptop apagado y nada más
en la cabeza:

### 1. En el dev box (este equipo)

```bash
cd /home/nexus/git/jarvis-os
git status                                      # rama y cambios pendientes
git log --oneline -10                           # últimos commits
cat /home/nexus/.claude/projects/-home-nexus-git-jarvis-os/memory/MEMORY.md
```

`MEMORY.md` indexa todo el estado conversacional acumulado entre
sesiones. Lee los pointers que parezcan relevantes.

### 2. En el laptop Asus (jarvis-asus)

Boot, login al usuario `jarvis`, en TTY1 escribe `Hyprland` (o configura
auto-start vía display manager más adelante). Una vez en Hyprland abre
una terminal y:

```bash
cd /opt/jarvis-os
git pull
./arch/update.sh                # aplica cambios al sistema
```

Para arrancar IronClaw (modo agente texto):

```bash
jarvis-chat run
```

Para arrancar el voice daemon (modo voz, F1.3.b):

```bash
cd /opt/jarvis-os/voice_daemon
./scripts/setup.sh              # 1 vez tras `git pull` con cambios en deps
./scripts/run.sh                # arranca con logs filtrados (recomendado)
# O directo:
uv run voice-daemon
```

Luego dices **"hey jarvis, qué hora es"** y el log muestra
`wake.detected` → `transcript.final`.

## Comandos básicos

### Compilar y aplicar cambios

```bash
cd /opt/jarvis-os
./arch/update.sh                # cargo build + reinstala binarios cambiados
```

### Verificar que jarvis-linux MCP está vivo

```bash
ironclaw mcp list               # debe listar jarvis-linux registrado
ironclaw mcp test jarvis-linux  # debe listar 8 tools disponibles
```

### Logs systemd-user

```bash
journalctl --user -u jarvis-mcp-register -f
journalctl --user -u jarvis-voice-daemon -f
```

### Rollback Btrfs (equivalente a `nixos-rebuild --rollback`)

GRUB muestra entries de snapshots de snapper. Reboot → elegir snapshot
anterior. snapper-timeline + grub-btrfsd corren auto.

## Stack técnico (decisiones cerradas)

| Capa | Elección | Razón |
|------|----------|-------|
| Distro base | **Arch Linux** | Rolling, control fino, sin abstracción Nix sobre comunidad shell |
| Compositor | Hyprland + illogical-impulse (end-4) | Quickshell/Material You, AI panel built-in |
| Persistencia | Disco persistente Btrfs + snapper | Rollback equivalente a generations |
| Idioma teclado | es | Default jarvis-os baked en `arch/configs/hyprland/general.conf` |
| Lenguaje núcleo | Rust 1.92 | IronClaw + crates jarvis_* |
| Lenguaje voz | Python 3.11 (uv sandbox) | openWakeWord legacy py constraint, aislado del system Python 3.14 |
| Voz STT | faster-whisper base/int8 español | CPU-friendly, latencia <2s |
| Voz wake | openwakeword "hey_jarvis" ONNX | Built-in, threshold 0.5 |
| Voz VAD | silero-vad torch JIT | ~1.5MB, hysteresis 200ms |
| Voz TTS | ElevenLabs Flash v2_5 (cloud) | F1.3.c — calidad alta, $5/mes uso normal |
| LLM main | Claude Sonnet 4.6 (Anthropic API) | Coste razonable con prompt caching |
| DB IronClaw | libSQL local en `~/.ironclaw/jarvis.db` | Stateless por defecto, opt-in Turso sync |
| Políticas | crate `jarvis_policies` extiende `ironclaw_safety` | No OPA externo |

## Hitos alcanzados

- **2026-04-29 — Hito v7**: primera conversación tool-calling end-to-end
  en Asus. `jarvis-chat run` → "Lista los 5 procesos que más memoria
  consumen" → IronClaw razona → invoca `process.list` MCP → respuesta
  natural ("qs es el que más consume"). jarvis-os vivo en hardware target.

## Pendiente próximo

- Validar voice_daemon en Asus (`./scripts/setup.sh` + `./scripts/run.sh`,
  decir "hey jarvis qué hora es", verificar transcript correcto).
- Cabledo voice → IronClaw: cliente Rust o bash que lea
  `transcript_final` del WS, inyecte a `ironclaw chat`, mande respuesta
  como `speak` al daemon.
- F1.3.c TTS: `Speaker.synthesize` real con ElevenLabs Flash + playback
  duplex sounddevice + emit `tts_amplitude` al hub.
- F2 HUD ring: Tauri layer-shell, anillo cyan WebGL reactivo a
  `tts_amplitude`.

## Referencias

- Spec canónica: `jarvis-os-spec-v0.2.docx.md`
- IronClaw upstream (este mismo repo): [README.md](README.md)
- IronClaw ↔ OpenClaw paridad: [FEATURE_PARITY.md](FEATURE_PARITY.md)
- Hyprland docs: <https://wiki.hyprland.org/>
- end-4/dots-hyprland: <https://github.com/end-4/dots-hyprland>
- openWakeWord: <https://github.com/dscripka/openWakeWord>
- silero-vad: <https://github.com/snakers4/silero-vad>
- faster-whisper: <https://github.com/SYSTRAN/faster-whisper>
