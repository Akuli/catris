from __future__ import annotations

import re
import time
from abc import abstractmethod
from typing import TYPE_CHECKING, ClassVar

from catris.ansi import (
    BACKSPACE,
    CLEAR_SCREEN,
    COLOR,
    CSI,
    DOWN_ARROW_KEY,
    LEFT_ARROW_KEY,
    RIGHT_ARROW_KEY,
    UP_ARROW_KEY,
)
from catris.games import GAME_CLASSES, Game, RingGame
from catris.player import Player
from catris.squares import Square

if TYPE_CHECKING:
    from catris.high_scores import HighScore
    from catris.server_and_client import Client


ASCII_ART = r"""
                   __     ___    _____   _____   _____   ___
                  / _)   / _ \  |_   _| |  __ \ |_   _| / __)
                 | (_   / /_\ \   | |   |  _  /  _| |_  \__ \
                  \__) /_/   \_\  |_|   |_| \_\ |_____| (___/
                        https://github.com/Akuli/catris
"""


class View:
    # Can return lines or a tuple: (lines, cursor_pos)
    @abstractmethod
    def get_lines_to_render(self) -> list[bytes] | tuple[list[bytes], tuple[int, int]]:
        pass

    # Return True to quit the game
    @abstractmethod
    def handle_key_press(self, received: bytes) -> bool | None:
        pass


class TextEntryView(View):
    PROMPT: ClassVar[str]

    def __init__(self, client: Client) -> None:
        self._client = client
        self._text = b""
        self._backslash_r_received = False
        self.error: str | None = None

    def get_lines_to_render(self) -> tuple[list[bytes], tuple[int, int]]:
        result = ASCII_ART.encode("ascii").splitlines()
        while len(result) < 10:
            result.append(b"")

        prompt_line = " " * 20 + self.PROMPT + self.get_text()
        result.append(prompt_line.encode("utf-8"))

        if self.error is not None:
            result.append(b"")
            result.append(b"")
            result.append(
                (COLOR % 31) + b"  " + self.error.encode("utf-8") + (COLOR % 0)
            )

        return (result, (11, len(prompt_line) + 1))

    def get_text(self) -> str:
        return "".join(
            c for c in self._text.decode("utf-8", errors="replace") if c.isprintable()
        )

    @abstractmethod
    def on_enter_pressed(self) -> None:
        pass

    def handle_key_press(self, received: bytes) -> None:
        # Enter presses can get sent in different ways...
        # Linux/MacOS raw mode: b"\r"
        # Linux/MacOS cooked mode (not supported): b"YourName\n"
        # Windows: b"\r\n" (handled as if it was \r and \n separately)
        if received == b"\r":
            self.on_enter_pressed()
            self._backslash_r_received = True
        elif received == b"\n":
            if not self._backslash_r_received:
                self.error = "Your terminal doesn't seem to be in raw mode. Run 'stty raw' and try again."
        elif received in BACKSPACE:
            # Don't just delete last byte, so that non-ascii can be erased
            # with a single backspace press
            self._text = self.get_text()[:-1].encode("utf-8")
        elif received.startswith(CSI):
            # arrow keys or similar
            pass
        elif len(self._text) < 15:  # enough for names and lobby IDs
            self._text += received


class AskNameView(TextEntryView):
    PROMPT = "Name: "

    def on_enter_pressed(self) -> None:
        name = self.get_text().strip()
        if not name:
            self.error = "Please write a name before pressing Enter."
            return
        if any(c.isspace() and c != " " for c in name):
            self.error = (
                "The name can contain spaces, but not other whitespace characters."
            )
            return

        # Prevent two simultaneous clients with the same name.
        # But it's fine if you leave and then join back with the same name.
        if name.lower() in (
            client.name.lower()
            for client in self._client.server.all_clients
            if client.name is not None
        ):
            self.error = "This name is in use. Try a different name."
            return

        if (
            self._client.server.only_lobby is not None
            and self._client.server.only_lobby.is_full
        ):
            self.error = "The server is full. Please try again later."
            return

        print(f"name asking done: {name!r}")
        self._client.name = name
        if self._client.server.only_lobby is None:
            # multiple lobbies mode
            self._client.view = ChooseIfNewLobbyView(self._client)
        else:
            self._client.server.only_lobby.add_client(self._client)
            self._client.view = ChooseGameView(self._client)


class MenuView(View):
    def __init__(self) -> None:
        self.menu_items: list[str] = []
        self.selected_index = 0

    def get_lines_to_render(self) -> list[bytes]:
        item_width = 35
        result = [b"", b""]
        for index, item in enumerate(self.menu_items):
            display_text = item.center(item_width).encode("utf-8")
            if index == self.selected_index:
                display_text = (COLOR % 47) + display_text  # white background
                display_text = (COLOR % 30) + display_text  # black foreground
                display_text += COLOR % 0
            result.append(b" " * ((80 - item_width) // 2) + display_text)
        return result

    @abstractmethod
    def on_enter_pressed(self) -> bool | None:
        pass

    def handle_key_press(self, received: bytes) -> bool:
        if received == UP_ARROW_KEY:
            if self.selected_index > 0:
                self.selected_index -= 1
        elif received == DOWN_ARROW_KEY:
            if self.selected_index + 1 < len(self.menu_items):
                self.selected_index += 1
        elif received == b"\r":
            return bool(self.on_enter_pressed())
        else:
            # Select menu item whose text starts with pressed key.
            # Aka press r for ring mode
            try:
                received_text = received.decode("utf-8")
            except UnicodeDecodeError:
                pass
            else:
                for index, text in enumerate(self.menu_items):
                    if text.lower().startswith(received_text.lower()):
                        self.selected_index = index
                        break
        return False  # do not quit yet


class ChooseIfNewLobbyView(MenuView):
    def __init__(self, client: Client):
        super().__init__()
        self._client = client

        self.menu_items.append("New lobby")
        self.menu_items.append("Join an existing lobby")
        self.menu_items.append("Quit")

    def get_lines_to_render(self) -> list[bytes]:
        # TODO: display some server stats? number of connected users, etc
        return ASCII_ART.encode("ascii").split(b"\n") + super().get_lines_to_render()

    def on_enter_pressed(self) -> bool:
        text = self.menu_items[self.selected_index]
        if text == "Quit":
            return True
        elif text == "New lobby":
            # TODO: max number of lobbies?
            from catris.lobby import Lobby, generate_lobby_id

            lobby_id = generate_lobby_id(self._client.server.lobbies.keys())
            print("Creating new lobby with ID", lobby_id)
            lobby = Lobby(lobby_id)
            self._client.server.lobbies[lobby_id] = lobby
            lobby.add_client(self._client)
            self._client.view = ChooseGameView(self._client)
        elif text == "Join an existing lobby":
            self._client.view = AskLobbyIDView(self._client)
        else:
            raise NotImplementedError(text)
        return False


class AskLobbyIDView(TextEntryView):
    PROMPT = "Lobby ID (6 characters): "

    def __init__(self, client: Client) -> None:
        super().__init__(client)
        self._last_attempt_time = 0.0

    def on_enter_pressed(self) -> None:
        from catris.lobby import MAX_CLIENTS_PER_LOBBY

        # Prevent brute-forcing the IDs
        # TODO: max number of simultaneous connections for each client IP address
        if time.monotonic() < self._last_attempt_time + 1:
            return

        lobby_id = self.get_text().strip().upper()
        print(self._client.name, "attempts to join lobby:", lobby_id)

        if not re.fullmatch(r"[A-Z0-9]{6}", lobby_id):
            self.error = "The text you entered doesn't look like a lobby ID."
            return

        self._last_attempt_time = time.monotonic()

        lobby = self._client.server.lobbies.get(lobby_id)
        if lobby is None:
            print(self._client.name, "tried to join non-existent lobby:", lobby_id)
            self.error = f"There is no lobby with ID '{lobby_id}'."
            return
        if lobby.is_full:
            print(self._client.name, "tried to join a full lobby:", lobby_id)
            self.error = f"Lobby '{lobby_id}' is full. It already has {MAX_CLIENTS_PER_LOBBY} players."
            return

        lobby.add_client(self._client)
        self._client.view = ChooseGameView(self._client)


class ChooseGameView(MenuView):
    def __init__(
        self, client: Client, previous_game_class: type[Game] = GAME_CLASSES[0]
    ):
        super().__init__()
        self._client = client
        self.selected_index = GAME_CLASSES.index(previous_game_class)
        self._fill_menu()

    def _should_show_cannot_join_error(self) -> bool:
        assert self._client.name is not None
        assert self._client.lobby is not None

        if self.selected_index >= len(GAME_CLASSES):
            return False
        game = self._client.lobby.games.get(GAME_CLASSES[self.selected_index])
        return game is not None and not game.player_can_join(self._client.name)

    def _fill_menu(self) -> None:
        assert self._client.lobby is not None
        self.menu_items.clear()
        for game_class in GAME_CLASSES:
            game = self._client.lobby.games.get(game_class, None)
            n = 0 if game is None else len(game.players)
            self.menu_items.append(
                f"{game_class.NAME} ({n}/{game_class.MAX_PLAYERS} players)"
            )

        self.menu_items.append("Quit")

    def get_lines_to_render(self) -> list[bytes]:
        from catris.lobby import MAX_CLIENTS_PER_LOBBY

        self._fill_menu()
        assert self._client.lobby is not None
        result = [b"", b""]
        if self._client.server.only_lobby is None:
            # Multiple lobbies mode
            result.append(f"   Lobby ID: {self._client.lobby.lobby_id}".encode("ascii"))
            result.append(b"")
            result.append(b"   Players in lobby:")
        else:
            result.append(b"   Players:")

        for number, client in enumerate(self._client.lobby.clients, start=1):
            assert client.name is not None
            assert client.color is not None
            text = (
                f"      {number}. ".encode("ascii")
                + (COLOR % client.color)
                + client.name.encode("utf-8")
                + (COLOR % 0)
            )
            if client == self._client:
                text += b" (you)"
            result.append(text)
        result.extend([b""] * (MAX_CLIENTS_PER_LOBBY - len(self._client.lobby.clients)))
        result.extend(super().get_lines_to_render())
        if self._should_show_cannot_join_error():
            result.append(b"")
            result.append(b"")
            result.append(
                (COLOR % 31) + b"This game is full.".center(80).rstrip() + (COLOR % 0)
            )
        return result

    def on_enter_pressed(self) -> bool:
        if self.menu_items[self.selected_index] == "Quit":
            return True

        if not self._should_show_cannot_join_error():
            self._client.view = CheckTerminalSizeView(
                self._client, GAME_CLASSES[self.selected_index]
            )
        return False


class CheckTerminalSizeView(View):
    def __init__(self, client: Client, game_class: type[Game]):
        self._client = client
        self._game_class = game_class

    def get_lines_to_render(self) -> list[bytes]:
        width = 80
        height = self._game_class.TERMINAL_HEIGHT_NEEDED

        text_lines = [
            b"Please adjust your terminal size so that you can",
            b"see the entire rectangle. Press Enter when done.",
        ]

        lines = [b"|" + b" " * (width - 2) + b"|"] * height
        lines[0] = lines[-1] = b"o" + b"-" * (width - 2) + b"o"
        for index, line in enumerate(text_lines):
            lines[2 + index] = b"|" + line.center(width - 2) + b"|"
            lines[-2 - len(text_lines) + index] = b"|" + line.center(width - 2) + b"|"

        return lines

    def handle_key_press(self, received: bytes) -> None:
        if received == b"\r":
            # rendering this view is a bit special :)
            #
            # Make sure screen clears before changing view, even if the next
            # view isn't actually as tall as this view. This can happen if a
            # game was full and you're thrown back to main menu.
            self._client._send_bytes(CLEAR_SCREEN)
            assert self._client.lobby is not None
            self._client.lobby.start_game(self._client, self._game_class)


class GameOverView(MenuView):
    def __init__(self, client: Client, game: Game, new_high_score: HighScore):
        super().__init__()
        self.menu_items.extend(["New Game", "Choose a different game", "Quit"])
        self._client = client
        self.game = game
        self.new_high_score = new_high_score
        self._high_scores: list[HighScore] | None = None

    def set_high_scores(self, high_scores: list[HighScore]) -> None:
        self._high_scores = high_scores

    def get_lines_to_render(self) -> list[bytes]:
        if self._high_scores is None:
            return [b"", b"", b"Loading...".center(80).rstrip()]

        lines = [b"", b"", b""]
        lines.append(b"Game Over :(".center(80).rstrip())
        lines.append(
            f"Your score was {self.new_high_score.score}.".encode("ascii")
            .center(80)
            .rstrip()
        )

        lines.extend(super().get_lines_to_render())

        lines.append(b"")
        lines.append(b"")
        lines.append(b"=== HIGH SCORES ".ljust(80, b"="))
        lines.append(b"")
        lines.append(b"| Score | Duration | Players")
        lines.append(b"|-------|----------|-------".ljust(80, b"-"))

        for hs in self._high_scores:
            player_string = ", ".join(hs.players)
            line_string = (
                f"| {hs.score:<6}| {hs.get_duration_string():<9}| {player_string}"
            )
            line = line_string.encode("utf-8")
            if hs == self.new_high_score:
                lines.append((COLOR % 42) + line)
            else:
                lines.append((COLOR % 0) + line)

        lines.append(COLOR % 0)  # Needed if last score was highlighted
        return lines

    def on_enter_pressed(self) -> bool:
        if self._high_scores is None:
            return False

        text = self.menu_items[self.selected_index]
        if text == "New Game":
            assert self._client.lobby is not None
            self._client.lobby.start_game(self._client, type(self.game))
        elif text == "Choose a different game":
            self._client.view = ChooseGameView(self._client, type(self.game))
        elif text == "Quit":
            return True
        else:
            raise NotImplementedError(text)

        return False


def get_block_preview(squares: set[Square]) -> list[bytes]:
    min_x = min(square.x for square in squares)
    min_y = min(square.y for square in squares)
    width = max(square.x - min_x for square in squares) + 1
    height = max(square.y - min_y for square in squares) + 1

    result = [[b"  "] * width for y in range(height)]
    for square in squares:
        result[square.y - min_y][square.x - min_x] = square.get_text(landed=False)
    return [b"".join(row) for row in result]


class PlayingView(View):
    def __init__(self, client: Client, game: Game, player: Player):
        self._client = client
        self._server = client.server
        # no idea why these need explicit type annotations
        self.game: Game = game
        self.player: Player = player

    def get_lines_to_render(self) -> list[bytes]:
        lines = self.game.get_lines_to_render(self.player)
        assert self._client.lobby is not None
        if self._client.lobby.lobby_id is not None:
            # multiple lobbies mode
            lines[4] += f"  Lobby ID: {self._client.lobby.lobby_id}".encode("ascii")
        lines[5] += (
            (COLOR % 36) + f"  Score: {self.game.score}".encode("ascii") + (COLOR % 0)
        )
        if self._client.rotate_counter_clockwise:
            lines[6] += b"  Counter-clockwise"

        lines[7] += b"  Next:"
        for index, row in enumerate(
            get_block_preview(self.player.next_moving_squares), start=9
        ):
            lines[index] += b"   " + row
        if isinstance(self.player.moving_block_or_wait_counter, int):
            n = self.player.moving_block_or_wait_counter
            lines[16] += f"  Please wait: {n}".encode("ascii")
        return lines

    def handle_key_press(self, received: bytes) -> None:
        if received in (b"A", b"a", LEFT_ARROW_KEY):
            self.game.move_if_possible(self.player, dx=-1, dy=0, in_player_coords=True)
            self.player.set_fast_down(False)
        elif received in (b"D", b"d", RIGHT_ARROW_KEY):
            self.game.move_if_possible(self.player, dx=1, dy=0, in_player_coords=True)
            self.player.set_fast_down(False)
        elif received in (b"W", b"w", UP_ARROW_KEY, b"\r"):
            self.game.rotate(self.player, self._client.rotate_counter_clockwise)
            self.player.set_fast_down(False)
        elif received in (b"S", b"s", DOWN_ARROW_KEY, b" "):
            self.player.set_fast_down(True)
        elif received in (b"R", b"r"):
            self._client.rotate_counter_clockwise = (
                not self._client.rotate_counter_clockwise
            )
            self._client.render()
        elif (
            received in (b"F", b"f")
            and isinstance(self.game, RingGame)
            and len(self.game.players) == 1
        ):
            self.game.players[0].flip_view()
            if self.game.is_valid():
                self.game.need_render_event.set()
            else:
                # Can't flip, blocks are on top of each other. Flip again to undo.
                self.game.players[0].flip_view()
