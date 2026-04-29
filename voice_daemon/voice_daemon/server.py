"""Servidor WebSocket local del voice_daemon.

Expone un endpoint en `ws://127.0.0.1:7331` (puerto canónico de jarvis-os).
Mensajes JSON line-delimited en ambas direcciones.

## Eventos del daemon hacia los clientes

- `wake_detected` — palabra de activación detectada.
    `{ "type": "wake_detected", "model": "hey_jarvis", "score": 0.78, "ts": 1234567890.123 }`

- `transcript_partial` / `transcript_final` — texto del usuario.
    `{ "type": "transcript_final", "text": "qué hora es", "language": "es" }`

- `agent_state` — estado del FSM del agente (alimenta el anillo del HUD).
    `{ "type": "agent_state", "state": "idle"|"listening"|"thinking"|"speaking" }`

- `tts_amplitude` — frame de amplitud durante speaking (drives the ring).
    `{ "type": "tts_amplitude", "rms": 0.42, "is_final": false }`

## Mensajes desde los clientes hacia el daemon

- IronClaw envía respuestas para TTS:
    `{ "type": "speak", "text": "Son las 18:42." }`

- HUD envía resoluciones de CONFIRM (panel inline del usuario):
    `{ "type": "confirm_response", "request_id": "...", "approved": true }`

El protocolo se versiona vía un primer mensaje `hello` con `version`. Si
en el futuro hay clientes desactualizados, el daemon puede degradar.
"""

from __future__ import annotations

import asyncio
import json
from collections.abc import AsyncIterator
from dataclasses import dataclass, field
from typing import Any

import structlog

log = structlog.get_logger(__name__)


@dataclass
class ServerConfig:
    host: str = "127.0.0.1"
    port: int = 7331
    protocol_version: int = 1


@dataclass
class _Hub:
    """Bus pub/sub interno: el daemon emite eventos, los clientes suscritos
    los reciben. Sólo hay un hub por daemon."""

    subscribers: set[asyncio.Queue[dict[str, Any]]] = field(default_factory=set)

    async def publish(self, event: dict[str, Any]) -> None:
        for q in list(self.subscribers):
            try:
                q.put_nowait(event)
            except asyncio.QueueFull:
                log.warning("server.queue_full")
                self.subscribers.discard(q)


class Server:
    """WebSocket server con un hub pub/sub interno."""

    def __init__(self, config: ServerConfig | None = None) -> None:
        self.config = config or ServerConfig()
        self._hub = _Hub()
        self._task: asyncio.Task[None] | None = None

    async def start(self) -> None:
        """Arranca el listener WebSocket en background."""
        # Lazy import: websockets pesa al importar.
        import websockets  # type: ignore[import-not-found]

        async def handler(ws: Any) -> None:
            await self._handle_client(ws)

        srv = await websockets.serve(handler, self.config.host, self.config.port)
        log.info(
            "server.listening",
            host=self.config.host,
            port=self.config.port,
            version=self.config.protocol_version,
        )
        # Mantener el server vivo en una task; el caller lo cancela en stop().
        self._task = asyncio.create_task(srv.wait_closed())

    async def stop(self) -> None:
        if self._task is not None:
            self._task.cancel()
            self._task = None
        log.info("server.stopped")

    async def emit(self, event: dict[str, Any]) -> None:
        """Publica un evento a todos los suscriptores."""
        await self._hub.publish(event)

    async def _handle_client(self, ws: Any) -> None:
        """Lifecycle de una conexión cliente: handshake → suscribir → loop."""
        queue: asyncio.Queue[dict[str, Any]] = asyncio.Queue(maxsize=256)
        self._hub.subscribers.add(queue)
        try:
            await ws.send(
                json.dumps(
                    {"type": "hello", "version": self.config.protocol_version}
                )
            )

            send_task = asyncio.create_task(self._pump_outbound(ws, queue))
            recv_task = asyncio.create_task(self._pump_inbound(ws))

            done, pending = await asyncio.wait(
                {send_task, recv_task}, return_when=asyncio.FIRST_COMPLETED
            )
            for t in pending:
                t.cancel()
        finally:
            self._hub.subscribers.discard(queue)

    @staticmethod
    async def _pump_outbound(ws: Any, queue: asyncio.Queue[dict[str, Any]]) -> None:
        while True:
            event = await queue.get()
            await ws.send(json.dumps(event))

    @staticmethod
    async def _pump_inbound(ws: Any) -> AsyncIterator[dict[str, Any]]:
        async for raw in ws:
            try:
                msg = json.loads(raw)
            except json.JSONDecodeError as e:
                log.warning("server.bad_json", error=str(e))
                continue
            log.info("server.client_msg", type=msg.get("type"))
            yield msg
