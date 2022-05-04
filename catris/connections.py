from __future__ import annotations

import asyncio
import logging

try:
    from websockets.server import WebSocketServerProtocol
    from websockets.exceptions import WebSocketException
except ImportError:
    WebSocketServerProtocol = None  # type: ignore
    WebSocketException = None  # type: ignore


class RawTCPConnection:
    def __init__(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        self._reader = reader
        self._writer = writer

    def get_ip(self) -> str:
        return self._writer.get_extra_info("peername")[0]

    def get_send_queue_size(self) -> int:
        # https://github.com/python/typeshed/issues/5779
        return self._writer.transport.get_write_buffer_size()  # type: ignore

    def put_to_send_queue(self, data: bytes) -> None:
        self._writer.write(data)

    async def receive_bytes(self) -> bytes:
        return await self._reader.read(100)

    async def flush(self) -> None:
        await self._writer.drain()

    def close(self) -> None:
        self._writer.transport.close()

    def is_closing(self) -> bool:
        return self._writer.transport.is_closing()


class WebSocketConnection:
    def __init__(self, ws: WebSocketServerProtocol) -> None:
        self._ws = ws
        self._send_queue = bytearray()
        self._send_task: asyncio.Task[None] | None = None

    def get_ip(self) -> str:
        return self._ws.transport.get_extra_info("peername")[0]

    def get_send_queue_size(self) -> int:
        return len(self._send_queue)

    def put_to_send_queue(self, data: bytes) -> None:
        self._send_queue.extend(data)
        if self._send_task is None or self._send_task.done():
            self._send_task = asyncio.create_task(self._send_from_queue())

    async def _send_from_queue(self) -> None:
        while self._send_queue:
            data_to_send = bytes(self._send_queue)
            self._send_queue.clear()

            try:
                await self._ws.send(data_to_send)
            except WebSocketException as e:
                # Ideally we would know what client this connection belongs to.
                # But for raw TCP connections, asyncio's internals log a message
                # without any extra info anyway, so that wouldn't help much.
                self.close()
                logging.warning(f"sending to websocket failed: {e}")

    async def receive_bytes(self) -> bytes:
        try:
            result = await self._ws.recv()
        except WebSocketException as e:
            raise OSError(str(e)) from e

        if isinstance(result, str):
            raise OSError("client sent text, expected bytes")
        return result

    async def flush(self):
        if self._send_task is not None:
            try:
                await self._send_task
            except WebSocketException as e:
                raise OSError(str(e)) from e

    # Docs say: "For legacy reasons, close() completes in at most
    # 5 * close_timeout seconds for clients and 4 * close_timeout
    # for servers."
    #
    # A small close_timeout is set when creating the connection, so
    # this shouldn't create many simultaneously running tasks.
    def close(self) -> None:
        asyncio.create_task(self._ws.close())

    def is_closing(self) -> bool:
        # Docs say: "Be aware that both open and closed are False during the
        # opening and closing sequences."
        return not self._ws.open
