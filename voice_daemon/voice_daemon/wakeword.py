"""Wake word detection vía openWakeWord.

openWakeWord mantiene su propio sliding window interno y acepta chunks
de cualquier longitud (16-bit PCM mono 16kHz). Devuelve, en cada
predict(), un dict de scores por modelo cargado.

Usamos el modelo built-in `hey_jarvis`. Para custom wake words
(`"hey jarvis-os"`) habría que entrenar uno propio — fuera de F1.3.b.
"""

from __future__ import annotations

import time
from dataclasses import dataclass
from typing import Any

import numpy as np
import structlog

log = structlog.get_logger(__name__)


@dataclass
class WakeEvent:
    """Evento emitido cuando se detecta la palabra de activación."""

    model_name: str
    score: float
    timestamp: float


class WakeWordDetector:
    """Wrapper sobre openWakeWord, con feed síncrono.

    El cooldown post-detección (evitar disparos múltiples sobre la
    misma palabra) se aplica en el orquestador, no aquí: el orquestador
    sabe cuándo el agente está en cooldown porque acaba de procesar
    una utterance.
    """

    def __init__(
        self,
        model_name: str = "hey_jarvis",
        threshold: float = 0.5,
        sample_rate: int = 16000,
        inference_framework: str = "onnx",
    ) -> None:
        self.model_name = model_name
        self.threshold = threshold
        self.sample_rate = sample_rate
        self.inference_framework = inference_framework
        self._model: Any = None

    async def start(self) -> None:
        # Lazy import: onnxruntime + openwakeword pesan al importar.
        import openwakeword  # type: ignore[import-not-found]
        from openwakeword.model import Model  # type: ignore[import-not-found]

        log.info("wakeword.downloading_models_if_needed")
        # Descarga modelos pre-trained la primera vez (idempotente).
        openwakeword.utils.download_models()

        log.info(
            "wakeword.loading",
            model=self.model_name,
            framework=self.inference_framework,
        )
        self._model = Model(
            wakeword_models=[self.model_name],
            inference_framework=self.inference_framework,
        )
        log.info(
            "wakeword.ready",
            model=self.model_name,
            threshold=self.threshold,
        )

    def feed(self, chunk: np.ndarray) -> WakeEvent | None:
        """Pasa un chunk al detector. Devuelve WakeEvent si dispara.

        openwakeword.Model.predict() acepta arrays int16 16kHz mono de
        cualquier longitud y mantiene su propio sliding window de 80ms.
        El score que devuelve se evalúa sobre la ventana actual.
        """
        if self._model is None:
            raise RuntimeError("call start() before feed()")

        scores = self._model.predict(chunk)
        score = float(scores.get(self.model_name, 0.0))
        if score >= self.threshold:
            return WakeEvent(
                model_name=self.model_name,
                score=score,
                timestamp=time.time(),
            )
        return None

    async def stop(self) -> None:
        self._model = None
        log.info("wakeword.stopped")
