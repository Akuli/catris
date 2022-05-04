from __future__ import annotations

import asyncio
import collections
import itertools
import logging
import time
from typing import TYPE_CHECKING

from catris.ansi import (
    CLEAR_FROM_CURSOR_TO_END_OF_SCREEN,
    CLEAR_SCREEN,
    CLEAR_TO_END_OF_LINE,
    CONTROL_C,
    CONTROL_D,
    CONTROL_Q,
    CONTROL_R,
    CSI,
    ESC,
    HIDE_CURSOR,
    MOVE_CURSOR,
    SHOW_CURSOR,
)
from catris.lobby import Lobby
from catris.views import AskNameView, CheckTerminalSizeView, TextEntryView, View

try:
    from websockets.server import WebSocketServerProtocol
    from websockets.exceptions import WebSocketException
except ImportError:
    WebSocketServerProtocol = None  # type: ignore
    WebSocketException = None  # type: ignore


class _RawTCPConnection:
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

    @property
    def is_closing(self) -> bool:
        return self._writer.transport.is_closing()


class _WebSocketConnection:
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

    @property
    def is_closing(self) -> bool:
        # Docs say: "Be aware that both open and closed are False during the
        # opening and closing sequences."
        return not self._ws.open


class Server:
    def __init__(self, use_lobbies: bool) -> None:
        self._connection_ips: collections.deque[tuple[float, str]] = collections.deque()

        self.all_clients: set[Client] = set()
        self.lobbies: dict[str, Lobby] = {}  # keys are lobby IDs
        if use_lobbies:
            self.only_lobby = None
        else:
            # Create a single lobby that will be used for everything
            self.only_lobby = Lobby(None)

    async def _handle_any_connection(
        self, client: Client
    ) -> None:
        ip = client._connection.get_ip()
        self._connection_ips.append((time.monotonic(), ip))
        one_min_ago = time.monotonic() - 60
        while self._connection_ips and self._connection_ips[0][0] < one_min_ago:
            self._connection_ips.popleft()

        count = [old_ip for connection_time, old_ip in self._connection_ips].count(ip)
        if count >= 5:
            client.log(
                f"This is the {count}th connection from IP address {ip} within the last minute"
            )

        await client.handle()

    async def handle_raw_tcp_connection(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        client = Client(self, _RawTCPConnection(reader, writer))
        client.log("New raw TCP connection")
        await self._handle_any_connection(client)

    async def handle_websocket_connection(self, ws: WebSocketServerProtocol) -> None:
        client = Client(self, _WebSocketConnection(ws))
        client.log("New websocket connection")
        await self._handle_any_connection(client)


_id_counter = itertools.count(1)


class Client:
    def __init__(
        self, server: Server, connection: _RawTCPConnection | _WebSocketConnection
    ) -> None:
        self._connection = connection
        self._client_id = next(_id_counter)
        self.server = server
        self._current_receive_task: asyncio.Task[bytes] | None = None
        self._recv_stats: collections.deque[tuple[float, int]] = collections.deque()

        self.name: str | None = None
        self.lobby: Lobby | None = None
        self.color: int | None = None
        self.view: View = AskNameView(self)
        self._last_rendered_view: View | None = None
        self._last_displayed_lines: list[bytes] = []

        self.rotate_counter_clockwise = False
        self.lobby_id_hidden = False

        # Web UI doesn't have any way to resize. It's always big enough.
        self.user_can_resize_terminal = not isinstance(connection, _WebSocketConnection)

    def get_lobby_id_for_display(self) -> bytes:
        assert self.lobby is not None
        if self.lobby.lobby_id is None:
            return b""
        if self.lobby_id_hidden:
            return b"Lobby ID: ******"
        return f"Lobby ID: {self.lobby.lobby_id}".encode("ascii")

    def log(self, msg: str, *, level: int = logging.INFO) -> None:
        logging.log(level, f"(client {self._client_id}) {msg}")

    def render(self, *, force_redraw: bool = False) -> None:
        if isinstance(self.view, CheckTerminalSizeView):
            # Very different from other views
            self._last_displayed_lines.clear()
            self._send_bytes(
                CLEAR_SCREEN
                + (MOVE_CURSOR % (1, 1))
                + b"\r\n".join(self.view.get_lines_to_render())
            )
            return

        lines = self.view.get_lines_to_render()
        if isinstance(lines, tuple):
            lines, cursor_pos = lines
        else:
            # Bottom of view. If user types something, it's unlikely to be
            # noticed here before it gets wiped by the next refresh.
            cursor_pos = (len(lines) + 1, 1)

        # Send it all at once, so that hopefully cursor won't be in a
        # temporary place for long times, even if internet is slow
        to_send = b""

        if self._last_rendered_view != self.view or force_redraw:
            self._last_displayed_lines.clear()
            to_send += CLEAR_SCREEN

        while len(lines) < len(self._last_displayed_lines):
            lines.append(b"")
        while len(lines) > len(self._last_displayed_lines):
            self._last_displayed_lines.append(b"")

        if isinstance(self.view, TextEntryView):
            to_send += SHOW_CURSOR
        else:
            to_send += HIDE_CURSOR

        # Hide user's key press at cursor location. Needs to be done at
        # whatever cursor location is currently, before we move it.
        to_send += b"\r"  # move cursor to start of line
        to_send += CLEAR_TO_END_OF_LINE

        for y, (old_line, new_line) in enumerate(
            zip(self._last_displayed_lines, lines), start=1
        ):
            # Re-rendering cursor line helps with AskNameView
            if old_line != new_line or y == cursor_pos[0]:
                to_send += MOVE_CURSOR % (y, 1)
                to_send += new_line
                to_send += CLEAR_TO_END_OF_LINE
        self._last_displayed_lines = lines.copy()
        self._last_rendered_view = self.view

        to_send += MOVE_CURSOR % cursor_pos
        self._send_bytes(to_send)

    def _send_bytes(self, b: bytes) -> None:
        if self._connection.is_closing:
            return

        self._connection.put_to_send_queue(b)

        # Prevent filling the server's memory if client sends but never receives.
        # Usually send buffer is empty (0 bytes) because operating system has buffering too.
        # But it feels weird to rely on operating system's (undocumented?) buffer size.
        #
        # On 80x24 terminal with no colors, we send max 80*24 = 1920 bytes at a time.
        # There's some extra space for colors and bigger terminals.
        if self._connection.get_send_queue_size() > 4 * 1024:
            self.log("More than 4K of data in send buffer, disconnecting")
            self._connection.close()
            # Closing isn't enough to stop receiving immediately.
            # At least not with raw TCP connections
            if self._current_receive_task is not None:
                self._current_receive_task.cancel()

    async def _receive_bytes(self) -> bytes | None:
        # Makes game playable while under very heavy cpu load.
        # Should no longer be necessary, but just in case...
        await asyncio.sleep(0)

        if self._connection.is_closing:
            return None

        assert self._current_receive_task is None
        self._current_receive_task = asyncio.create_task(self._connection.receive_bytes())
        try:
            result = await asyncio.wait_for(self._current_receive_task, timeout=10 * 60)
        except asyncio.TimeoutError:
            self.log("Nothing received in 10min, disconnecting")
            self._send_bytes(
                SHOW_CURSOR
                + b"Closing connection because it has been idle for 10 minutes.\r\n"
            )
            return None
        except asyncio.CancelledError:
            # cancelled in _send_bytes()
            return None
        except OSError as e:
            self.log(f"Receive error: {e}")
            return None
        finally:
            self._current_receive_task = None

        # Prevent 100% cpu usage if someone sends a lot of data
        now = time.monotonic()
        self._recv_stats.append((now, len(result)))
        while self._recv_stats and self._recv_stats[0][0] < now - 1:
            self._recv_stats.popleft()
        # By smashing keys as much as possible I can get to about 60 bytes/sec.
        # I think bad connection might send several seconds of key presses at once.
        if sum(length for timestamp, length in self._recv_stats) > 256:
            self.log("Received more than 256 bytes/sec, disconnecting")
            return None

        # Checking ESC key here is a bad idea.
        # Arrow keys are sent as ESC + other bytes, and recv() can sometimes
        # return only some of the sent data.
        if (
            not result
            or CONTROL_C in result
            or CONTROL_D in result
            or CONTROL_Q in result
        ):
            return None

        return result

    async def handle(self) -> None:
        try:
            self.server.all_clients.add(self)
            self.log(f"There are now {len(self.server.all_clients)} connected clients")
            received = b""
            force_redraw_on_next_render = False

            while True:
                self.render(force_redraw=force_redraw_on_next_render)
                force_redraw_on_next_render = False

                new_chunk = await self._receive_bytes()
                if new_chunk is None:
                    break
                received += new_chunk

                # Arrow key presses are received as 3 bytes. The first two of
                # them are CSI, aka ESC [. If we have received a part of an
                # arrow key press, don't process it yet, wait for the rest to
                # arrive instead.
                key_presses = []
                while received not in (b"", ESC, CSI):
                    n = 3 if received.startswith(CSI) else 1
                    key_presses.append(received[:n])
                    received = received[n:]

                for key_press in key_presses:
                    if key_press == CONTROL_R:
                        self.log("Ctrl+R pressed, forcing redraw on next render")
                        force_redraw_on_next_render = True
                    elif self.view.handle_key_press(key_press):
                        return

        finally:
            self.log("Closing connection")
            self.server.all_clients.discard(self)
            self.log(f"There are now {len(self.server.all_clients)} connected clients")
            if self.lobby is not None:
                lobby = self.lobby
                lobby.remove_client(self)
                # Now self.lobby is now None, but lobby isn't
                if not lobby.clients and lobby.lobby_id is not None:
                    self.log(
                        f"Removing lobby because last user quits: {lobby.lobby_id}"
                    )
                    del self.server.lobbies[lobby.lobby_id]

            # \r moves cursor to start of line
            self._send_bytes(b"\r" + CLEAR_FROM_CURSOR_TO_END_OF_SCREEN + SHOW_CURSOR)

            try:
                await asyncio.wait_for(self._connection.flush(), timeout=3)
            except (OSError, asyncio.TimeoutError):
                pass
            self._connection.close()
