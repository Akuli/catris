from __future__ import annotations
import copy
import time
import contextlib
import socketserver
import threading
import random
from typing import Iterator


# TODO:
#   - mark current player
#   - moving blocks: arrow keys / wasd / mouse wheel
#   - ask players names when joining, and display them below game
#   - better game over handling
#   - spectating: after your game over, you can still watch others play


# https://en.wikipedia.org/wiki/ANSI_escape_code
ESC = b"\x1b"
CSI = ESC + b"["
CLEAR_SCREEN = CSI + b"2J"
MOVE_CURSOR = CSI + b"%d;%dH"
COLOR = CSI + b"1;%dm"
CLEAR_TO_END_OF_LINE = CSI + b"0K"


# Width varies as people join/leave
HEIGHT = 10
WIDTH_PER_PLAYER = 7

SHAPE_LETTERS = "LIJOTZS"
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


class TetrisClient(socketserver.BaseRequestHandler):
    server: TetrisServer

    def setup(self) -> None:
        self.last_displayed_lines = [b""] * (HEIGHT + 2)

    def new_block(self) -> None:
        self.moving_block_shape_letter = random.choice(SHAPE_LETTERS)

        index = self.server.clients.index(self)
        self._moving_block_location = (
            WIDTH_PER_PLAYER // 2 + index * WIDTH_PER_PLAYER,
            -max(y + 1 for x, y in BLOCK_SHAPES[self.moving_block_shape_letter]),
        )
        print("***(1)", self._moving_block_location, self.moving_block_shape_letter)
        print("***(2)", list(self.get_moving_block_coords()))

    def get_moving_block_coords(self) -> Iterator[tuple[int, int]]:
        base_x, base_y = self._moving_block_location
        for rel_x, rel_y in BLOCK_SHAPES[self.moving_block_shape_letter]:
            yield (base_x + rel_x, base_y + rel_y)

    def render_game(self) -> None:
        lines = []
        lines.append(b"o" + b"--" * self.server.get_width() + b"o")
        for y, row in enumerate(self.server.get_color_data()):
            lines.append(
                b"|%s|"
                % b"".join(
                    b"  "
                    if item is None
                    else ((COLOR % BLOCK_COLORS[item]) + b"  " + (COLOR % 0))
                    for item in row
                )
            )
        lines.append(b"o" + b"--" * self.server.get_width() + b"o")

        assert len(lines) == HEIGHT + 2
        for y, (old_line, new_line) in enumerate(zip(self.last_displayed_lines, lines)):
            if old_line != new_line:
                self.request.sendall(MOVE_CURSOR % (y + 1, 1))
                self.request.sendall(new_line)
                self.request.sendall(CLEAR_TO_END_OF_LINE)

        self.last_displayed_lines = lines.copy()

    def _move_block_down(self) -> None:
        if any(
            y + 1 >= HEIGHT
            or (y + 1 >= 0 and self.server.landed_blocks[y + 1][x] is not None)
            for x, y in self.get_moving_block_coords()
        ):
            for x, y in self.get_moving_block_coords():
                if y < 0:
                    raise RuntimeError("game over")
                self.server.landed_blocks[y][x] = self.moving_block_shape_letter
            self.new_block()
        else:
            x, y = self._moving_block_location
            y += 1
            self._moving_block_location = (x, y)

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

    def handle(self) -> None:
        with self.server.state_change():
            self.server.clients.append(self)
            for row in self.server.landed_blocks:
                row.extend([None] * WIDTH_PER_PLAYER)
            self.new_block()
            print("Moving block coords", list(self.get_moving_block_coords()))

        try:
            self.request.sendall(CLEAR_SCREEN)

            next_move = time.monotonic()
            while True:
                timeout = next_move - time.monotonic()
                if timeout < 0 or not self.server.wait_for_update(timeout=timeout):
                    with self.server.state_change():
                        self._move_block_down()
                    next_move += 0.5
                self.render_game()
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

        self._lock = threading.Lock()
        self.clients: list[TetrisClient] = []
        self.landed_blocks: list[list[str | None]] = [[] for y in range(HEIGHT)]

    def get_width(self) -> int:
        return WIDTH_PER_PLAYER * len(self.clients)

    @contextlib.contextmanager
    def state_change(self) -> Iterator[None]:
        with self._lock:
            yield
        with self._needs_update:
            self._needs_update.notify_all()

    def wait_for_update(self, timeout: float | None = None) -> bool:
        with self._needs_update:
            return self._needs_update.wait(timeout=timeout)

    def get_color_data(self) -> list[list[str | None]]:
        result = copy.deepcopy(self.landed_blocks)

        with self._lock:
            for client in self.clients:
                for x, y in client.get_moving_block_coords():
                    if y >= 0:
                        result[y][x] = client.moving_block_shape_letter

        return result


server = TetrisServer(12345)
server.serve_forever()
