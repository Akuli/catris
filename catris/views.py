from __future__ import annotations

import re
import time
from abc import abstractmethod
from typing import TYPE_CHECKING, Callable, ClassVar, Sequence

from catris.ansi import (
    BACKSPACE,
    COLOR,
    CSI,
    DOWN_ARROW_KEY,
    LEFT_ARROW_KEY,
    MOVE_CURSOR_TO_COLUMN,
    RIGHT_ARROW_KEY,
    UP_ARROW_KEY,
)
from catris.connections import WebSocketConnection
from catris.games import GAME_CLASSES, Game, RingGame
from catris.player import Player
from catris.squares import Square

if TYPE_CHECKING:
    from catris.high_scores import HighScore
    from catris.server_and_client import Client


_ASCII_ART = rb"""
                   __     ___    _____   _____   _____   ___
                  / _)   / _ \  |_   _| |  __ \ |_   _| / __)
                 | (_   / /_\ \   | |   |  _  /  _| |_  \__ \
                  \__) /_/   \_\  |_|   |_| \_\ |_____| (___/
                        https://github.com/Akuli/catris
"""


class View:
    def __init__(self, client: Client) -> None:
        self.client = client

    def get_terminal_size(self) -> tuple[int, int]:
        return (80, 24)

    # Can return lines or a tuple: (lines, cursor_pos)
    @abstractmethod
    def get_lines_to_render(self) -> list[bytes] | tuple[list[bytes], tuple[int, int]]:
        pass

    # Return True to quit the game. False and None do nothing.
    @abstractmethod
    def handle_key_press(self, received: bytes) -> bool | None:
        pass


class TextEntryView(View):
    PROMPT: ClassVar[str]

    def __init__(self, client: Client) -> None:
        super().__init__(client)
        self._text = b""
        self._backslash_r_received = False
        self.error = ""

    def get_lines_to_render(self) -> tuple[list[bytes], tuple[int, int]]:
        result = _ASCII_ART.splitlines()
        while len(result) < 10:
            result.append(b"")

        prompt_line = " " * 20 + self.PROMPT + self.get_text()
        result.append(prompt_line.encode("utf-8"))

        result.append(b"")
        result.append(b"")
        result.append((COLOR % 31) + b"  " + self.error.encode("utf-8") + (COLOR % 0))

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


# latin-1 chars, aka bytes(range(256)).decode("latin-1"), with some removed
# It's important to ban characters that are more than 1 unit wide on terminal.
VALID_NAME_CHARS = (
    " !\"#$%&'()*+-./:;<=>?@\\^_`{|}~¡¢£¤¥¦§¨©ª«¬®¯°±²³´µ¶·¸¹º»¼½¾¿×÷"
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    "abcdefghijklmnopqrstuvwxyz"
    "0123456789"
    "ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÐÑÒÓÔÕÖØÙÚÛÜÝÞßàáâãäåæçèéêëìíîïðñòóôõöøùúûüýþÿ"
)


class AskNameView(TextEntryView):
    PROMPT = "Name: "

    def on_enter_pressed(self) -> None:
        name = self.get_text().strip()
        if not name:
            self.error = "Please write a name before pressing Enter."
            return

        for char in name:
            if char not in VALID_NAME_CHARS:
                self.error = f"The name can't contain a '{char}' character."
                return

        # Prevent two simultaneous clients with the same name.
        # But it's fine if you leave and then join back with the same name.
        if name.lower() in (
            client.name.lower()
            for client in self.client.server.all_clients
            if client.name is not None
        ):
            self.error = "This name is in use. Try a different name."
            return

        if (
            self.client.server.only_lobby is not None
            and self.client.server.only_lobby.is_full
        ):
            self.error = "The server is full. Please try again later."
            return

        self.client.log(f"Name asking done: {name!r}")
        self.client.name = name
        if self.client.server.only_lobby is None:
            # multiple lobbies mode
            self.client.view = ChooseIfNewLobbyView(self.client)
        else:
            self.client.server.only_lobby.add_client(self.client)
            self.client.view = ChooseGameView(self.client)

    def get_lines_to_render(self) -> tuple[list[bytes], tuple[int, int]]:
        lines, cursor_pos = super().get_lines_to_render()
        lines.append(b"")
        lines.append(b"")
        lines.append(b"")

        texts = [
            b"If you play well, your name will be",
            b"visible to everyone in the high scores.",
            b"",
            b"Your IP will be logged on the server only if you",
            b"connect 5 or more times within the same minute.",
        ]
        for text in texts:
            lines.append(text.center(80).rstrip())

        return (lines, cursor_pos)


class _Menu:
    def __init__(
        self, items: Sequence[tuple[str, Callable[[], bool | None]] | None] = ()
    ) -> None:
        self.items = list(items)
        self.selected_index = 0

    def get_lines_to_render(
        self, *, width: int = 80, fill: bool = False
    ) -> list[bytes]:
        item_width = 35
        if fill:
            result = [b" " * width] * 2
        else:
            result = [b""] * 2
        for index, item in enumerate(self.items):
            if item is None:
                result.append(b"")
            else:
                text, callback = item
                display_text = text.center(item_width).encode("utf-8")
                if index == self.selected_index:
                    display_text = (COLOR % 47) + display_text  # white background
                    display_text = (COLOR % 30) + display_text  # black foreground
                    display_text += COLOR % 0
                left_fill = (width - item_width) // 2
                if fill:
                    right_fill = width - item_width - left_fill
                else:
                    right_fill = 0
                result.append(b" " * left_fill + display_text + b" " * right_fill)
        return result

    def handle_key_press(self, received: bytes) -> bool:
        if received == UP_ARROW_KEY:
            if self.selected_index > 0:
                self.selected_index -= 1
                while self.items[self.selected_index] is None:
                    self.selected_index -= 1
                    assert self.selected_index >= 0
        elif received == DOWN_ARROW_KEY:
            if self.selected_index + 1 < len(self.items):
                self.selected_index += 1
                while self.items[self.selected_index] is None:
                    self.selected_index += 1
        elif received == b"\r":
            item = self.items[self.selected_index]
            assert item is not None
            text, callback = item
            return bool(callback())
        else:
            # Select menu item whose text starts with pressed key.
            # Aka press r for ring mode
            try:
                received_text = received.decode("utf-8")
            except UnicodeDecodeError:
                pass
            else:
                for index, item in enumerate(self.items):
                    if item is not None:
                        text, callback = item
                        if text.lower().startswith(received_text.lower()):
                            self.selected_index = index
                            break
        return False  # do not quit yet


class ChooseIfNewLobbyView(View):
    def __init__(self, client: Client):
        super().__init__(client)
        self._menu = _Menu(
            [
                ("New lobby", self._new_lobby),
                ("Join an existing lobby", self._join_existing),
                ("Quit", lambda: True),
            ]
        )

    def get_lines_to_render(self) -> list[bytes]:
        result = _ASCII_ART.split(b"\n") + self._menu.get_lines_to_render()

        # Bring text to roughly same place as in previous view
        while len(result) < 17:
            result.append(b"")

        texts = [
            b"If you want to play alone, just make a new lobby.",
            b"",
            b"For multiplayer, one player makes a lobby and others join it.",
        ]
        for text in texts:
            result.append(text.center(80).rstrip())

        return result

    def handle_key_press(self, received: bytes) -> bool:
        return self._menu.handle_key_press(received)

    def _new_lobby(self) -> None:
        from catris.lobby import Lobby, generate_lobby_id

        lobby_id = generate_lobby_id(self.client.server.lobbies.keys())
        self.client.log(f"Creating new lobby: {lobby_id}")
        lobby = Lobby(lobby_id)
        self.client.server.lobbies[lobby_id] = lobby
        lobby.add_client(self.client)
        self.client.view = ChooseGameView(self.client)

    def _join_existing(self) -> None:
        self.client.view = AskLobbyIDView(self.client)


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

        if not re.fullmatch(r"[A-Z0-9]{6}", lobby_id):
            self.error = "The text you entered doesn't look like a lobby ID."
            return

        self._last_attempt_time = time.monotonic()

        lobby = self.client.server.lobbies.get(lobby_id)
        if lobby is None:
            self.client.log(f"tried to join a non-existent lobby: {lobby_id}")
            self.error = f"There is no lobby with ID '{lobby_id}'."
            return
        if lobby.is_full:
            self.client.log(f"tried to join a full lobby: {lobby_id}")
            self.error = f"Lobby '{lobby_id}' is full. It already has {MAX_CLIENTS_PER_LOBBY} players."
            return

        lobby.add_client(self.client)
        self.client.view = ChooseGameView(self.client)


class ChooseGameView(View):
    def __init__(
        self, client: Client, selected_game_class: type[Game] = GAME_CLASSES[0]
    ):
        super().__init__(client)
        self._menu = _Menu()
        self._fill_menu()
        self._menu.selected_index = GAME_CLASSES.index(selected_game_class)

    def _should_show_cannot_join_error(self) -> bool:
        assert self.client.name is not None
        assert self.client.lobby is not None

        if self._menu.selected_index >= len(GAME_CLASSES):
            return False
        game = self.client.lobby.games.get(GAME_CLASSES[self._menu.selected_index])
        return game is not None and len(game.players) >= type(game).get_max_players()

    def _fill_menu(self) -> None:
        assert self.client.lobby is not None
        self._menu.items.clear()
        for game_class in GAME_CLASSES:
            game = self.client.lobby.games.get(game_class, None)
            current = 0 if game is None else len(game.players)
            maximum = game_class.get_max_players()
            self._menu.items.append(
                (
                    f"{game_class.NAME} ({current}/{maximum} players)",
                    self._start_playing,
                )
            )

        self._menu.items.append(None)
        self._menu.items.append(("Gameplay tips", self._show_gameplay_tips))
        self._menu.items.append(("Quit", lambda: True))

    def get_lines_to_render(self) -> list[bytes]:
        from catris.lobby import MAX_CLIENTS_PER_LOBBY

        self._fill_menu()
        assert self.client.lobby is not None
        result = [b"", b""]
        if self.client.server.only_lobby is None:
            # Multiple lobbies mode
            if self.client.lobby_id_hidden:
                h_message = b" (press i to show)"
            else:
                h_message = b" (press i to hide)"
            result.append(
                b"   "
                + self.client.get_lobby_id_for_display()
                + (COLOR % 90)
                + h_message
                + (COLOR % 0)
            )
            result.append(b"")
            result.append(b"   Players in lobby:")
        else:
            result.append(b"   Players:")

        for number, client in enumerate(self.client.lobby.clients, start=1):
            assert client.name is not None
            assert client.color is not None
            text = (
                f"      {number}. ".encode("ascii")
                + (COLOR % client.color)
                + client.name.encode("utf-8")
                + (COLOR % 0)
            )
            if client == self.client:
                text += (COLOR % 90) + b" (you)" + (COLOR % 0)
            result.append(text)
        result.extend([b""] * (MAX_CLIENTS_PER_LOBBY - len(self.client.lobby.clients)))
        result.extend(self._menu.get_lines_to_render())
        if self._should_show_cannot_join_error():
            result.append(b"")
            result.append(b"")
            result.append(
                (COLOR % 31) + b"This game is full.".center(80).rstrip() + (COLOR % 0)
            )
        return result

    def handle_key_press(self, received: bytes) -> bool:
        if received in (b"I", b"i"):
            self.client.lobby_id_hidden = not self.client.lobby_id_hidden
            return False
        return self._menu.handle_key_press(received)

    def _show_gameplay_tips(self) -> None:
        self.client.view = TipsView(self.client, self)

    def _start_playing(self) -> None:
        if not self._should_show_cannot_join_error():
            game_class = GAME_CLASSES[self._menu.selected_index]
            assert self.client.lobby is not None
            self.client.lobby.start_game(self.client, game_class)


_GAMEPLAY_TIPS = """
Keys:
  <Ctrl+C>, <Ctrl+D> or <Ctrl+Q>: quit
  <Ctrl+R>: redraw the whole screen (may be needed after resizing the window)
  <W>/<A>/<S>/<D> or <↑>/<←>/<↓>/<→>: move and rotate (don't hold down <S> or <↓>)
  <H>: hold (aka save) block for later, switch to previously held block if any
  <R>: change rotating direction
  <P>: pause/unpause (affects all players)
  <F>: flip the game upside down (only available in ring mode with 1 player)

There's only one score. [You play together], not against other players. Try to
work together and make good use of everyone's blocks.

With multiple players, when your playing area fills all the way to the top,
you need to wait 30 seconds before you can continue playing. The game ends
when all players are simultaneously on their 30 seconds waiting time. This
means that if other players are doing well, you can [intentionally fill your
playing area] to do your waiting time before others mess up."""


class TipsView(View):
    def __init__(self, client: Client, previous_view: View) -> None:
        super().__init__(client)
        self._previous_view = previous_view
        self._menu = _Menu([("Back to menu", self._back_to_previous_view)])

    def _back_to_previous_view(self) -> None:
        self.client.view = self._previous_view

    def get_lines_to_render(self) -> list[bytes]:
        lines = (
            _GAMEPLAY_TIPS.encode("utf-8")
            .replace(b"\n", b"\n  ")
            .replace(b"[", COLOR % 36)  # must be first because COLOR contains "["
            .replace(b"]", COLOR % 0)
            .replace(b"<", COLOR % 35)
            .replace(b">", COLOR % 0)
            .splitlines()
        )

        if isinstance(self.client.connection, WebSocketConnection):
            # Ctrl keys e.g. Ctrl+C not supported or needed in web ui
            old_length = len(lines)
            lines = [line for line in lines if b"Ctrl+" not in line]
            assert len(lines) == old_length - 2

        return lines + self._menu.get_lines_to_render()

    def handle_key_press(self, received: bytes) -> bool:
        return self._menu.handle_key_press(received)


class GameOverView(View):
    def __init__(self, client: Client, game: Game, new_high_score: HighScore):
        super().__init__(client)
        self.game = game
        self.new_high_score = new_high_score
        self._high_scores: list[HighScore] | None = None

    def set_high_scores(self, high_scores: list[HighScore]) -> None:
        self._high_scores = high_scores

    def handle_key_press(self, received: bytes) -> None:
        if received == b"\r":
            self.client.view = ChooseGameView(self.client, type(self.game))

    def get_lines_to_render(self) -> list[bytes]:
        if self._high_scores is None:
            return [b"", b"", b"Loading...".center(80).rstrip()]

        lines = [b"", b""]

        if self.new_high_score in self._high_scores:
            lines.append(b"Game Over :)".center(80).rstrip())
        else:
            lines.append(b"Game Over :(".center(80).rstrip())

        padding = (80 - len(b"Your score was %d." % self.new_high_score.score)) // 2
        lines.append(
            b"%sYour score was %s%d%s."
            % (b" " * padding, COLOR % 36, self.new_high_score.score, COLOR % 0)
        )

        lines.append(b"")
        lines.append(b"")
        lines.append(b"=== HIGH SCORES ".ljust(80, b"="))
        lines.append(b"")
        if len(self.new_high_score.players) >= 2:
            lines.append(b"| Score | Duration | Players")
        else:
            lines.append(b"| Score | Duration | Player")
        lines.append(b"|-------|----------|-------".ljust(80, b"-"))

        for hs in self._high_scores:
            player_string = ", ".join(hs.players)
            line_string = (
                f"| {hs.score:<6}| {hs.get_duration_string():<9}| {player_string}"
            )

            n = 80 - len(line_string)
            if len(line_string) > n:
                # TODO: not ideal, truncate each player name instead?
                line_string = line_string[:n-3] + "..."

            line = line_string.encode("utf-8")
            if hs == self.new_high_score:
                lines.append((COLOR % 42) + line)
            else:
                lines.append((COLOR % 0) + line)

        lines.append(COLOR % 0)  # Needed if last score was highlighted
        lines.append(b"")
        lines.append(b"")
        lines.append(b"")
        lines.append(b"Press Enter to continue...".center(80).rstrip())
        return lines


def _get_block_preview(squares: set[Square]) -> list[bytes]:
    min_x = min(square.x for square in squares)
    min_y = min(square.y for square in squares)
    width = max(square.x - min_x for square in squares) + 1
    height = max(square.y - min_y for square in squares) + 1

    result = [[b"  "] * width for y in range(height)]
    for square in squares:
        result[square.y - min_y][square.x - min_x] = square.get_text(
            player=None, landed=False
        )
    return [b"".join(row) for row in result]


class PlayingView(View):
    def __init__(self, client: Client, game: Game, player: Player):
        super().__init__(client)
        # no idea why these need explicit type annotations
        self.game: Game = game
        self.player: Player = player
        self._paused_menu = _Menu(
            [("Continue playing", game.toggle_pause), ("Quit game", self.quit_game)]
        )

    def get_terminal_size(self) -> tuple[int, int]:
        width, height = self.game.get_terminal_size()
        width += 22  # room for UI on the side
        return (max(width, 80), height)

    def quit_game(self) -> None:
        assert self.client.lobby is not None
        assert self.client.lobby.games[type(self.game)] == self.game
        self.game.remove_player(self.player)
        self.game.need_render_event.set()
        self.client.view = ChooseGameView(self.client, type(self.game))
        self.client.lobby.update_choose_game_views()

    def get_lines_to_render(self) -> list[bytes]:
        lines = self.game.get_lines_to_render(self.player)
        assert self.client.lobby is not None
        lines[4] += b"  " + self.client.get_lobby_id_for_display()
        lines[5] += (
            (COLOR % 36) + f"  Score: {self.game.score}".encode("ascii") + (COLOR % 0)
        )
        if self.client.rotate_counter_clockwise:
            lines[6] += b"  Counter-clockwise"

        if isinstance(self.player.moving_block_or_wait_counter, int):
            n = self.player.moving_block_or_wait_counter
            lines[12] += f"  Please wait: {n}".encode("ascii")
        else:
            lines[8] += b"  Next:"
            for index, row in enumerate(
                _get_block_preview(self.player.next_moving_squares), start=10
            ):
                lines[index] += b"   " + row
            if self.player.held_squares is None:
                lines[16] += b"  Nothing in hold"
                lines[17] += b"     (press h)"
            else:
                lines[16] += b"  Holding:"
                for index, row in enumerate(
                    _get_block_preview(self.player.held_squares), start=18
                ):
                    lines[index] += b"   " + row

        if self.game.is_paused:
            width = 60
            paused_lines = [
                b"o%so" % (b"=" * width),
                b"|%s|" % (b" " * width),
                b"|%s|" % (b" " * width),
                b"|%s|" % b" Game paused ".center(width),
                b"|%s|" % b"^^^^^^^^^^^^^".center(width),
                *[
                    b"|" + (COLOR % 0) + line + (COLOR % 92) + b"|"
                    for line in self._paused_menu.get_lines_to_render(
                        width=width, fill=True
                    )
                ],
                b"|%s|" % (b" " * width),
                b"|%s|" % (b" " * width),
                b"|%s|" % b"You will be disconnected automatically if".center(width),
                b"|%s|" % b"you don't press any keys for 10 minutes.".center(width),
                b"|%s|" % (b" " * width),
                b"|%s|" % (b" " * width),
                b"o%so" % (b"=" * width),
            ]

            terminal_width, terminal_height = self.get_terminal_size()

            for index, line in enumerate(
                paused_lines, start=(terminal_height - len(paused_lines)) // 2
            ):
                left = (terminal_width - width - 2) // 2
                lines[index] += (
                    (MOVE_CURSOR_TO_COLUMN % (left + 1))
                    + (COLOR % 92)
                    + line
                    + (COLOR % 0)
                    + (MOVE_CURSOR_TO_COLUMN % terminal_width)
                )
        else:
            self._paused_menu.selected_index = 0

        return lines

    def handle_key_press(self, received: bytes) -> bool | None:
        if received in (b"P", b"p"):
            self.game.toggle_pause()
            return None

        if self.game.is_paused:
            return self._paused_menu.handle_key_press(received)

        if received in (b"R", b"r"):
            self.client.rotate_counter_clockwise = (
                not self.client.rotate_counter_clockwise
            )
            self.client.render()
        elif received in (b"A", b"a", LEFT_ARROW_KEY):
            self.game.move_if_possible(self.player, dx=-1, dy=0, in_player_coords=True)
            self.player.set_fast_down(False)
        elif received in (b"D", b"d", RIGHT_ARROW_KEY):
            self.game.move_if_possible(self.player, dx=1, dy=0, in_player_coords=True)
            self.player.set_fast_down(False)
        elif received in (b"W", b"w", UP_ARROW_KEY, b"\r"):
            self.game.rotate_if_possible(
                self.player, self.client.rotate_counter_clockwise
            )
            self.player.set_fast_down(False)
        elif received in (b"S", b"s", DOWN_ARROW_KEY, b" "):
            self.player.set_fast_down(True)
        elif received in (b"H", b"h"):
            self.game.hold_block(self.player)
            self.player.set_fast_down(False)
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
        return None
