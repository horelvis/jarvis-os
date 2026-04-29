"""Entry point del voice_daemon — pipeline orquestador F1.3.b.

FSM:
    idle → (wake_detected) → listening → (silence_timeout) → thinking
    thinking → (transcribe done) → idle (cooldown)

Cada chunk de audio (32ms) pasa por:
    1. WakeWord (siempre, salvo cooldown post-utterance).
    2. VAD (sólo durante listening, para detectar fin de habla).

El audio del utterance se acumula desde el momento del wake hasta que
VAD permanece silencioso por SILENCE_TIMEOUT_MS, o hasta MAX_UTTERANCE_S.
Tras transcribir se emite `transcript_final` por WebSocket y se vuelve
a idle con un cooldown de COOLDOWN_MS para no auto-disparar otra wake
sobre la cola del propio audio.
"""

from __future__ import annotations

import asyncio
import os
import signal
import sys
import time
from pathlib import Path

import numpy as np
import structlog
from dotenv import load_dotenv

from voice_daemon.audio_capture import AudioCapture, CaptureConfig
from voice_daemon.server import Server, ServerConfig
from voice_daemon.stt import Transcriber
from voice_daemon.tts import Speaker
from voice_daemon.vad import VadGate
from voice_daemon.wakeword import WakeWordDetector

# Parámetros del FSM (ms salvo donde se indique).
SILENCE_TIMEOUT_MS = 800
COOLDOWN_MS = 1000
MAX_UTTERANCE_S = 15.0


def _configure_logging() -> None:
    structlog.configure(
        processors=[
            structlog.processors.add_log_level,
            structlog.processors.TimeStamper(fmt="iso"),
            structlog.processors.JSONRenderer(),
        ],
        wrapper_class=structlog.make_filtering_bound_logger(20),  # INFO+
    )


def _load_env() -> None:
    candidates = [
        Path("/etc/jarvis/env"),
        Path.home() / ".ironclaw" / ".env",
        Path.cwd() / ".env",
        Path(__file__).parent.parent.parent / ".env",
    ]
    for p in candidates:
        if p.is_file():
            load_dotenv(p)
            return


async def _pipeline(
    capture: AudioCapture,
    wake: WakeWordDetector,
    vad: VadGate,
    stt: Transcriber,
    server: Server,
    log: structlog.BoundLogger,
    stop_event: asyncio.Event,
) -> None:
    """Bucle único que combina wake + VAD + buffer + STT en serie."""
    state: str = "idle"
    cooldown_until: float = 0.0
    utterance: list[np.ndarray] = []
    last_speech_at: float = 0.0
    listening_started_at: float = 0.0

    log.info("pipeline.ready")

    async for chunk in capture.stream():
        if stop_event.is_set():
            break
        now = time.monotonic()

        if state == "idle":
            if now < cooldown_until:
                continue
            wake_evt = wake.feed(chunk)
            if wake_evt is not None:
                log.info(
                    "wake.detected",
                    score=wake_evt.score,
                    model=wake_evt.model_name,
                )
                await server.emit(
                    {
                        "type": "wake_detected",
                        "model": wake_evt.model_name,
                        "score": wake_evt.score,
                        "ts": wake_evt.timestamp,
                    }
                )
                state = "listening"
                utterance = []
                listening_started_at = now
                last_speech_at = now
                await server.emit(
                    {"type": "agent_state", "state": "listening"}
                )
            continue

        if state == "listening":
            vad_frame = vad.feed(chunk)
            utterance.append(chunk)
            if vad_frame.is_speech:
                last_speech_at = now

            silence_long = (now - last_speech_at) * 1000 >= SILENCE_TIMEOUT_MS
            too_long = (now - listening_started_at) >= MAX_UTTERANCE_S
            # Sólo cierra utterance si ya hubo al menos algo de habla — si
            # la wake disparó por ruido y nunca hubo speech, evitamos
            # transcribir sólo silencio. last_speech_at parte igual a
            # listening_started_at, así que comparamos contra él.
            spoke = last_speech_at > listening_started_at
            if (silence_long and spoke) or too_long:
                state = "thinking"
                await server.emit(
                    {"type": "agent_state", "state": "thinking"}
                )
                audio_full = np.concatenate(utterance)
                duration_s = len(audio_full) / 16000.0
                log.info(
                    "utterance.closed",
                    samples=len(audio_full),
                    duration_s=round(duration_s, 2),
                    reason="silence" if silence_long else "max_duration",
                )
                try:
                    result = await asyncio.to_thread(
                        stt.transcribe, audio_full
                    )
                    log.info(
                        "transcript.final",
                        text=result.text,
                        language=result.language,
                    )
                    await server.emit(
                        {
                            "type": "transcript_final",
                            "text": result.text,
                            "language": result.language,
                        }
                    )
                except Exception as e:  # noqa: BLE001
                    log.error("transcribe.failed", error=str(e))

                utterance = []
                state = "idle"
                cooldown_until = now + (COOLDOWN_MS / 1000.0)
                await server.emit(
                    {"type": "agent_state", "state": "idle"}
                )
            elif silence_long and not spoke:
                # Wake disparó pero no hubo habla — abortar sin transcribir.
                log.info("utterance.aborted_no_speech")
                utterance = []
                state = "idle"
                cooldown_until = now + (COOLDOWN_MS / 1000.0)
                await server.emit(
                    {"type": "agent_state", "state": "idle"}
                )


async def _run() -> None:
    log = structlog.get_logger("voice_daemon.main")
    log.info("daemon.starting", pid=os.getpid())

    server = Server(ServerConfig())
    capture = AudioCapture(CaptureConfig())
    wake = WakeWordDetector()
    vad = VadGate()
    stt = Transcriber()
    tts = Speaker()

    await server.start()
    await capture.start()
    await wake.start()
    await vad.start()
    await stt.start()
    await tts.start()

    await server.emit({"type": "agent_state", "state": "idle"})

    stop_event = asyncio.Event()

    def _on_signal() -> None:
        log.info("daemon.signal_received")
        stop_event.set()

    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, _on_signal)

    pipeline_task = asyncio.create_task(
        _pipeline(capture, wake, vad, stt, server, log, stop_event)
    )

    log.info("daemon.ready")

    await stop_event.wait()

    pipeline_task.cancel()
    try:
        await pipeline_task
    except asyncio.CancelledError:
        pass

    log.info("daemon.stopping")
    await tts.stop()
    await stt.stop()
    await vad.stop()
    await wake.stop()
    await capture.stop()
    await server.stop()
    log.info("daemon.stopped")


def main() -> int:
    _configure_logging()
    _load_env()
    try:
        asyncio.run(_run())
    except KeyboardInterrupt:
        return 130
    return 0


if __name__ == "__main__":
    sys.exit(main())
