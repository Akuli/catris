from __future__ import annotations

from abc import abstractmethod
from typing import TYPE_CHECKING, Union

from catris.ansi import (
    BACKSPACE,
    CLEAR_SCREEN,
    COLOR,
    DOWN_ARROW_KEY,
    HIDE_CURSOR,
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

# Longest allowed name will get truncated in a few places, that's fine
NAME_MAX_LENGTH = 15


class AskNameView:
    def __init__(self, client: Client):
        assert client.name is None
        self._client = client
        self._name_so_far = b""
        self._error: str | None = None
        self._backslash_r_received = False

    def _get_name(self) -> str:
        return "".join(
            c
            for c in self._name_so_far.decode("utf-8", errors="replace")
            if c.isprintable()
        )

    def get_lines_to_render_and_cursor_pos(self) -> tuple[list[bytes], tuple[int, int]]:
        result = ASCII_ART.encode("ascii").splitlines()
        while len(result) < 10:
            result.append(b"")

        name_line = " " * 20 + f"Name: {self._get_name()}"
        result.append(name_line.encode("utf-8"))

        if self._error is not None:
            result.append(b"")
            result.append(b"")
            result.append(
                (COLOR % 31) + b"  " + self._error.encode("utf-8") + (COLOR % 0)
            )

        return (result, (11, len(name_line) + 1))

    def handle_key_press(self, received: bytes) -> None:
        # Enter presses can get sent in different ways...
        # Linux/MacOS raw mode: b"\r"
        # Linux/MacOS cooked mode (not supported): b"YourName\n"
        # Windows: b"\r\n" (handled as if it was \r and \n separately)
        if received == b"\r":
            self._on_enter_pressed()
            self._backslash_r_received = True
        elif received == b"\n":
            if not self._backslash_r_received:
                self._error = "Your terminal doesn't seem to be in raw mode. Run 'stty raw' and try again."
        elif received in BACKSPACE:
            # Don't just delete last byte, so that non-ascii can be erased
            # with a single backspace press
            self._name_so_far = self._get_name()[:-1].encode("utf-8")
        else:
            if len(self._name_so_far) < NAME_MAX_LENGTH:
                self._name_so_far += received

    def _on_enter_pressed(self) -> None:
        name = self._get_name().strip()
        if not name:
            self._error = "Please write a name before pressing Enter."
            return
        if any(c.isspace() and c != " " for c in name):
            self._error = (
                "The name can contain spaces, but not other whitespace characters."
            )
            return

        # Prevent two simultaneous clients with the same name.
        # But it's fine if you leave and then join back with the same name.
        if name.lower() in (
            client.name.lower()
            for client in self._client.server.clients
            if client.name is not None
        ):
            self._error = "This name is in use. Try a different name."
            return

        print(f"name asking done: {name!r}")
        self._client._send_bytes(HIDE_CURSOR)
        self._client.name = name
        self._client.view = ChooseGameView(self._client)


class MenuView:
    def __init__(self) -> None:
        self.menu_items: list[str] = []
        self.selected_index = 0

    def get_lines_to_render(self) -> list[bytes]:
        item_width = 30
        result = [b"", b""]
        for index, item in enumerate(self.menu_items):
            display_text = item.center(item_width).encode("utf-8")
            if index == self.selected_index:
                display_text = (COLOR % 47) + display_text  # white background
                display_text = (COLOR % 30) + display_text  # black foreground
                display_text += COLOR % 0
            result.append(b" " * ((80 - item_width) // 2) + display_text)
        return result

    # Return True to quit the game
    @abstractmethod
    def on_enter_pressed(self) -> bool | None:
        pass

    def handle_key_press(self, received: bytes) -> bool:
        if received in (UP_ARROW_KEY, b"W", b"w") and self.selected_index > 0:
            self.selected_index -= 1
        if received in (DOWN_ARROW_KEY, b"S", b"s") and self.selected_index + 1 < len(
            self.menu_items
        ):
            self.selected_index += 1
        if received == b"\r":
            return bool(self.on_enter_pressed())
        return False  # do not quit yet


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
        return self.selected_index < len(GAME_CLASSES) and any(
            isinstance(g, GAME_CLASSES[self.selected_index])
            and not g.player_can_join(self._client.name)
            for g in self._client.server.games
        )

    def _fill_menu(self) -> None:
        self.menu_items.clear()
        for game_class in GAME_CLASSES:
            ongoing_games = [
                g for g in self._client.server.games if isinstance(g, game_class)
            ]
            if ongoing_games:
                [game] = ongoing_games
                player_count = len(game.players)
            else:
                player_count = 0

            text = game_class.NAME
            if player_count == 1:
                text += " (1 player)"
            else:
                text += f" ({player_count} players)"
            self.menu_items.append(text)

        self.menu_items.append("Quit")

    def get_lines_to_render(self) -> list[bytes]:
        self._fill_menu()
        result = ASCII_ART.encode("ascii").split(b"\n") + super().get_lines_to_render()
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


class CheckTerminalSizeView:
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
            self._client.server.start_game(self._client, self._game_class)


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
            assert self._client.name is not None
            self._client.server.start_game(self._client, type(self.game))
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
    max_x = max(square.x for square in squares)
    max_y = max(square.y for square in squares)

    result = [[b"  "] * (max_x - min_x + 1) for y in range(min_y, max_y + 1)]
    for square in squares:
        result[square.y - min_y][square.x - min_x] = square.get_text(landed=False)
    return [b"".join(row) for row in result]


class PlayingView:
    def __init__(self, client: Client, game: Game, player: Player):
        self._client = client
        self._server = client.server
        # no idea why these need explicit type annotations
        self.game: Game = game
        self.player: Player = player

    def get_lines_to_render(self) -> list[bytes]:
        lines = self.game.get_lines_to_render(self.player)
        lines[5] += f"  Score: {self.game.score}".encode("ascii")
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


View = Union[
    AskNameView, ChooseGameView, CheckTerminalSizeView, PlayingView, GameOverView
]
