from __future__ import annotations

import asyncio
import collections
import itertools
import logging
import time

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

    async def handle_connection(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        client = Client(self, reader, writer)

        ip = writer.get_extra_info("peername")[0]
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


_id_counter = itertools.count(1)


class Client:
    def __init__(
        self, server: Server, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        self._client_id = next(_id_counter)
        self.server = server
        self._reader = reader
        self.writer = writer
        self._recv_stats: collections.deque[tuple[float, int]] = collections.deque()

        self._last_displayed_lines: list[bytes] = []
        self.name: str | None = None
        self.lobby: Lobby | None = None
        self.color: int | None = None
        self.view: View = AskNameView(self)
        self._last_rendered_view: View | None = None

        self.rotate_counter_clockwise = False
        self.lobby_id_hidden = False

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
        self.writer.write(b)

        # Prevent filling the server's memory if client sends but never receives.
        # I don't use .drain() because one client's slowness shouldn't slow others.
        if self.writer.transport.get_write_buffer_size() > 64 * 1024:  # type: ignore
            self.log("More than 64K of data in send buffer, disconnecting")
            self.writer.transport.close()

    async def _receive_bytes(self) -> bytes | None:
        # Makes game playable while under very heavy cpu load.
        # Should no longer be necessary, but just in case...
        await asyncio.sleep(0)

        if self.writer.transport.is_closing():
            return None

        try:
            result = await asyncio.wait_for(self._reader.read(100), timeout=3 * 60)
        except asyncio.TimeoutError:
            self.log("Nothing received in 3min, disconnecting")
            self._send_bytes(
                SHOW_CURSOR
                + b"Closing connection because it has been idle for 3 minutes.\r\n"
            )
            return None
        except OSError as e:
            self.log(f"Receive error: {e}")
            return None

        # Prevent 100% cpu usage if someone sends a lot of data
        now = time.monotonic()
        self._recv_stats.append((now, len(result)))
        while self._recv_stats and self._recv_stats[0][0] < now - 1:
            self._recv_stats.popleft()
        if sum(length for timestamp, length in self._recv_stats) > 2000:
            self.log("Received more than 2KB/sec, disconnecting")
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
        self.log("New connection")

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
                await asyncio.wait_for(self.writer.drain(), timeout=3)
            except (OSError, asyncio.TimeoutError):
                pass
            self.writer.transport.close()
