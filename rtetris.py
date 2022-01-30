from __future__ import annotations
import time
import contextlib
import socketserver
import threading
from typing import Iterator


# https://en.wikipedia.org/wiki/ANSI_escape_code
ESC = b'\x1b'
CSI = ESC + b'['
CLEAR_SCREEN = CSI + b'2J'
MOVE_CURSOR = CSI + b'%d;%dH'
COLOR = CSI + b'1;%dm'


WIDTH = 10
HEIGHT = 20

BLOCK_SHAPES = {
    "L": [
        (0, -1),
        (0, 0),
        (0, 1), (1, 1),
    ]
}
BLOCK_COLORS = {
    "L": 43,
}


class TetrisClient(socketserver.BaseRequestHandler):
    server: TetrisServer

    def setup(self) -> None:
        self.last_displayed_lines = [b""] * (HEIGHT+2)
        self.moving_block_shape = "L"
        self.moving_block_location = (4, 3)

    def render_game(self) -> None:
        lines = []
        lines.append(b'o' + b'--'*WIDTH + b'o')
        for y, row in enumerate(self.server.get_color_data()):
            lines.append(b'|%s|' % b"".join(
                b"  " if item is None else (
                    (COLOR % BLOCK_COLORS[item]) + b"  " + (COLOR % 0)
                )
                for item in row
            ))
        lines.append(b'o' + b'--'*WIDTH + b'o')

        assert len(lines) == HEIGHT+2
        for y, (old_line, new_line) in enumerate(zip(self.last_displayed_lines, lines)):
            if old_line != new_line:
                self.request.sendall(MOVE_CURSOR % (y+1, 1))
                self.request.sendall(new_line)

        self.last_displayed_lines = lines.copy()

    def _move_block_down(self) -> None:
        x, y = self.moving_block_location
        y += 1
        self.moving_block_location = (x, y)

    def handle(self) -> None:
        with self.server.state_change():
            self.server.clients.add(self)

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
                self.server.clients.remove(self)


class TetrisServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True

    def __init__(self, port: int):
        super().__init__(('', port), TetrisClient)
        self._needs_update = threading.Condition()
        self.clients: set[TetrisClient] = set()
        self._lock = threading.Lock()

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
        result: list[list[str | None]] = [[None] * WIDTH for y in range(HEIGHT)]

        with self._lock:
            for client in self.clients:
                for rel_x, rel_y in BLOCK_SHAPES[client.moving_block_shape]:
                    base_x, base_y = client.moving_block_location
                    result[base_y + rel_y][base_x + rel_x] = client.moving_block_shape

        return result


server = TetrisServer(12345)
server.serve_forever()
