from __future__ import annotations
import collections
import time
import socketserver


# https://en.wikipedia.org/wiki/ANSI_escape_code
ESC = b'\x1b'
CSI = ESC + b'['
CLEAR_SCREEN = CSI + b'2J'
MOVE_CURSOR = CSI + b'%d;%dH'


WIDTH = 8
HEIGHT = 20

BLOCK_SHAPES = {
    "L": [
        (0, -1),
        (0, 0),
        (0, 1), (1, 1),
    ]
}


class Game:

    def __init__(self):
        self.moving_block_shape = "L"
        self.moving_block_location = (4, 3)

    def get_color_data(self):
        result = [[None] * WIDTH for y in range(HEIGHT)]

        for rel_x, rel_y in BLOCK_SHAPES[self.moving_block_shape]:
            base_x, base_y = self.moving_block_location
            result[base_y + rel_y][base_x + rel_x] = self.moving_block_shape

        return result


class RequestHandler(socketserver.BaseRequestHandler):
    server: Server

    def __init__(self, *args, **kwargs):
        self.last_displayed_lines = [""] * HEIGHT
        super().__init__(*args, **kwargs)

    def display_lines(self, lines: list[str]) -> None:
        assert len(lines) == HEIGHT
        for y, (old_line, new_line) in enumerate(zip(self.last_displayed_lines, lines)):
            if old_line != new_line:
                self.request.sendall(MOVE_CURSOR % (y+1, 1))
                self.request.sendall(new_line.encode("utf-8"))

        self.last_displayed_lines = lines.copy()

    def handle(self):
        self.request.sendall(CLEAR_SCREEN)

        lines = ["foo", "bar"]
        while len(lines) < HEIGHT:
            lines.append("...")

        while True:
            self.display_lines(lines)
            time.sleep(0.1)
            lines.append(lines.pop(0))


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True

    def __init__(self, port: int):
        super().__init__(('', port), RequestHandler)
        self.game = None


server = Server(12345)
server.serve_forever()
