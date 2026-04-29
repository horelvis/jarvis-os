"""Speech-to-text vía Faster-Whisper.

Faster-Whisper usa CTranslate2 como backend, optimizado para CPU/GPU
con int8 quantization. En el Asus ZenBook (Whiskey Lake i7-8565U sin
CUDA en hábito normal) corre en CPU con int8.

El modelo por defecto es `base` para latencia baja en español. Si
necesitas más calidad cambia a `small` o `distil-large-v3` (más lento
pero WER más bajo).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

import numpy as np
import structlog

log = structlog.get_logger(__name__)


@dataclass
class TranscriptSegment:
    text: str
    start_seconds: float
    end_seconds: float
    avg_log_prob: float


@dataclass
class TranscriptResult:
    text: str
    language: str
    segments: list[TranscriptSegment] = field(default_factory=list)
    duration_seconds: float = 0.0


class Transcriber:
    """Faster-Whisper wrapper con buenos defaults para jarvis-os.

    El modelo se carga una vez al `start()` y se reutiliza. Cada
    `transcribe()` es síncrono (CTranslate2) — el orquestador lo
    envuelve con `asyncio.to_thread` para no bloquear el event loop.
    """

    def __init__(
        self,
        model_size: str = "base",
        compute_type: str = "int8",
        device: str = "cpu",
        language: str = "es",
        beam_size: int = 1,
    ) -> None:
        self.model_size = model_size
        self.compute_type = compute_type
        self.device = device
        self.language = language
        self.beam_size = beam_size
        self._model: Any = None

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

    def transcribe(
        self, audio: np.ndarray, sample_rate: int = 16000
    ) -> TranscriptResult:
        """Transcribe un buffer completo de audio (utterance).

        Sincrónico. Llamar vía `asyncio.to_thread` desde código async.
        """
        if self._model is None:
            raise RuntimeError("call start() before transcribe()")
        if sample_rate != 16000:
            raise ValueError(f"sample_rate must be 16000, got {sample_rate}")

        if audio.dtype == np.int16:
            audio_f32 = audio.astype(np.float32) / 32768.0
        elif audio.dtype == np.float32:
            audio_f32 = audio
        else:
            audio_f32 = audio.astype(np.float32)

        segments_iter, info = self._model.transcribe(
            audio_f32,
            language=self.language,
            beam_size=self.beam_size,
            vad_filter=False,  # ya pasamos por silero
        )
        segments: list[TranscriptSegment] = []
        for s in segments_iter:
            segments.append(
                TranscriptSegment(
                    text=s.text.strip(),
                    start_seconds=float(s.start),
                    end_seconds=float(s.end),
                    avg_log_prob=float(s.avg_logprob),
                )
            )
        text = " ".join(seg.text for seg in segments).strip()
        return TranscriptResult(
            text=text,
            language=info.language,
            segments=segments,
            duration_seconds=float(info.duration),
        )

    async def stop(self) -> None:
        self._model = None
        log.info("stt.stopped")
