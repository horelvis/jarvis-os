# voice_daemon

Voice pipeline para jarvis-os: captura → VAD → wake word → STT → IronClaw → TTS.

**Estado:** F1.3.b implementado (input pipeline real). F1.3.c (TTS playback)
sigue stub: `tts.py:Speaker.synthesize` devuelve un generator vacío. La
arquitectura WebSocket y los contratos de `transcript_final`,
`agent_state`, `wake_detected` y `tts_amplitude` ya son estables.

## Arquitectura

```
sounddevice (mic, 16kHz mono int16, 512 samples/chunk)
    │
    ▼
WakeWordDetector (openwakeword, modelo "hey_jarvis", threshold 0.5)
    │  (en estado idle; cooldown 1s post-utterance)
    ▼
[wake_detected] → state=listening
    │
    ▼
VadGate (silero-vad, threshold 0.5, hysteresis 200ms)
    │  (chunks acumulados en utterance_buffer hasta silencio prolongado)
    ▼
[silencio >= 800ms o duración >= 15s] → close utterance → state=thinking
    │
    ▼
Transcriber (faster-whisper "base", language="es", beam=1)
    │
    ▼
[transcript_final] vía WebSocket → cualquier cliente conectado
    │  (en F1.3 cabledo: IronClaw lee este evento, responde, manda "speak")
    ▼  (Fase 2)
IronClaw → text response → "speak" message
    │
    ▼  (Fase 3)
Speaker (elevenlabs flash v2_5) → PCM stream → sounddevice playback +
                                              tts_amplitude → HUD ring
```

## Instalación con uv

El sistema lleva Python 3.14 pero `pyproject.toml` exige 3.11 (constraint
heredada de modelos cargados con tflite; openwakeword 0.6+ ya soporta
ONNX, en futuras iteraciones se puede subir el rango).

```bash
cd voice_daemon/
uv python install 3.11        # idempotente, descarga 3.11 en sandbox uv
uv sync                        # crea .venv + instala dependencias
```

Primer `uv sync` descarga torch, faster-whisper y modelos
de openWakeWord (~2-3 GB). En el modelo `base` de Whisper son ~140MB
adicionales que se bajan al primer arranque del daemon.

## Ejecutar

```bash
uv run voice-daemon
```

Levanta el servidor en `ws://127.0.0.1:7331` y queda escuchando audio
del micrófono por defecto del sistema. Logs en JSON-line por stdout
(compatible con journald si systemd-user lo lanza).

Lo que verás al primer arranque:

```json
{"event": "daemon.starting", ...}
{"event": "vad.loading", ...}
{"event": "vad.ready", ...}
{"event": "wakeword.downloading_models_if_needed", ...}
{"event": "wakeword.loading", "model": "hey_jarvis", "framework": "onnx"}
{"event": "wakeword.ready", ...}
{"event": "stt.loading", "model": "base", "compute_type": "int8", "device": "cpu"}
{"event": "stt.ready", ...}
{"event": "audio.capture_started", "sample_rate": 16000, "chunk_size": 512}
{"event": "daemon.ready"}
{"event": "pipeline.ready"}
```

A partir de aquí di **"hey jarvis"** seguido de tu pregunta. Verás:

```json
{"event": "wake.detected", "score": 0.78, "model": "hey_jarvis"}
{"event": "utterance.closed", "duration_s": 2.4, "reason": "silence"}
{"event": "transcript.final", "text": "qué hora es", "language": "es"}
```

## Variables de entorno (Fase 3 / TTS)

Cargadas desde uno de:
- `/etc/jarvis/env`
- `~/.ironclaw/.env`
- `$CWD/.env`

Necesarias para F1.3.c:

- `ELEVENLABS_API_KEY` — Flash TTS.
- `ELEVENLABS_VOICE_ID` — voice clone o pre-built.
- `ELEVENLABS_MODEL_ID` — `eleven_flash_v2_5` por defecto.

## Protocolo WebSocket

Ver docstring de `voice_daemon/server.py` para la lista completa de
mensajes. Los eventos que ya emite el daemon en F1.3.b:

- `agent_state` — `"idle"` / `"listening"` / `"thinking"`
- `wake_detected` — palabra clave reconocida
- `transcript_final` — texto del usuario tras transcribir

Aún no emite (Fase 3): `tts_amplitude`, `transcript_partial`.

## Pendiente F1.3 fase 2 (cableado IronClaw)

Cliente intermedio que:
- Lee `transcript_final` del daemon.
- Inyecta el texto en IronClaw (vía CLI o gateway HTTP).
- Recibe la respuesta y manda `{"type":"speak","text":...}` al daemon.

Puede vivir en Rust (preferido, integrarse al binario `ironclaw` como
modo "voice") o como script bash provisional (rápido para validar).

## Pendiente F1.3.c (TTS output)

- `Speaker.synthesize` con stream HTTP de ElevenLabs Flash.
- Playback duplex (sounddevice OutputStream + emit `tts_amplitude` al hub).
- Barge-in: si VAD detecta voz del usuario mientras Jarvis habla, cortar
  el playback y volver a `listening`.
