# Usage:
#
#   $ stty raw
#   $ nc localhost 12345
#   $ stty cooked

from __future__ import annotations
import copy
import time
import contextlib
import socketserver
import threading
import socket
import random
from typing import Iterator


# TODO:
#   - better game over handling
#   - spectating: after your game over, you can still watch others play
#   - what to do about overlapping moving blocks of different players?
#   - arrow down probably shouldn't be as damaging to what other people are doing
#   - mouse wheeling


# https://en.wikipedia.org/wiki/ANSI_escape_code
ESC = b"\x1b"
CSI = ESC + b"["
CLEAR_SCREEN = CSI + b"2J"
MOVE_CURSOR = CSI + b"%d;%dH"
COLOR = CSI + b"1;%dm"
CLEAR_TO_END_OF_LINE = CSI + b"0K"

# figured with trial and error
CONTROL_C = b"\x03"
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
PLAYER_COLORS = [31, 32, 33, 34, 35, 36, 37]  # foreground colors


def _name_to_string(name_bytes: bytes) -> str:
    return "".join(
        c for c in name_bytes.decode("utf-8", errors="replace") if c.isprintable()
    )


class TetrisClient(socketserver.BaseRequestHandler):
    server: TetrisServer
    request: socket.socket

    def setup(self) -> None:
        print(self.client_address, "New connection")
        self.last_displayed_lines = [b""] * (HEIGHT + 3)
        self.disconnecting = False

    def new_block(self) -> None:
        self.moving_block_letter = random.choice(list(BLOCK_SHAPES.keys()))

        index = self.server.clients.index(self)
        self._moving_block_location = (
            WIDTH_PER_PLAYER // 2 + index * WIDTH_PER_PLAYER,
            -1,
        )
        self._rotation = 0

    def get_moving_block_coords(
        self, rotation: int | None = None
    ) -> list[tuple[int, int]]:
        if rotation is None:
            rotation = self._rotation

        result = []

        base_x, base_y = self._moving_block_location
        for rel_x, rel_y in BLOCK_SHAPES[self.moving_block_letter]:
            for iteration in range(rotation):
                rel_x, rel_y = -rel_y, rel_x
            result.append((base_x + rel_x, base_y + rel_y))

        return result

    def render_game(self, blink: list[int] = []) -> None:
        header_line = b"o"
        name_line = b" "
        for client in self.server.clients:
            header_line += COLOR % client.color
            if client == self:
                header_line += b"==" * WIDTH_PER_PLAYER
            else:
                header_line += b"--" * WIDTH_PER_PLAYER

            name_line += COLOR % client.color
            name_line += client.name.center(2 * WIDTH_PER_PLAYER).encode("utf-8")

        header_line += COLOR % 0
        header_line += b"o"
        name_line += COLOR % 0

        lines = []
        lines.append(name_line)
        lines.append(header_line)

        for y, row in enumerate(self.server.get_color_data()):
            line = b"|"
            for color in row:
                if y in blink:
                    line += COLOR % 47  # white
                elif color is not None:
                    line += COLOR % color
                line += b"  "
                if color is not None:
                    line += COLOR % 0
            line += b"|"
            lines.append(line)

        lines.append(b"o" + b"--" * self.server.get_width() + b"o")

        assert len(lines) == len(self.last_displayed_lines)
        for y, (old_line, new_line) in enumerate(zip(self.last_displayed_lines, lines)):
            if old_line != new_line:
                self.request.sendall(MOVE_CURSOR % (y + 1, 1))
                self.request.sendall(new_line)
                self.request.sendall(CLEAR_TO_END_OF_LINE)
        self.last_displayed_lines = lines.copy()

        # TODO: not ideal
        self.request.sendall(MOVE_CURSOR % (1, 1))

    def _moving_block_coords_are_possible(self, coords: list[tuple[int, int]]) -> bool:
        return all(
            0 <= x < self.server.get_width()
            and y < HEIGHT
            and (y < 0 or self.server.landed_blocks[y][x] is None)
            for x, y in coords
        )

    def _move_if_possible(self, dx: int, dy: int) -> bool:
        if not self._moving_block_coords_are_possible(
            [(x + dx, y + dy) for x, y in self.get_moving_block_coords()]
        ):
            return False

        x, y = self._moving_block_location
        x += dx
        y += dy
        self._moving_block_location = (x, y)
        return True

    def _move_block_down(self) -> None:
        moved = self._move_if_possible(dx=0, dy=1)
        if not moved:
            for x, y in self.get_moving_block_coords():
                if y < 0:
                    raise RuntimeError("game over")
                self.server.landed_blocks[y][x] = BLOCK_COLORS[self.moving_block_letter]
            self.server.clear_full_lines()
            self.new_block()

    def _move_block_down_all_the_way(self) -> None:
        while self._move_if_possible(dx=0, dy=1):
            pass

    def keep_moving_block_between_walls(self) -> None:
        left = min(x for x, y in self.get_moving_block_coords())
        right = max(x for x, y in self.get_moving_block_coords()) + 1
        if left < 0:
            x, y = self._moving_block_location
            x += abs(left)
            self._moving_block_location = (x, y)
        elif right > self.server.get_width():
            x, y = self._moving_block_location
            x -= right - self.server.get_width()
            self._moving_block_location = (x, y)

    def _rotate(self) -> None:
        if self.moving_block_letter == "O":
            return

        new_rotation = self._rotation + 1
        if self.moving_block_letter in "ISZ":
            new_rotation %= 2

        new_coords = self.get_moving_block_coords(rotation=new_rotation)
        if self._moving_block_coords_are_possible(new_coords):
            self._rotation = new_rotation

    def _receive_bytes(self, maxsize: int) -> bytes | None:
        try:
            result = self.request.recv(maxsize)
        except OSError as e:
            if not self.disconnecting:
                print(self.client_address, "Disconnect:", e)
            self.disconnecting = True
            return None

        if result == CONTROL_C or not result:
            if not self.disconnecting:
                print(self.client_address, "Disconnect: received", result)
            self.disconnecting = True
            return None

        return result

    def _prompt_name(self) -> str | None:
        self.request.sendall(CLEAR_SCREEN)
        self.request.sendall(MOVE_CURSOR % (5, 5))

        message = f"Name (max {2*WIDTH_PER_PLAYER} letters): ".encode("ascii")
        self.request.sendall(message)
        name_start_pos = (5, 5 + len(message))

        name = b""
        while True:
            byte = self._receive_bytes(1)
            if byte is None:
                return None
            if byte == b"\r":
                return _name_to_string(name)[: 2 * WIDTH_PER_PLAYER]

            if byte == b"\n":
                self.request.sendall(MOVE_CURSOR % (8, 2))
                self.request.sendall(COLOR % 31)  # red
                self.request.sendall(
                    b"Your terminal doesn't seem to be in raw mode. Run 'stty raw' and try again."
                )
                self.request.sendall(COLOR % 0)

            if byte == BACKSPACE:
                # Don't just delete last byte, so that non-ascii can be erased
                # with a single backspace press
                name = _name_to_string(name)[:-1].encode("utf-8")
            else:
                name += byte

            self.request.sendall(MOVE_CURSOR % name_start_pos)
            # Send name as it will show up to other users
            self.request.sendall(_name_to_string(name).encode("utf-8"))
            self.request.sendall(CLEAR_TO_END_OF_LINE)

    def _input_thread(self) -> None:
        while True:
            chunk = self._receive_bytes(10)
            if chunk is None:
                # User disconnected, stop waiting for timeout in handle()
                with self.server.state_change():
                    pass
                break

            with self.server.state_change():
                if chunk in (b"A", b"a", LEFT_ARROW_KEY):
                    self._move_if_possible(dx=-1, dy=0)
                if chunk in (b"D", b"d", RIGHT_ARROW_KEY):
                    self._move_if_possible(dx=1, dy=0)
                if chunk in (b"W", b"w", UP_ARROW_KEY, b"\n"):
                    self._rotate()
                if chunk in (b"S", b"s", DOWN_ARROW_KEY, b" "):
                    self._move_block_down_all_the_way()

    def handle(self) -> None:
        name = self._prompt_name()
        if name is None:
            return
        self.name = name
        print(self.client_address, "entered name:", self.name)

        with self.server.state_change():
            available_colors = PLAYER_COLORS.copy()
            for client in self.server.clients:
                available_colors.remove(client.color)
            self.color: int = available_colors[0]

            self.server.clients.append(self)
            for row in self.server.landed_blocks:
                row.extend([None] * WIDTH_PER_PLAYER)
            self.new_block()

        try:
            threading.Thread(target=self._input_thread).start()

            self.request.sendall(CLEAR_SCREEN)

            next_move = time.monotonic()
            while True:
                timeout = next_move - time.monotonic()
                if timeout < 0 or not self.server.wait_for_update(timeout=timeout):
                    with self.server.state_change():
                        self._move_block_down()
                    next_move += 0.5
                if self.disconnecting:
                    break
                self.render_game()
        except OSError as e:
            if not self.disconnecting:
                print(self.client_address, "Disconnect:", e)
                self.disconnecting = True
        finally:
            with self.server.state_change():
                i = self.server.clients.index(self)
                del self.server.clients[i]

                for row in self.server.landed_blocks:
                    del row[i * WIDTH_PER_PLAYER : (i + 1) * WIDTH_PER_PLAYER]
                for other_client in self.server.clients:
                    other_client.keep_moving_block_between_walls()


class TetrisServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True

    def __init__(self, port: int):
        super().__init__(("", port), TetrisClient)
        self._needs_update = threading.Condition()

        # TODO: I don't like how this has to be RLock just for the blinking feature
        self._lock = threading.RLock()
        self.clients: list[TetrisClient] = []
        self.landed_blocks: list[list[int | None]] = [[] for y in range(HEIGHT)]

    def get_width(self) -> int:
        return WIDTH_PER_PLAYER * len(self.clients)

    @contextlib.contextmanager
    def state_change(self) -> Iterator[None]:
        with self._lock:
            yield
        with self._needs_update:
            self._needs_update.notify_all()

    # Assumes the lock is held
    def clear_full_lines(self) -> None:
        full_lines = [y for y, row in enumerate(self.landed_blocks) if None not in row]
        if full_lines:
            empty_list: list[int] = []  # mypy sucks?
            for blink in [full_lines, empty_list, full_lines, empty_list]:
                # TODO: add lock for rendering?
                for client in self.clients:
                    client.render_game(blink)
                time.sleep(0.1)

        self.landed_blocks = [row for row in self.landed_blocks if None in row]
        while len(self.landed_blocks) < HEIGHT:
            self.landed_blocks.insert(0, [None] * self.get_width())

    def wait_for_update(self, timeout: float | None = None) -> bool:
        with self._needs_update:
            return self._needs_update.wait(timeout=timeout)

    def get_color_data(self) -> list[list[int | None]]:
        result = copy.deepcopy(self.landed_blocks)

        with self._lock:
            for client in self.clients:
                for x, y in client.get_moving_block_coords():
                    if y >= 0:
                        result[y][x] = BLOCK_COLORS[client.moving_block_letter]

        return result


server = TetrisServer(12345)
server.serve_forever()
