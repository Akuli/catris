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
        if self.game_is_over() or not client_currently_connected:
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
        # It's possible to get more than 4 lines cleared if a player leaves.
        # Don't reward that too much, it's not a good thing if players mess up.
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

        # It's more difficult to get full lines with more players, so reward
        self.score += len(self.players) * single_player_score

        self._landed_blocks = [
            row for y, row in enumerate(self._landed_blocks) if y not in full_lines
        ]
        while len(self._landed_blocks) < HEIGHT:
            self._landed_blocks.insert(0, [None] * self.get_width())

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
    def add_player(self, name: str) -> Player | None:
        game_over = self.game_is_over()

        # Name can exist already, if player quits and comes back
        for player in self.players:
            if player.name == name:
                break
        else:
            try:
                color = min(PLAYER_COLORS - {p.color for p in self.players})
            except ValueError:
                # all colors used
                return None

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


def _name_to_string(name_bytes: bytes) -> str:
    return "".join(
        c for c in name_bytes.decode("utf-8", errors="replace") if c.isprintable()
    )


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True

    def __init__(self, port: int):
        super().__init__(("", port), Client)

        # RLock because state usage triggers rendering, which uses state
        self._lock = threading.RLock()
        self.__state = GameState()  # Access only with access_game_state()
        self.clients: set[Client] = set()  # This too is locked with the same lock

        threading.Thread(target=self._move_blocks_down_thread).start()

    @contextlib.contextmanager
    def access_game_state(self, *, render: bool = True) -> Iterator[GameState]:
        with self._lock:
            yield self.__state
            if render:
                for client in self.clients:
                    client.render_game()

    def _countdown(self, player: Player) -> None:
        while True:
            time.sleep(1)
            with self.access_game_state() as state:
                assert isinstance(player.moving_block_or_wait_counter, int)
                player.moving_block_or_wait_counter -= 1
                if player.moving_block_or_wait_counter == 0:
                    client_currently_connected = any(
                        c.player == player for c in self.clients
                    )
                    state.end_waiting(player, client_currently_connected)
                    return

    def _move_blocks_down_thread(self) -> None:
        while True:
            with self.access_game_state() as state:
                needs_wait_counter = state.move_blocks_down()
                full_lines = state.find_full_lines()

            for player in needs_wait_counter:
                threading.Thread(target=self._countdown, args=[player]).start()

            if full_lines:
                for color in [47, 0, 47, 0]:
                    with self.access_game_state() as state:
                        state.set_color_of_lines(full_lines, color)
                    time.sleep(0.1)
                with self.access_game_state() as state:
                    state.clear_lines(full_lines)

            with self.access_game_state(render=False) as state:
                score = state.score

            time.sleep(0.5 / (1 + score / 1000))


class Client(socketserver.BaseRequestHandler):
    server: Server
    request: socket.socket

    def setup(self) -> None:
        print(self.client_address, "New connection")
        self.last_displayed_lines: list[bytes] | None = None
        self._send_queue: queue.Queue[bytes | None] = queue.Queue()

    def render_game(self) -> None:
        assert self.player is not None

        with self.server.access_game_state(render=False) as state:
            score_y = 5
            rotate_dir_y = 6
            game_status_y = 8  # Leave a visible gap above, to highlight game over text

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

            lines[score_y] += f"  Score: {state.score}".encode("ascii")
            if self.player.rotate_counter_clockwise:
                lines[rotate_dir_y] += b"  Counter-clockwise"

            if state.game_is_over():
                lines[game_status_y] += b"  GAME OVER"
            elif isinstance(self.player.moving_block_or_wait_counter, int):
                n = self.player.moving_block_or_wait_counter
                lines[game_status_y] += f"  Please wait: {n}".encode("ascii")

            if self.last_displayed_lines is None:
                self.last_displayed_lines = [b""] * len(lines)

            assert len(lines) == len(self.last_displayed_lines)

            # Send it all at once, so that hopefully cursor won't be in a
            # temporary place for long times, even if internet is slow
            to_send = b""

            for y, (old_line, new_line) in enumerate(
                zip(self.last_displayed_lines, lines)
            ):
                if old_line != new_line:
                    to_send += MOVE_CURSOR % (y + 1, 1)
                    to_send += new_line
                    to_send += CLEAR_TO_END_OF_LINE
            self.last_displayed_lines = lines.copy()

            # Wipe bottom of terminal and leave cursor there.
            # This way, if user types something, it will be wiped.
            to_send += MOVE_CURSOR % (24, 1)
            to_send += CLEAR_TO_END_OF_LINE

            self._send_queue.put(to_send)

    def _receive_bytes(self, maxsize: int) -> bytes | None:
        try:
            result = self.request.recv(maxsize)
        except OSError as e:
            print(self.client_address, e)
            self._send_queue.put(None)
            return None

        # Checking ESC key here is a bad idea.
        # Arrow keys are sent as ESC + other bytes, and recv() can sometimes
        # return only some of the sent data.
        if result in {CONTROL_C, CONTROL_D, CONTROL_Q, b""}:
            self._send_queue.put(None)
            return None

        return result

    # returns error message, or None for success
    def _start_playing(self, name: str) -> str | None:
        if not name:
            return "Please write a name before pressing Enter."
        if len(f"[{name}] {WAIT_TIME}") > 2 * WIDTH_PER_PLAYER:
            return "The name is too long."

        # Must lock while assigning self.name and self.color, so can't get duplicates
        with self.server.access_game_state() as state:
            # Prevent two simultaneous clients with the same name.
            # But it's fine if you leave and then join back with the same name
            if name in (c.player.name for c in self.server.clients):
                return "This name is in use. Try a different name."

            player = state.add_player(name)
            if player is None:
                return "Server is full. Please try again later."

            self.player: Player = player
            self.server.clients.add(self)

        return None

    def _show_prompt_error(self, error: str) -> None:
        self._send_queue.put(MOVE_CURSOR % (15, 2))
        self._send_queue.put(COLOR % 31)  # red
        self._send_queue.put(error.encode("utf-8"))
        self._send_queue.put(COLOR % 0)
        self._send_queue.put(CLEAR_TO_END_OF_LINE)

    def _prompt_name(self) -> bool:
        name_x = 20
        name_y = 10
        name_prompt = "Name: "

        self._send_queue.put(MOVE_CURSOR % (1, 1))
        self._send_queue.put(ASCII_ART.encode("ascii").replace(b"\n", b"\r\n"))
        self._send_queue.put(MOVE_CURSOR % (ASCII_ART.count("\n") + 1, 1))
        self._send_queue.put(b"https://github.com/Akuli/catris".center(80).rstrip())
        self._send_queue.put(MOVE_CURSOR % (name_y, name_x))
        self._send_queue.put(name_prompt.encode("ascii"))

        name = b""
        while True:
            byte = self._receive_bytes(1)
            if byte is None:
                return False
            elif byte == b"\n":
                self._show_prompt_error(
                    "Your terminal doesn't seem to be in raw mode. Run 'stty raw' and try again."
                )
            elif byte == b"\r":
                error = self._start_playing(_name_to_string(name))
                if error is None:
                    return True
                self._show_prompt_error(error)
            elif byte == BACKSPACE:
                # Don't just delete last byte, so that non-ascii can be erased
                # with a single backspace press
                name = _name_to_string(name)[:-1].encode("utf-8")
            else:
                name += byte

            self._send_queue.put(MOVE_CURSOR % (name_y, name_x + len(name_prompt)))
            # Send name as it will show up to other users
            self._send_queue.put(_name_to_string(name).encode("utf-8"))
            self._send_queue.put(CLEAR_TO_END_OF_LINE)

    def _send_queue_thread(self) -> None:
        while True:
            item = self._send_queue.get()
            if item is not None:
                try:
                    self.request.sendall(item)
                    continue
                except OSError as e:
                    print(self.client_address, e)

            with self.server.access_game_state() as state:
                if self in self.server.clients:
                    self.server.clients.remove(self)
                    if isinstance(
                        self.player.moving_block_or_wait_counter, MovingBlock
                    ):
                        self.player.moving_block_or_wait_counter = None

                    if not self.server.clients:
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
            self._send_queue.put(CLEAR_SCREEN)
            if not self._prompt_name():
                return
            self._send_queue.put(HIDE_CURSOR)

            print(
                self.client_address,
                f"starting game, name={self.player.name!r}",
            )

            while True:
                command = self._receive_bytes(10)
                if command is None:
                    break

                if command in (b"A", b"a", LEFT_ARROW_KEY):
                    with self.server.access_game_state() as state:
                        state.move_if_possible(self.player, dx=-1, dy=0)
                elif command in (b"D", b"d", RIGHT_ARROW_KEY):
                    with self.server.access_game_state() as state:
                        state.move_if_possible(self.player, dx=1, dy=0)
                elif command in (b"W", b"w", UP_ARROW_KEY, b"\r"):
                    with self.server.access_game_state() as state:
                        state.rotate(self.player)
                elif command in (b"S", b"s", DOWN_ARROW_KEY, b" "):
                    with self.server.access_game_state() as state:
                        state.move_down_all_the_way(self.player)
                elif command in (b"R", b"r"):
                    self.player.rotate_counter_clockwise = (
                        not self.player.rotate_counter_clockwise
                    )
                    self.render_game()
                else:
                    # Hide the characters that the user typed
                    self.render_game()

        except OSError as e:
            print(self.client_address, e)

        finally:
            self._send_queue.put(None)
            send_queue_thread.join()  # Don't close until stuff is sent


server = Server(12345)
print("Listening on port 12345...")
server.serve_forever()
