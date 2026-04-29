"""Text-to-speech vía ElevenLabs Flash.

Flash es el modelo de baja latencia de ElevenLabs (~75ms a primer chunk
de audio). Comparado con Multilingual v2, sacrifica algo de naturalidad
por velocidad — ideal para un asistente en tiempo real.

El stream de audio se reenvía simultáneamente:
- Al output device (sounddevice playback) → el usuario lo oye.
- A los suscriptores del socket (HUD ring) → frames de amplitud para
  animar el anillo central reactivo a la voz de Jarvis (NO al mic del
  usuario, ver feedback_hud_voice_ring.md).
"""

from __future__ import annotations

import os
from collections.abc import AsyncIterator
from dataclasses import dataclass

import numpy as np
import structlog

log = structlog.get_logger(__name__)


@dataclass
class TtsAudioFrame:
    """Frame de audio TTS para playback + análisis de amplitud."""

    pcm: np.ndarray  # int16, mono, sample rate del modelo (16kHz Flash)
    amplitude_rms: float  # RMS normalizado [0..1] para alimentar el anillo
    is_final: bool  # último frame de la utterance


class Speaker:
    """ElevenLabs Flash TTS streaming wrapper.

    Uso:
        speaker = Speaker(voice_id="...")
        async for frame in speaker.synthesize("Hola, ¿en qué te ayudo?"):
            playback(frame.pcm)
            broadcast_amplitude(frame.amplitude_rms)
    """

    def __init__(
        self,
        voice_id: str | None = None,
        model_id: str = "eleven_flash_v2_5",
        api_key: str | None = None,
    ) -> None:
        self.voice_id = voice_id or os.environ.get("ELEVENLABS_VOICE_ID")
        self.model_id = model_id or os.environ.get("ELEVENLABS_MODEL_ID", "eleven_flash_v2_5")
        self.api_key = api_key or os.environ.get("ELEVENLABS_API_KEY")
        self._client = None

        if not self.api_key:
            log.warning("tts.no_api_key", note="ELEVENLABS_API_KEY no encontrada")
        if not self.voice_id:
            log.warning("tts.no_voice_id", note="ELEVENLABS_VOICE_ID no encontrada")

    async def start(self) -> None:
        """Crea el cliente HTTP. No descarga modelo (cloud)."""
        from elevenlabs.client import ElevenLabs  # type: ignore[import-not-found]

        log.info("tts.client_init", model=self.model_id, voice=self.voice_id)
        self._client = ElevenLabs(api_key=self.api_key)

    async def synthesize(self, text: str) -> AsyncIterator[TtsAudioFrame]:
        """Sintetiza `text` y yield-ea frames de audio + amplitud.

        Scaffold F1.3 — implementación real en F1.3.c. Pseudo-código:

            stream = self._client.text_to_speech.stream(
                voice_id=self.voice_id,
                model_id=self.model_id,
                text=text,
                output_format="pcm_16000",
            )
            for chunk_bytes in stream:
                pcm = np.frombuffer(chunk_bytes, dtype=np.int16)
                rms = float(np.sqrt(np.mean(pcm.astype(np.float32) ** 2)) / 32768.0)
                yield TtsAudioFrame(pcm=pcm, amplitude_rms=rms, is_final=False)
            yield TtsAudioFrame(pcm=np.zeros(0, dtype=np.int16), amplitude_rms=0.0, is_final=True)
        """
        if self._client is None:
            raise RuntimeError("call start() before synthesize()")

        _ = text
        if False:  # pragma: no cover
            yield TtsAudioFrame(
                pcm=np.zeros(0, dtype=np.int16), amplitude_rms=0.0, is_final=True
            )

    async def stop(self) -> None:
        self._client = None
        log.info("tts.stopped")
