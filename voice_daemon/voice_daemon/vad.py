"""Voice Activity Detection (Silero VAD).

Silero VAD es un modelo Torch JIT minúsculo (~1.5MB) que devuelve, para
cada frame de audio, una probabilidad de "está hablando alguien". Se usa
como gate para decidir si un chunk forma parte del utterance del user
(post-wake) o si estamos en silencio y debemos cerrar la utterance.

Silero exige frames de **exactamente** 512 samples a 16kHz (o 256 a 8kHz).
audio_capture.py fija blocksize=512 para alinear con esto.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

import numpy as np
import structlog

log = structlog.get_logger(__name__)


@dataclass
class VadFrame:
    """Resultado de evaluar un frame contra el VAD.

    `is_speech` incluye hysteresis (gate sigue abierto N ms tras la última
    detección de habla), `probability` es el output crudo del modelo.
    """

    is_speech: bool
    probability: float
    audio: np.ndarray


class VadGate:
    """Gate basado en Silero VAD con hysteresis post-speech.

    Una vez que el modelo emite prob >= threshold, mantiene `is_speech=True`
    durante hysteresis_ms más, para no recortar el final de las palabras
    cuando la energía cae brevemente.
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
        self._model: Any = None
        self._open_until_sample: int = 0
        self._cursor_sample: int = 0

    async def start(self) -> None:
        from silero_vad import load_silero_vad  # type: ignore[import-not-found]

        log.info("vad.loading")
        self._model = load_silero_vad()
        log.info(
            "vad.ready",
            threshold=self.threshold,
            hysteresis_ms=self.hysteresis_ms,
        )

    def feed(self, chunk: np.ndarray) -> VadFrame:
        """Evalúa un chunk de 512 samples int16 y devuelve VadFrame.

        Llamada síncrona — Silero VAD corre en CPU y la inferencia tarda
        <1ms por frame. Devolver inmediatamente desde el orquestador es
        más simple que un AsyncIterator y permite combinar con WakeWord
        sobre el mismo chunk.
        """
        if self._model is None:
            raise RuntimeError("call start() before feed()")

        import torch  # lazy: torch se importa una vez al primer feed.

        # int16 → float32 [-1, 1].
        audio_f32 = chunk.astype(np.float32) / 32768.0
        audio_tensor = torch.from_numpy(audio_f32)

        # Silero acepta tensor 1-D mono.
        prob = float(self._model(audio_tensor, self.sample_rate).item())

        if prob >= self.threshold:
            # Refresca la ventana hysteresis: gate permanece abierto hasta
            # cursor + hysteresis_ms.
            hyst_samples = self.hysteresis_ms * self.sample_rate // 1000
            self._open_until_sample = (
                self._cursor_sample + len(chunk) + hyst_samples
            )

        gate_open = self._cursor_sample < self._open_until_sample
        self._cursor_sample += len(chunk)

        return VadFrame(
            is_speech=gate_open,
            probability=prob,
            audio=chunk,
        )

    async def stop(self) -> None:
        self._model = None
        log.info("vad.stopped")
