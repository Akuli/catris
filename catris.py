from __future__ import annotations
import copy
import dataclasses
import time
import contextlib
import socketserver
import threading
import socket
import random
import queue
from typing import Iterator

ASCII_ART = r"""
                   __     ___    _____   _____   _____   ___
                  / _)   / _ \  |_   _| |  __ \ |_   _| / __)
                 | (_   / /_\ \   | |   |  _  /  _| |_  \__ \
                  \__) /_/   \_\  |_|   |_| \_\ |_____| (___/
                        https://github.com/Akuli/catris
"""

# https://en.wikipedia.org/wiki/ANSI_escape_code
ESC = b"\x1b"
CSI = ESC + b"["
CLEAR_SCREEN = CSI + b"2J"
CLEAR_FROM_CURSOR_TO_END_OF_SCREEN = CSI + b"0J"
MOVE_CURSOR = CSI + b"%d;%dH"
SHOW_CURSOR = CSI + b"?25h"
HIDE_CURSOR = CSI + b"?25l"
COLOR = CSI + b"1;%dm"  # "COLOR % 0" resets to default colors
CLEAR_TO_END_OF_LINE = CSI + b"0K"

# figured out with prints
CONTROL_C = b"\x03"
CONTROL_D = b"\x04"
CONTROL_Q = b"\x11"
BACKSPACE = b"\x7f"
UP_ARROW_KEY = CSI + b"A"
DOWN_ARROW_KEY = CSI + b"B"
RIGHT_ARROW_KEY = CSI + b"C"
LEFT_ARROW_KEY = CSI + b"D"

# Width varies as people join/leave
HEIGHT = 20
WIDTH_PER_PLAYER = 7

BLOCK_SHAPES = {
    "L": [(-1, 0), (0, 0), (1, 0), (1, -1)],
    "I": [(-2, 0), (-1, 0), (0, 0), (1, 0)],
    "J": [(-1, -1), (-1, 0), (0, 0), (1, 0)],
    "O": [(-1, 0), (0, 0), (0, -1), (-1, -1)],
    "T": [(-1, 0), (0, 0), (1, 0), (0, -1)],
    "Z": [(-1, -1), (0, -1), (0, 0), (1, 0)],
    "S": [(1, -1), (0, -1), (0, 0), (-1, 0)],
}
BLOCK_COLORS = {
    # Colors from here: https://tetris.fandom.com/wiki/Tetris_Guideline
    "L": 47,  # white, but should be orange (not available in standard ansi colors)
    "I": 46,  # cyan
    "J": 44,  # blue
    "O": 43,  # yellow
    "T": 45,  # purple
    "Z": 41,  # red
    "S": 42,  # green
}

# If you mess up, how many seconds should you wait?
WAIT_TIME = 10


class MovingBlock:
    def __init__(self, player_index: int):
        self.shape_letter = random.choice(list(BLOCK_SHAPES.keys()))
        self.center_x = (WIDTH_PER_PLAYER * player_index) + (WIDTH_PER_PLAYER // 2)
        self.center_y = -1
        self.rotation = 0

    def get_coords(self) -> set[tuple[int, int]]:
        result = set()
        for rel_x, rel_y in BLOCK_SHAPES[self.shape_letter]:
            for iteration in range(self.rotation % 4):
                rel_x, rel_y = -rel_y, rel_x
            result.add((self.center_x + rel_x, self.center_y + rel_y))
        return result


@dataclasses.dataclass(eq=False)
class Player:
    name: str
    color: int
    rotate_counter_clockwise: bool = False
    moving_block_or_wait_counter: MovingBlock | int | None = None


class GameState:
    def __init__(self) -> None:
        self.reset()

    def reset(self) -> None:
        self.game_id = time.monotonic_ns()
        self.players: list[Player] = []
        self._landed_blocks: list[list[int | None]] = [[] for y in range(HEIGHT)]
        self.score = 0

    def game_is_over(self) -> bool:
        return bool(self.players) and not any(
            isinstance(p.moving_block_or_wait_counter, MovingBlock)
            for p in self.players
        )

    def end_waiting(self, player: Player, client_currently_connected: bool) -> None:
        assert player.moving_block_or_wait_counter == 0
        if not client_currently_connected:
            player.moving_block_or_wait_counter = None
            return

        index = self.players.index(player)
        x_min = WIDTH_PER_PLAYER * index
        x_max = x_min + WIDTH_PER_PLAYER
        for row in self._landed_blocks:
            row[x_min:x_max] = [None] * WIDTH_PER_PLAYER
        player.moving_block_or_wait_counter = MovingBlock(index)

    def get_width(self) -> int:
        return WIDTH_PER_PLAYER * len(self.players)

    def _get_moving_blocks(self) -> list[MovingBlock]:
        result = []
        for player in self.players:
            if isinstance(player.moving_block_or_wait_counter, MovingBlock):
                result.append(player.moving_block_or_wait_counter)
        return result

    def is_valid(self) -> bool:
        seen = set()

        for y, row in enumerate(self._landed_blocks):
            for x, color in enumerate(row):
                if color is not None:
                    seen.add((x, y))

        for block in self._get_moving_blocks():
            coords = block.get_coords()
            if coords & seen or not all(
                x in range(self.get_width()) and y < HEIGHT for x, y in coords
            ):
                return False
            seen.update(coords)
        return True

    def find_full_lines(self) -> list[int]:
        return [
            y for y, row in enumerate(self._landed_blocks) if row and None not in row
        ]

    # Between find_full_lines and clear_full_lines, there's a flashing animation.
    # Color can't be None, because then it would be possible to put blocks to a flashing line.
    def set_color_of_lines(self, full_lines: list[int], color: int) -> None:
        for y in full_lines:
            self._landed_blocks[y] = [color] * self.get_width()

    def clear_lines(self, full_lines: list[int]) -> None:
        if len(full_lines) == 0:
            single_player_score = 0
        elif len(full_lines) == 1:
            single_player_score = 10
        elif len(full_lines) == 2:
            single_player_score = 30
        elif len(full_lines) == 3:
            single_player_score = 60
        else:
            single_player_score = 100

        # It's more difficult to get full lines with more players.
        # A line is full in the game, if all players have it player-specifically full.
        # If players stick to their own areas and are independent:
        #
        #     P(line clear with n players)
        #   = P(player 1 full AND player 2 full AND ... AND player n full)
        #   = P(player 1 full) * P(player 2 full) * ... * P(player n full)
        #   = P(line clear with 1 player)^n
        #
        # This means the game gets exponentially more difficult with more players.
        # We try to compensate for this by giving exponentially more points.
        n = len(self.players)
        if n >= 1:  # avoid floats
            self.score += single_player_score * 2 ** (n - 1)

        self._landed_blocks = [
            row for y, row in enumerate(self._landed_blocks) if y not in full_lines
        ]
        while len(self._landed_blocks) < HEIGHT:
            self._landed_blocks.insert(0, [None] * self.get_width())

        # When landed blocks move down, they can go on top of moving blocks.
        # This is quite rare, but results in invalid state errors.
        # When this happens, just delete the landed block.
        for moving_block in self._get_moving_blocks():
            for x, y in moving_block.get_coords():
                if y >= 0:
                    self._landed_blocks[y][x] = None
        assert self.is_valid()

    def get_square_colors(self) -> list[list[int | None]]:
        assert self.is_valid()
        result = copy.deepcopy(self._landed_blocks)
        for moving_block in self._get_moving_blocks():
            for x, y in moving_block.get_coords():
                if y >= 0:
                    result[y][x] = BLOCK_COLORS[moving_block.shape_letter]
        return result

    def move_if_possible(self, player: Player, dx: int, dy: int) -> bool:
        assert self.is_valid()
        if isinstance(player.moving_block_or_wait_counter, MovingBlock):
            player.moving_block_or_wait_counter.center_x += dx
            player.moving_block_or_wait_counter.center_y += dy
            if self.is_valid():
                return True
            player.moving_block_or_wait_counter.center_x -= dx
            player.moving_block_or_wait_counter.center_y -= dy

        return False

    def move_down_all_the_way(self, player: Player) -> None:
        while self.move_if_possible(player, dx=0, dy=1):
            pass

    def rotate(self, player: Player) -> None:
        if isinstance(player.moving_block_or_wait_counter, MovingBlock):
            block = player.moving_block_or_wait_counter
            if block.shape_letter == "O":
                return

            old_rotation = block.rotation
            if player.rotate_counter_clockwise:
                new_rotation = old_rotation - 1
            else:
                new_rotation = old_rotation + 1

            if block.shape_letter in "ISZ":
                new_rotation %= 2

            assert self.is_valid()
            block.rotation = new_rotation
            if not self.is_valid():
                block.rotation = old_rotation

    # None return value means server full
    def add_player(self, name: str) -> Player:
        game_over = self.game_is_over()

        # Name can exist already, if player quits and comes back
        for player in self.players:
            if player.name == name:
                break
        else:
            color = min(PLAYER_COLORS - {p.color for p in self.players})
            player = Player(name, color)
            self.players.append(player)
            for row in self._landed_blocks:
                row.extend([None] * WIDTH_PER_PLAYER)

        if not game_over and not isinstance(player.moving_block_or_wait_counter, int):
            player.moving_block_or_wait_counter = MovingBlock(
                self.players.index(player)
            )
            assert not self.game_is_over()
        return player

    def move_blocks_down(self) -> set[Player]:
        # Blocks of different users can be on each other's way, but should
        # still be moved if the bottommost block will move.
        #
        # Solution: repeatedly try to move each one, and stop when nothing moves.
        todo = {
            player
            for player in self.players
            if isinstance(player.moving_block_or_wait_counter, MovingBlock)
        }
        while True:
            something_moved = False
            for player in todo.copy():
                moved = self.move_if_possible(player, dx=0, dy=1)
                if moved:
                    something_moved = True
                    todo.remove(player)
            if not something_moved:
                break

        needs_wait_counter = set()
        for player in todo:
            assert isinstance(player.moving_block_or_wait_counter, MovingBlock)
            letter = player.moving_block_or_wait_counter.shape_letter
            coords = player.moving_block_or_wait_counter.get_coords()

            if any(y < 0 for x, y in coords):
                needs_wait_counter.add(player)
            else:
                for x, y in coords:
                    self._landed_blocks[y][x] = BLOCK_COLORS[letter]
                index = self.players.index(player)
                player.moving_block_or_wait_counter = MovingBlock(index)

        for player in needs_wait_counter:
            player.moving_block_or_wait_counter = WAIT_TIME
        return needs_wait_counter


# If you want to play with more than 4 players, use bigger terminal than 80x24
PLAYER_COLORS = {31, 32, 33, 34, 35, 36, 37}  # foreground colors


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True

    def __init__(self, port: int):
        super().__init__(("", port), Client)

        # RLock because state usage triggers rendering, which uses state
        self.lock = threading.RLock()
        # All of the below are locked with self.lock:
        self.__state = GameState()  # see access_game_state()
        self.clients: set[Client] = set()

        threading.Thread(target=self._move_blocks_down_thread).start()

    @contextlib.contextmanager
    def access_game_state(self, *, render: bool = True) -> Iterator[GameState]:
        with self.lock:
            assert self.__state.is_valid()
            assert not self.__state.game_is_over()
            yield self.__state

            assert self.__state.is_valid()
            if render:
                if self.__state.game_is_over():
                    score = self.__state.score
                    print("Game over! Score", score)
                    self.__state.reset()
                    for client in self.clients:
                        if isinstance(client.view, PlayingView):
                            client.view = GameOverView(client, score)
                            client.render()
                else:
                    for client in self.clients:
                        if isinstance(client.view, PlayingView):
                            client.render()

    def _countdown(self, player: Player, game_id: int) -> None:
        while True:
            time.sleep(1)
            with self.access_game_state() as state:
                if state.game_id != game_id:
                    return

                assert isinstance(player.moving_block_or_wait_counter, int)
                player.moving_block_or_wait_counter -= 1
                if player.moving_block_or_wait_counter == 0:
                    client_currently_connected = any(
                        isinstance(client.view, PlayingView)
                        and client.view.player == player
                        for client in self.clients
                    )
                    state.end_waiting(player, client_currently_connected)
                    return

    def _move_blocks_down_once(self) -> None:
        with self.access_game_state() as state:
            game_id = state.game_id
            needs_wait_counter = state.move_blocks_down()
            full_lines = state.find_full_lines()
            for player in needs_wait_counter:
                threading.Thread(target=self._countdown, args=[player, game_id]).start()

        if full_lines:
            for color in [47, 0, 47, 0]:
                with self.access_game_state() as state:
                    if state.game_id != game_id:
                        return
                    state.set_color_of_lines(full_lines, color)
                time.sleep(0.1)
            with self.access_game_state() as state:
                if state.game_id != game_id:
                    return
                state.clear_lines(full_lines)

    def _move_blocks_down_thread(self) -> None:
        while True:
            self._move_blocks_down_once()
            with self.access_game_state(render=False) as state:
                score = state.score
            time.sleep(0.5 / (1 + score / 1000))


class AskNameView:
    def __init__(self, client: Client):
        assert client.name is None
        self._client = client
        self._name_so_far = b""
        self._error: str | None = None

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
        if received == b"\n":
            self._error = "Your terminal doesn't seem to be in raw mode. Run 'stty raw' and try again."
        elif received == b"\r":
            self._start_playing()
        elif received == BACKSPACE:
            # Don't just delete last byte, so that non-ascii can be erased
            # with a single backspace press
            self._name_so_far = self._get_name()[:-1].encode("utf-8")
        else:
            self._name_so_far += received

    def _start_playing(self) -> None:
        name = self._get_name()
        if not name:
            self._error = "Please write a name before pressing Enter."
            return
        if len(f"[{name}] {WAIT_TIME}") > 2 * WIDTH_PER_PLAYER:
            self._error = "The name is too long."
            return

        # Must lock while assigning name and color, so can't get duplicates
        with self._client.server.access_game_state() as state:
            names_of_connected_players = {
                client.name
                for client in self._client.server.clients
                if client.name is not None
            }
            names_in_use = names_of_connected_players | {p.name for p in state.players}

            if len(names_in_use) == len(PLAYER_COLORS):
                self._error = "Server is full. Please try again later."
                return

            # Prevent two simultaneous clients with the same name.
            # But it's fine if you leave and then join back with the same name.
            if name in names_of_connected_players:
                self._error = "This name is in use. Try a different name."
                return

            print(self._client.client_address, f"starting game as {name!r}")
            self._client.send_queue.put(HIDE_CURSOR)
            self._client.name = name
            player = state.add_player(name)
            self._client.view = PlayingView(self._client, player)


class PlayingView:
    def __init__(self, client: Client, player: Player):
        self._client = client
        self.player = player

    def get_lines_to_render(self) -> list[bytes]:
        with self._client.server.access_game_state(render=False) as state:
            header_line = b"o"
            name_line = b" "
            for player in state.players:
                if player.moving_block_or_wait_counter is None:
                    # Player disconnected
                    display_name = f"[{player.name}]"
                elif isinstance(player.moving_block_or_wait_counter, int):
                    # Waiting for the countdown
                    display_name = (
                        f"[{player.name}] {player.moving_block_or_wait_counter}"
                    )
                else:
                    display_name = player.name

                color_bytes = COLOR % player.color
                header_line += color_bytes
                name_line += color_bytes

                if player == self.player:
                    header_line += b"==" * WIDTH_PER_PLAYER
                else:
                    header_line += b"--" * WIDTH_PER_PLAYER
                name_line += display_name.center(2 * WIDTH_PER_PLAYER).encode("utf-8")

            name_line += COLOR % 0
            header_line += COLOR % 0
            header_line += b"o"

            lines = [name_line, header_line]

            for blink_y, row in enumerate(state.get_square_colors()):
                line = b"|"
                for color in row:
                    if color is None:
                        line += b"  "
                    else:
                        line += COLOR % color
                        line += b"  "
                        line += COLOR % 0
                line += b"|"
                lines.append(line)

            lines.append(b"o" + b"--" * state.get_width() + b"o")

            lines[5] += f"  Score: {state.score}".encode("ascii")
            if self.player.rotate_counter_clockwise:
                lines[6] += b"  Counter-clockwise"
            if isinstance(self.player.moving_block_or_wait_counter, int):
                n = self.player.moving_block_or_wait_counter
                lines[8] += f"  Please wait: {n}".encode("ascii")

            return lines

    def handle_key_press(self, received: bytes) -> None:
        if received in (b"A", b"a", LEFT_ARROW_KEY):
            with self._client.server.access_game_state() as state:
                state.move_if_possible(self.player, dx=-1, dy=0)
        elif received in (b"D", b"d", RIGHT_ARROW_KEY):
            with self._client.server.access_game_state() as state:
                state.move_if_possible(self.player, dx=1, dy=0)
        elif received in (b"W", b"w", UP_ARROW_KEY, b"\r"):
            with self._client.server.access_game_state() as state:
                state.rotate(self.player)
        elif received in (b"S", b"s", DOWN_ARROW_KEY, b" "):
            with self._client.server.access_game_state() as state:
                state.move_down_all_the_way(self.player)
        elif received in (b"R", b"r"):
            self.player.rotate_counter_clockwise = (
                not self.player.rotate_counter_clockwise
            )


class GameOverView:
    def __init__(self, client: Client, score: int):
        self._client = client
        self._score = score
        self._all_menu_items = ["New Game", "Quit"]
        self._selected_item = "New Game"

    def get_lines_to_render(self) -> list[bytes]:
        lines = [b""] * 7
        lines[3] = b"Game Over :(".center(80).rstrip()
        lines[4] = f"Your score was {self._score}.".encode("ascii").center(80).rstrip()

        item_width = 20

        for menu_item in self._all_menu_items:
            display_text = menu_item.center(item_width).encode("utf-8")
            if menu_item == self._selected_item:
                display_text = (COLOR % 47) + display_text  # white background
                display_text = (COLOR % 30) + display_text  # black foreground
                display_text += COLOR % 0
            lines.append(b" " * ((80 - item_width) // 2) + display_text)

        return lines

    def handle_key_press(self, received: bytes) -> bool:
        i = self._all_menu_items.index(self._selected_item)
        if received in (UP_ARROW_KEY, b"W", b"w") and i > 0:
            self._selected_item = self._all_menu_items[i - 1]
        if received in (DOWN_ARROW_KEY, b"S", b"s"):
            try:
                self._selected_item = self._all_menu_items[i + 1]
            except IndexError:
                pass
        if received == b"\r":
            if self._selected_item == "New Game":
                assert self._client.name is not None
                with self._client.server.access_game_state() as state:
                    player = state.add_player(self._client.name)
                    self._client.view = PlayingView(self._client, player)
            elif self._selected_item == "Quit":
                return True
            else:
                raise NotImplementedError(self._selected_item)

        return False  # do not quit yet


class Client(socketserver.BaseRequestHandler):
    server: Server
    request: socket.socket

    def setup(self) -> None:
        print(self.client_address, "New connection")
        self.last_displayed_lines: list[bytes] = []
        self.send_queue: queue.Queue[bytes | None] = queue.Queue()
        self.name: str | None = None
        self.view: AskNameView | PlayingView | GameOverView = AskNameView(self)

    def render(self) -> None:
        # Bottom of terminal. If user types something, it's unlikely to be
        # noticed here before it gets wiped by the next refresh.
        cursor_pos = (24, 1)

        if isinstance(self.view, AskNameView):
            lines, cursor_pos = self.view.get_lines_to_render_and_cursor_pos()
        else:
            lines = self.view.get_lines_to_render()

        while len(lines) < len(self.last_displayed_lines):
            lines.append(b"")
        while len(lines) > len(self.last_displayed_lines):
            self.last_displayed_lines.append(b"")

        # Send it all at once, so that hopefully cursor won't be in a
        # temporary place for long times, even if internet is slow
        to_send = b""

        for y, (old_line, new_line) in enumerate(zip(self.last_displayed_lines, lines)):
            if old_line != new_line:
                to_send += MOVE_CURSOR % (y + 1, 1)
                to_send += new_line
                to_send += CLEAR_TO_END_OF_LINE
        self.last_displayed_lines = lines.copy()

        to_send += MOVE_CURSOR % cursor_pos
        to_send += CLEAR_TO_END_OF_LINE

        self.send_queue.put(to_send)

    def _receive_bytes(self, maxsize: int) -> bytes | None:
        try:
            result = self.request.recv(maxsize)
        except OSError as e:
            print(self.client_address, e)
            self.send_queue.put(None)
            return None

        # Checking ESC key here is a bad idea.
        # Arrow keys are sent as ESC + other bytes, and recv() can sometimes
        # return only some of the sent data.
        if result in {CONTROL_C, CONTROL_D, CONTROL_Q, b""}:
            self.send_queue.put(None)
            return None

        return result

    def _send_queue_thread(self) -> None:
        while True:
            item = self.send_queue.get()
            if item is not None:
                try:
                    self.request.sendall(item)
                    continue
                except OSError as e:
                    print(self.client_address, e)

            with self.server.access_game_state() as state:
                self.server.clients.remove(self)
                if isinstance(self.view, PlayingView) and isinstance(
                    self.view.player.moving_block_or_wait_counter, MovingBlock
                ):
                    self.view.player.moving_block_or_wait_counter = None
                if not any(
                    isinstance(c.view, PlayingView) for c in self.server.clients
                ):
                    state.reset()

            print(self.client_address, "Disconnect")
            try:
                self.request.sendall(SHOW_CURSOR)
                self.request.sendall(MOVE_CURSOR % (24, 1))
                self.request.sendall(CLEAR_FROM_CURSOR_TO_END_OF_SCREEN)
            except OSError as e:
                print(self.client_address, e)
            try:
                self.request.shutdown(socket.SHUT_RDWR)
            except OSError as e:
                print(self.client_address, e)
            break

    def handle(self) -> None:
        send_queue_thread = threading.Thread(target=self._send_queue_thread)
        send_queue_thread.start()

        try:
            self.server.clients.add(self)
            self.send_queue.put(CLEAR_SCREEN)

            while True:
                with self.server.lock:
                    self.render()

                command = self._receive_bytes(10)
                if command is None:
                    break
                if self.view.handle_key_press(command):
                    break

        except OSError as e:
            print(self.client_address, e)

        finally:
            self.send_queue.put(None)
            send_queue_thread.join()  # Don't close until stuff is sent


server = Server(12345)
print("Listening on port 12345...")
server.serve_forever()
