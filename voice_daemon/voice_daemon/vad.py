"""Voice Activity Detection (Silero VAD).

Silero VAD es un modelo Torch JIT minúsculo (~1.5MB) que devuelve, para
cada frame de audio, una probabilidad de "está hablando alguien". Se usa
como gate antes del wake word y antes del STT para no mandar silencio al
modelo y ahorrar inferencia.
"""

from __future__ import annotations

from collections.abc import AsyncIterator
from dataclasses import dataclass

import numpy as np
import structlog

log = structlog.get_logger(__name__)


@dataclass
class VadFrame:
    """Resultado de evaluar un frame contra el VAD."""

    is_speech: bool
    probability: float  # 0..1
    audio: np.ndarray  # frame original int16, para pasar al siguiente stage


class VadGate:
    """Gate basado en Silero VAD.

    Procesa frames de audio (16kHz, int16, 30ms o 512 samples) y deja pasar
    solo los marcados como habla. Mantiene una pequeña ventana de hysteresis:
    una vez detectada habla, mantiene el gate abierto unos N ms tras silencio
    para no cortar palabras.
    """

    def __init__(
        self,
        sample_rate: int = 16000,
        threshold: float = 0.5,
        hysteresis_ms: int = 200,
    ) -> None:
        self.sample_rate = sample_rate
        self.threshold = threshold
        self.hysteresis_ms = hysteresis_ms
        self._model = None
        self._open_until_sample: int = 0
        self._cursor_sample: int = 0

    async def start(self) -> None:
        """Carga el modelo Silero JIT."""
        from silero_vad import load_silero_vad  # type: ignore[import-not-found]

        log.info("vad.loading")
        self._model = load_silero_vad()
        log.info("vad.ready", threshold=self.threshold, hysteresis_ms=self.hysteresis_ms)

    async def stream(
        self, audio_chunks: AsyncIterator[np.ndarray]
    ) -> AsyncIterator[VadFrame]:
        """Procesa chunks y emite VadFrame para cada uno.

        Los chunks deben tener 512 samples (32ms a 16kHz) para el modelo
        Silero. El caller (audio_capture.py futuro) garantiza ese tamaño.
        """
        if self._model is None:
            raise RuntimeError("call start() before stream()")

        # Scaffold — implementación real en F1.3.b.
        async for _chunk in audio_chunks:
            pass
        if False:  # pragma: no cover
            yield VadFrame(is_speech=False, probability=0.0, audio=np.zeros(512, dtype=np.int16))

    async def stop(self) -> None:
        self._model = None
        log.info("vad.stopped")
