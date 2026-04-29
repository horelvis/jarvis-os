"""Entry point del voice_daemon.

Orquesta el pipeline completo:

    audio_capture → vad → wakeword → stt → IronClaw (vía socket)
                                              → speak → tts → playback + amplitude

El estado del agente (idle/listening/thinking/speaking) se publica en el
mismo Server WebSocket para que el HUD lo refleje en tiempo real.
"""

from __future__ import annotations

import asyncio
import os
import signal
import sys
from pathlib import Path

import structlog
from dotenv import load_dotenv

from voice_daemon.server import Server, ServerConfig
from voice_daemon.stt import Transcriber
from voice_daemon.tts import Speaker
from voice_daemon.vad import VadGate
from voice_daemon.wakeword import WakeWordDetector


def _configure_logging() -> None:
    """structlog → JSON line para que journald/lectores estructurados parseen."""
    structlog.configure(
        processors=[
            structlog.processors.add_log_level,
            structlog.processors.TimeStamper(fmt="iso"),
            structlog.processors.JSONRenderer(),
        ],
        wrapper_class=structlog.make_filtering_bound_logger(20),  # INFO+
    )


def _load_env() -> None:
    """Carga .env del repo (modo dev) o /etc/jarvis/env (modo ISO).

    En la ISO live stateless, las API keys se inyectan al boot vía un
    archivo dropeado en /etc/jarvis/env desde la partición FAT del USB
    (ver decisión de persistencia en project_resume_point.md).
    """
    candidates = [
        Path("/etc/jarvis/env"),
        Path.cwd() / ".env",
        Path(__file__).parent.parent.parent / ".env",
    ]
    for p in candidates:
        if p.is_file():
            load_dotenv(p)
            return
    # Sin .env no es error fatal; algunas tools (TTS) avisan al fallar.


async def _run() -> None:
    log = structlog.get_logger("voice_daemon.main")
    log.info("daemon.starting", pid=os.getpid())

    # Componentes del pipeline (todos async, todos lazy-loadable).
    server = Server(ServerConfig())
    wake = WakeWordDetector()
    vad = VadGate()
    stt = Transcriber()
    tts = Speaker()

    # Inicialización en orden — el server primero, así si algo más falla
    # el HUD ya está conectado y puede mostrar el error.
    await server.start()
    await wake.start()
    await vad.start()
    await stt.start()
    await tts.start()

    await server.emit({"type": "agent_state", "state": "idle"})

    # Pipeline orquestación queda como TODO F1.3.b — necesita audio_capture.py
    # con sounddevice + cooldowns + manejo de utterance boundaries.
    log.info("daemon.ready", note="pipeline orchestration pending F1.3.b")

    # Mantener el daemon vivo hasta SIGTERM/SIGINT.
    stop_event = asyncio.Event()

    def _on_signal() -> None:
        log.info("daemon.signal_received")
        stop_event.set()

    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, _on_signal)

    await stop_event.wait()

    # Shutdown ordenado.
    log.info("daemon.stopping")
    await tts.stop()
    await stt.stop()
    await vad.stop()
    await wake.stop()
    await server.stop()
    log.info("daemon.stopped")


def main() -> int:
    """Punto de entrada del binary `voice-daemon` (definido en pyproject.toml)."""
    _configure_logging()
    _load_env()
    try:
        asyncio.run(_run())
    except KeyboardInterrupt:
        return 130
    return 0


if __name__ == "__main__":
    sys.exit(main())
