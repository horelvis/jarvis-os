"""Speech-to-text vía Faster-Whisper distil-large-v3.

distil-large-v3 es la variante destilada de Whisper large-v3: ~6× más
rápida con calidad muy similar (WER ~1.5% más alto). Faster-Whisper usa
CTranslate2 como backend, optimizado para CPU/GPU con int8 quantization.

En el iMac 2014 (Haswell, sin GPU CUDA) corre en CPU con int8 y modelo
~750MB. Latencia esperada en audio de 5s: ~1-2s end-to-end. Aceptable
para voice agent latency budget.
"""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np
import structlog

log = structlog.get_logger(__name__)


@dataclass
class TranscriptSegment:
    """Un segmento de transcripción con timestamps."""

    text: str
    start_seconds: float
    end_seconds: float
    avg_log_prob: float  # confianza


@dataclass
class TranscriptResult:
    """Resultado completo de una transcripción de utterance."""

    text: str  # texto completo concatenado
    language: str
    segments: list[TranscriptSegment] = field(default_factory=list)
    duration_seconds: float = 0.0


class Transcriber:
    """Faster-Whisper wrapper con buenos defaults para jarvis-os.

    El modelo se carga una vez al `start()` y se reutiliza. Cada
    `transcribe()` es síncrono internamente (CTranslate2) pero lo
    envolvemos con `asyncio.to_thread` desde el orquestador para no
    bloquear el event loop.
    """

    def __init__(
        self,
        model_size: str = "distil-large-v3",
        compute_type: str = "int8",
        device: str = "cpu",
        language: str = "es",
    ) -> None:
        self.model_size = model_size
        self.compute_type = compute_type
        self.device = device
        self.language = language
        self._model = None

    async def start(self) -> None:
        """Carga el modelo. Tarda 5-15s la primera vez (download)."""
        from faster_whisper import WhisperModel  # type: ignore[import-not-found]

        log.info(
            "stt.loading",
            model=self.model_size,
            compute_type=self.compute_type,
            device=self.device,
        )
        self._model = WhisperModel(
            self.model_size,
            device=self.device,
            compute_type=self.compute_type,
        )
        log.info("stt.ready")

    def transcribe(self, audio: np.ndarray, sample_rate: int = 16000) -> TranscriptResult:
        """Transcribe un buffer de audio completo (utterance).

        Sincrónico — debe llamarse vía `asyncio.to_thread` desde async code.

        Args:
            audio: float32 [-1, 1] o int16; lo convertimos internamente.
            sample_rate: usualmente 16000.
        """
        if self._model is None:
            raise RuntimeError("call start() before transcribe()")

        # Scaffold F1.3 — implementación real en F1.3.b.
        # Pseudo-código futuro:
        #   if audio.dtype == np.int16:
        #       audio = audio.astype(np.float32) / 32768.0
        #   segments_iter, info = self._model.transcribe(
        #       audio,
        #       language=self.language,
        #       beam_size=5,
        #       vad_filter=False,  # ya pasamos por silero
        #   )
        #   segments = [TranscriptSegment(...) for s in segments_iter]
        #   text = " ".join(s.text for s in segments).strip()
        #   return TranscriptResult(text=text, language=info.language, ...)
        _ = audio
        _ = sample_rate
        return TranscriptResult(text="", language=self.language)

    async def stop(self) -> None:
        self._model = None
        log.info("stt.stopped")
