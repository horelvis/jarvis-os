"""jarvis-os voice daemon.

Pipeline:
    audio capture (sounddevice)
        → VAD (Silero) gate
            → Wake word (openWakeWord)
                → STT (Faster-Whisper distil-large-v3)
                    → IronClaw agent (vía socket)
                        → TTS (ElevenLabs Flash)
                            → audio playback + amplitude stream → HUD

El daemon expone un servidor de socket WebSocket local al que se suscriben:
- IronClaw: recibe transcripts del usuario, envía respuestas para TTS.
- HUD widgets (EWW + Tauri ring): reciben eventos de estado del agente
  (idle/listening/thinking/speaking) y, durante speaking, los frames de
  amplitud TTS para animar el anillo central.
"""

__version__ = "0.1.0"
