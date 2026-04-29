"""Wake word detection vía openWakeWord.

openWakeWord es un detector ligero CPU-only que evalúa frames de 80ms
contra modelos pre-entrenados (built-in: "hey jarvis", "alexa", "ok google",
etc.). Para custom wake words ("hey jarvis-os") se entrena un modelo propio
en F1.3.b — por ahora usamos el built-in "hey_jarvis".
"""

from __future__ import annotations

from collections.abc import AsyncIterator
from dataclasses import dataclass

import numpy as np
import structlog

log = structlog.get_logger(__name__)


@dataclass
class WakeEvent:
    """Evento emitido cuando se detecta la palabra de activación."""

    model_name: str
    score: float  # 0..1; threshold típico 0.5
    timestamp: float  # epoch seconds


class WakeWordDetector:
    """Wrapper sobre openWakeWord, async-friendly.

    Uso:
        detector = WakeWordDetector(model_name="hey_jarvis", threshold=0.5)
        async for event in detector.stream(audio_chunks):
            ...
    """

    def __init__(
        self,
        model_name: str = "hey_jarvis",
        threshold: float = 0.5,
        sample_rate: int = 16000,
    ) -> None:
        self.model_name = model_name
        self.threshold = threshold
        self.sample_rate = sample_rate
        self._model = None  # cargado en start() para tardar menos en import

    async def start(self) -> None:
        """Carga el modelo. Llamar una vez antes de stream()."""
        # Import perezoso: openwakeword arrastra onnx que tarda en importar.
        # Sin esto, el arranque del daemon se siente lento aunque no se use.
        from openwakeword.model import Model  # type: ignore[import-not-found]

        log.info("wakeword.loading", model=self.model_name)
        self._model = Model(wakeword_models=[self.model_name])
        log.info("wakeword.ready", model=self.model_name, threshold=self.threshold)

    async def stream(
        self, audio_chunks: AsyncIterator[np.ndarray]
    ) -> AsyncIterator[WakeEvent]:
        """Procesa chunks de audio (16kHz int16) y yield-ea WakeEvent
        cuando supera el threshold.

        El cooldown post-detección (para evitar disparos múltiples sobre la
        misma palabra) se maneja a nivel de pipeline orquestador, no aquí.
        """
        if self._model is None:
            raise RuntimeError("call start() before stream()")

        # NOTA: scaffold F1.3 — la implementación real aún no consume audio.
        # En F1.3.b cableamos `self._model.predict(chunk)` y emitimos el
        # WakeEvent cuando score >= threshold.
        async for _chunk in audio_chunks:
            # placeholder — silencia warning de unused
            pass
        # yield para que el tipo de retorno sea AsyncIterator
        if False:  # pragma: no cover
            yield WakeEvent(model_name=self.model_name, score=0.0, timestamp=0.0)

    async def stop(self) -> None:
        """Libera recursos del modelo."""
        self._model = None
        log.info("wakeword.stopped")
