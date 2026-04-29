"""Captura de audio del micrófono vía sounddevice / PortAudio.

PortAudio detecta automáticamente PipeWire en Linux moderno (Arch +
WirePlumber), así que pedimos 16kHz mono int16 y el resampling lo hace
PipeWire en el camino sin código específico nuestro.

Frame size: 512 samples a 16kHz = 32ms. Es el tamaño exacto que Silero
VAD espera y un múltiplo razonable para el buffer interno de
openWakeWord (que internamente usa ventanas de 80ms).
"""

from __future__ import annotations

import asyncio
from collections.abc import AsyncIterator
from dataclasses import dataclass
from typing import Any

import numpy as np
import structlog

log = structlog.get_logger(__name__)


@dataclass
class CaptureConfig:
    sample_rate: int = 16000
    chunk_size: int = 512  # 32ms a 16kHz: óptimo para Silero VAD.
    channels: int = 1
    dtype: str = "int16"
    queue_max: int = 64  # ~2s de buffering ante stalls del consumer.


class AudioCapture:
    """Captura mono 16kHz int16 a chunks fijos.

    Uso:
        cap = AudioCapture()
        await cap.start()
        async for chunk in cap.stream():
            ...   # chunk: np.ndarray shape (chunk_size,) dtype int16
        await cap.stop()
    """

    def __init__(self, config: CaptureConfig | None = None) -> None:
        self.config = config or CaptureConfig()
        self._queue: asyncio.Queue[np.ndarray] | None = None
        self._loop: asyncio.AbstractEventLoop | None = None
        self._stream: Any = None  # sd.InputStream

    async def start(self) -> None:
        # Lazy import: sounddevice abre el lib de PortAudio en el constructor.
        import sounddevice as sd  # type: ignore[import-not-found]

        self._loop = asyncio.get_running_loop()
        self._queue = asyncio.Queue(maxsize=self.config.queue_max)

        def callback(
            indata: np.ndarray,
            frames: int,
            _time_info: Any,
            status: Any,
        ) -> None:
            # Llamado desde el hilo de PortAudio. Mantener mínimo: copiar y
            # encolar atómicamente desde el event loop.
            if status:
                log.warning("audio.input_status", status=str(status))
            # indata: shape (frames, channels). Forzar 1D mono int16.
            mono = np.ascontiguousarray(indata[:, 0])
            assert self._loop is not None
            assert self._queue is not None
            try:
                self._loop.call_soon_threadsafe(self._queue.put_nowait, mono)
            except RuntimeError:
                # loop cerrado durante shutdown.
                pass

        self._stream = sd.InputStream(
            samplerate=self.config.sample_rate,
            blocksize=self.config.chunk_size,
            channels=self.config.channels,
            dtype=self.config.dtype,
            callback=callback,
        )
        self._stream.start()
        log.info(
            "audio.capture_started",
            sample_rate=self.config.sample_rate,
            chunk_size=self.config.chunk_size,
            channels=self.config.channels,
        )

    async def stream(self) -> AsyncIterator[np.ndarray]:
        if self._queue is None:
            raise RuntimeError("call start() before stream()")
        while True:
            chunk = await self._queue.get()
            yield chunk

    async def stop(self) -> None:
        if self._stream is not None:
            self._stream.stop()
            self._stream.close()
            self._stream = None
        log.info("audio.capture_stopped")
