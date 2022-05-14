from __future__ import annotations

import re
import time
from abc import abstractmethod
from typing import TYPE_CHECKING, ClassVar

from catris.ansi import (
    BACKSPACE,
    COLOR,
    CSI,
    DOWN_ARROW_KEY,
    LEFT_ARROW_KEY,
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
    " !\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~¡¢£¤¥¦§¨©ª«¬®¯°±²³´µ¶·¸¹º»¼½¾¿×÷"
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

        self._client.log(f"Name asking done: {name!r}")
        self._client.name = name
        if self._client.server.only_lobby is None:
            # multiple lobbies mode
            self._client.view = ChooseIfNewLobbyView(self._client)
        else:
            self._client.server.only_lobby.add_client(self._client)
            self._client.view = ChooseGameView(self._client)

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


class MenuView(View):
    def __init__(self) -> None:
        self.menu_items: list[str | None] = []  # None means blank line
        self.selected_index = 0

    def get_lines_to_render(self) -> list[bytes]:
        item_width = 35
        result = [b"", b""]
        for index, item in enumerate(self.menu_items):
            if item is None:
                result.append(b"")
                continue

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
                while self.menu_items[self.selected_index] is None:
                    self.selected_index -= 1
                    assert self.selected_index >= 0
        elif received == DOWN_ARROW_KEY:
            if self.selected_index + 1 < len(self.menu_items):
                self.selected_index += 1
                while self.menu_items[self.selected_index] is None:
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
                    if text is not None and text.lower().startswith(
                        received_text.lower()
                    ):
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
        result = _ASCII_ART.split(b"\n") + super().get_lines_to_render()

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

    def on_enter_pressed(self) -> bool:
        text = self.menu_items[self.selected_index]
        if text == "Quit":
            return True
        elif text == "New lobby":
            # TODO: max number of lobbies?
            from catris.lobby import Lobby, generate_lobby_id

            lobby_id = generate_lobby_id(self._client.server.lobbies.keys())
            self._client.log(f"Creating new lobby: {lobby_id}")
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

        if not re.fullmatch(r"[A-Z0-9]{6}", lobby_id):
            self.error = "The text you entered doesn't look like a lobby ID."
            return

        self._last_attempt_time = time.monotonic()

        lobby = self._client.server.lobbies.get(lobby_id)
        if lobby is None:
            self._client.log(f"tried to join a non-existent lobby: {lobby_id}")
            self.error = f"There is no lobby with ID '{lobby_id}'."
            return
        if lobby.is_full:
            self._client.log(f"tried to join a full lobby: {lobby_id}")
            self.error = f"Lobby '{lobby_id}' is full. It already has {MAX_CLIENTS_PER_LOBBY} players."
            return

        lobby.add_client(self._client)
        self._client.view = ChooseGameView(self._client)


class ChooseGameView(MenuView):
    def __init__(self, client: Client, selection: str | type[Game] = GAME_CLASSES[0]):
        super().__init__()
        self._client = client
        self._fill_menu()

        # TODO: this sucks lol?
        if isinstance(selection, str):
            self.selected_index = self.menu_items.index(selection)
        else:
            self.selected_index = GAME_CLASSES.index(selection)

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

        self.menu_items.append(None)
        self.menu_items.append("Gameplay tips")
        self.menu_items.append("Quit")

    def get_lines_to_render(self) -> list[bytes]:
        from catris.lobby import MAX_CLIENTS_PER_LOBBY

        self._fill_menu()
        assert self._client.lobby is not None
        result = [b"", b""]
        if self._client.server.only_lobby is None:
            # Multiple lobbies mode
            if self._client.lobby_id_hidden:
                h_message = b" (press i to show)"
            else:
                h_message = b" (press i to hide)"
            result.append(
                b"   "
                + self._client.get_lobby_id_for_display()
                + (COLOR % 90)
                + h_message
                + (COLOR % 0)
            )
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
                text += (COLOR % 90) + b" (you)" + (COLOR % 0)
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

    def handle_key_press(self, received: bytes) -> bool:
        if received in (b"I", b"i"):
            self._client.lobby_id_hidden = not self._client.lobby_id_hidden
            return False
        return super().handle_key_press(received)

    def on_enter_pressed(self) -> bool:
        if self.menu_items[self.selected_index] == "Quit":
            return True
        if self.menu_items[self.selected_index] == "Gameplay tips":
            self._client.view = TipsView(self._client)
            return False

        if not self._should_show_cannot_join_error():
            game_class = GAME_CLASSES[self.selected_index]
            assert self._client.lobby is not None
            self._client.lobby.start_game(self._client, game_class)
        return False


_GAMEPLAY_TIPS = """
Keys:
  <Ctrl+C>, <Ctrl+D> or <Ctrl+Q>: quit
  <Ctrl+R>: redraw the whole screen (may be needed after resizing the window)
  <W>/<A>/<S>/<D> or <↑>/<←>/<↓>/<→>: move and rotate (don't hold down <S> or <↓>)
  <H>: hold (aka save) block for later, switch to previously held block if any
  <R>: change rotating direction
  <P>: pause/unpause (affects all players)
  <F>: flip the game upside down (only available in ring mode with 1 player)

There's [only one score]. You play together, not against other players. Try to
work together and make good use of everyone's blocks.

With multiple players, when your playing area fills all the way to the top,
you need to wait 30 seconds before you can continue playing. The game ends
when all players are simultaneously on their 30 seconds waiting time. This
means that if other players are doing well, you can [intentionally fill your
playing area] to do your waiting time before others mess up."""


class TipsView(MenuView):
    def __init__(self, client: Client) -> None:
        super().__init__()
        self._client = client
        self.menu_items = ["Back to menu"]

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

        if isinstance(self._client.connection, WebSocketConnection):
            # Ctrl keys e.g. Ctrl+C not supported or needed in web ui
            old_length = len(lines)
            lines = [line for line in lines if b"Ctrl+" not in line]
            assert len(lines) == old_length - 2

        lines.extend(super().get_lines_to_render())
        return lines

    def on_enter_pressed(self) -> None:
        self._client.view = ChooseGameView(self._client, "Gameplay tips")


class GameOverView(View):
    def __init__(self, client: Client, game: Game, new_high_score: HighScore):
        self._client = client
        self.game = game
        self.new_high_score = new_high_score
        self._high_scores: list[HighScore] | None = None

    def set_high_scores(self, high_scores: list[HighScore]) -> None:
        self._high_scores = high_scores

    def handle_key_press(self, received: bytes) -> None:
        if received == b"\r":
            self._client.view = ChooseGameView(self._client, type(self.game))

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


def get_block_preview(squares: set[Square]) -> list[bytes]:
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
        self._client = client
        self._server = client.server
        # no idea why these need explicit type annotations
        self.game: Game = game
        self.player: Player = player

    def get_lines_to_render(self) -> list[bytes]:
        lines = self.game.get_lines_to_render(self.player)
        assert self._client.lobby is not None
        lines[4] += b"  " + self._client.get_lobby_id_for_display()
        lines[5] += (
            (COLOR % 36) + f"  Score: {self.game.score}".encode("ascii") + (COLOR % 0)
        )
        if self._client.rotate_counter_clockwise:
            lines[6] += b"  Counter-clockwise"
        if self.game.is_paused:
            lines[7] += b"  Paused"

        if isinstance(self.player.moving_block_or_wait_counter, int):
            n = self.player.moving_block_or_wait_counter
            lines[12] += f"  Please wait: {n}".encode("ascii")
        else:
            lines[8] += b"  Next:"
            for index, row in enumerate(
                get_block_preview(self.player.next_moving_squares), start=10
            ):
                lines[index] += b"   " + row
            if self.player.held_squares is None:
                lines[16] += b"  Nothing in hold"
                lines[17] += b"     (press h)"
            else:
                lines[16] += b"  Holding:"
                for index, row in enumerate(
                    get_block_preview(self.player.held_squares), start=18
                ):
                    lines[index] += b"   " + row

        return lines

    def handle_key_press(self, received: bytes) -> None:
        if received in (b"R", b"r"):
            self._client.rotate_counter_clockwise = (
                not self._client.rotate_counter_clockwise
            )
            self._client.render()
            return

        if received in (b"P", b"p"):
            self.game.toggle_pause()
            return

        if self.game.is_paused:
            return

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
