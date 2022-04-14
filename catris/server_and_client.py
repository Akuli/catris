from __future__ import annotations

import asyncio
import collections
import time

from catris.ansi import (
    CLEAR_FROM_CURSOR_TO_END_OF_SCREEN,
    CLEAR_SCREEN,
    CLEAR_TO_END_OF_LINE,
    CONTROL_C,
    CONTROL_D,
    CONTROL_Q,
    CSI,
    ESC,
    MOVE_CURSOR,
    SHOW_CURSOR,
)
from catris.games import GAME_CLASSES, Game
from catris.high_scores import save_and_display_high_scores
from catris.player import MovingBlock, Player
from catris.views import (
    AskNameView,
    CheckTerminalSizeView,
    ChooseGameView,
    PlayingView,
    View,
)


class Server:
    def __init__(self) -> None:
        self.clients: set[Client] = set()
        self.games: set[Game] = set()

    def start_game(self, client: Client, game_class: type[Game]) -> None:
        assert client in self.clients

        existing_games = [game for game in self.games if isinstance(game, game_class)]
        if existing_games:
            [game] = existing_games
        else:
            game = game_class()
            game.player_has_a_connected_client = self._player_has_a_connected_client
            game.tasks.append(asyncio.create_task(self._render_task(game)))
            self.games.add(game)

        assert client.name is not None
        player = game.get_existing_player_or_add_new_player(client.name)
        if player is None:
            client.view = ChooseGameView(client, game_class)
        else:
            client.view = PlayingView(client, game, player)

        # ChooseGameViews display how many players are currently playing each game
        for client in self.clients:
            if isinstance(client.view, ChooseGameView):
                client.render()

    def _player_has_a_connected_client(self, player: Player) -> bool:
        return any(
            isinstance(client.view, PlayingView) and client.view.player == player
            for client in self.clients
        )

    async def _render_task(self, game: Game) -> None:
        while True:
            await game.need_render_event.wait()
            game.need_render_event.clear()
            self.render_game(game)

    def render_game(self, game: Game) -> None:
        assert game in self.games
        assert game.is_valid()

        game.tasks = [t for t in game.tasks if not t.done()]

        if game.game_is_over():
            self.games.remove(game)
            for task in game.tasks:
                task.cancel()
            asyncio.create_task(save_and_display_high_scores(game, self.clients))
        else:
            for client in self.clients:
                if isinstance(client.view, PlayingView) and client.view.game == game:
                    client.render()

        # ChooseGameViews display how many players are currently playing each game
        for client in self.clients:
            if isinstance(client.view, ChooseGameView):
                client.render()

    async def handle_connection(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        client = Client(self, reader, writer)
        await client.handle()


class Client:
    def __init__(
        self, server: Server, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        self.server = server
        self._reader = reader
        self.writer = writer
        self._recv_stats: collections.deque[tuple[float, int]] = collections.deque()

        self.last_displayed_lines: list[bytes] = []
        self.name: str | None = None
        self.view: View = AskNameView(self)
        self.rotate_counter_clockwise = False

    def render(self) -> None:
        if isinstance(self.view, CheckTerminalSizeView):
            # Very different from other views
            self.last_displayed_lines.clear()
            self._send_bytes(
                CLEAR_SCREEN
                + (MOVE_CURSOR % (1, 1))
                + b"\r\n".join(self.view.get_lines_to_render())
            )
            return

        if isinstance(self.view, AskNameView):
            lines, cursor_pos = self.view.get_lines_to_render_and_cursor_pos()
        else:
            # Bottom of view. If user types something, it's unlikely to be
            # noticed here before it gets wiped by the next refresh.
            lines = self.view.get_lines_to_render()
            cursor_pos = (len(lines) + 1, 1)

        while len(lines) < len(self.last_displayed_lines):
            lines.append(b"")
        while len(lines) > len(self.last_displayed_lines):
            self.last_displayed_lines.append(b"")

        # Send it all at once, so that hopefully cursor won't be in a
        # temporary place for long times, even if internet is slow
        to_send = b""

        # Hide user's key press at cursor location. Needs to be done at
        # whatever cursor location is currently, before we move it.
        to_send += b"\r"  # move cursor to start of line
        to_send += CLEAR_TO_END_OF_LINE

        for y, (old_line, new_line) in enumerate(
            zip(self.last_displayed_lines, lines), start=1
        ):
            # Re-rendering cursor line helps with AskNameView
            if old_line != new_line or y == cursor_pos[0]:
                to_send += MOVE_CURSOR % (y, 1)
                to_send += new_line
                to_send += CLEAR_TO_END_OF_LINE
        self.last_displayed_lines = lines.copy()

        to_send += MOVE_CURSOR % cursor_pos
        self._send_bytes(to_send)

    def _send_bytes(self, b: bytes) -> None:
        self.writer.write(b)

        # Prevent filling the server's memory if client sends but never receives.
        # I don't use .drain() because one client's slowness shouldn't slow others.
        if self.writer.transport.get_write_buffer_size() > 64 * 1024:  # type: ignore
            print("More than 64K of data in send buffer, disconnecting:", self.name)
            self.writer.transport.close()

    async def _receive_bytes(self) -> bytes | None:
        await asyncio.sleep(0)  # Makes game playable while fuzzer is running

        if self.writer.transport.is_closing():
            return None

        try:
            result = await self._reader.read(100)
        except OSError as e:
            print("Receive error:", self.name, e)
            return None

        # Prevent 100% cpu usage if someone sends a lot of data
        now = time.monotonic()
        self._recv_stats.append((now, len(result)))
        while self._recv_stats and self._recv_stats[0][0] < now - 1:
            self._recv_stats.popleft()
        if sum(length for timestamp, length in self._recv_stats) > 2000:
            print("Received more than 2KB/sec, disconnecting:", self.name)
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
        print("New connection")

        try:
            if len(self.server.clients) >= sum(
                klass.MAX_PLAYERS for klass in GAME_CLASSES
            ):
                print("Sending server full message")
                self._send_bytes(b"The server is full. Please try again later.\r\n")
                return

            self.server.clients.add(self)
            self._send_bytes(CLEAR_SCREEN)
            received = b""

            while True:
                self.render()

                new_chunk = await self._receive_bytes()
                if new_chunk is None:
                    break
                received += new_chunk

                # Arrow key presses are received as 3 bytes. The first two of
                # them are CSI, aka ESC [. If we have received a part of an
                # arrow key press, don't process it yet, wait for the rest to
                # arrive instead.
                while received not in (b"", ESC, CSI):
                    if received.startswith(CSI):
                        handle_result = self.view.handle_key_press(received[:3])
                        received = received[3:]
                    else:
                        handle_result = self.view.handle_key_press(received[:1])
                        received = received[1:]
                    if handle_result:
                        return

        finally:
            print("Closing connection:", self.name)
            self.server.clients.discard(self)
            if isinstance(self.view, PlayingView) and isinstance(
                self.view.player.moving_block_or_wait_counter, MovingBlock
            ):
                self.view.player.moving_block_or_wait_counter = None
                self.view.game.need_render_event.set()

            # \r moves cursor to start of line
            self._send_bytes(b"\r" + CLEAR_FROM_CURSOR_TO_END_OF_SCREEN + SHOW_CURSOR)

            try:
                await asyncio.wait_for(self.writer.drain(), timeout=3)
            except (OSError, asyncio.TimeoutError):
                pass
            self.writer.transport.close()
