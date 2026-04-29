# voice_daemon

Voice pipeline para jarvis-os: captura → VAD → wake word → STT → agente → TTS.

**Estado:** F1.3.a — scaffolding. Módulos con stubs `start/stop/process` y
contratos estables. Implementaciones reales en F1.3.b/c.

## Arquitectura

```
sounddevice (mic, 16kHz int16)
    │
    ▼  (chunks 32ms / 512 samples)
silero_vad → VadGate
    │  (solo frames con habla)
    ▼
openwakeword → WakeWordDetector ── (score >= threshold) ──┐
    │                                                     │
    └─ buffer continuo                                    ▼
                                              utterance complete
                                                          │
                                                          ▼
                                             faster_whisper → Transcriber
                                                          │
                                                          ▼ (text)
                                             WebSocket → IronClaw
                                                          │
                                                          ▼ (response text)
                                             ElevenLabs Flash → Speaker
                                                          │
                                                          ▼ (PCM stream)
                                            ┌─────────────┴─────────────┐
                                            ▼                           ▼
                                     sounddevice playback        WebSocket emit
                                       (audio output)            (tts_amplitude)
                                                                       │
                                                                       ▼
                                                                   HUD ring
                                                            (anillo cyan reactivo
                                                             a la voz de Jarvis)
```

## Instalación con uv

```bash
cd voice_daemon/
uv sync
```

La primera vez descarga torch, faster-whisper y los modelos de openWakeWord
(~2-3 GB en disco). En el iMac live (stateless) los modelos vienen ya
horneados en la ISO.

## Ejecutar

```bash
uv run voice-daemon
```

Levanta el servidor en `ws://127.0.0.1:7331` y se queda esperando audio.
Logs en JSON-line por stdout (compatible con journald si systemd-user lo lanza).

## Variables de entorno

Requeridas (cargadas vía `python-dotenv` desde `.env` o `/etc/jarvis/env`):

- `ELEVENLABS_API_KEY` — TTS Flash (ya lo tienes en el `.env` del repo).
- `ELEVENLABS_VOICE_ID` — voice clone o pre-built.
- `ELEVENLABS_MODEL_ID` — `eleven_flash_v2_5` por defecto.

## Protocolo WebSocket

Ver docstring de `voice_daemon/server.py` para los tipos de mensaje.

## Pendiente F1.3.b

- `audio_capture.py` con `sounddevice.InputStream` (chunks 512 samples 16kHz).
- Implementación real de `VadGate.stream`, `WakeWordDetector.stream`,
  `Transcriber.transcribe` (cableado a las libs).
- Cooldown post-wake (evitar disparos múltiples en la misma palabra).
- Buffer de utterance hasta silencio prolongado (Silero gate).

## Pendiente F1.3.c

- `Speaker.synthesize` con stream HTTP de ElevenLabs Flash.
- Playback duplex (sounddevice OutputStream + emit amplitude al hub).
- Manejo de interrupciones (usuario habla mientras Jarvis habla → barge-in).
