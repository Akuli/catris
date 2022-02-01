from __future__ import annotations
import copy
import time
import contextlib
import socketserver
import threading
import socket
import random
from typing import Iterator


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

# If you want to play with more than 4 players, use bigger terminal
PLAYER_COLORS = {31, 32, 33, 34, 35, 36, 37}  # foreground colors


def _name_to_string(name_bytes: bytes) -> str:
    return "".join(
        c for c in name_bytes.decode("utf-8", errors="replace") if c.isprintable()
    )


# TODO: this class is too long
class TetrisClient(socketserver.BaseRequestHandler):
    server: TetrisServer
    request: socket.socket

    def setup(self) -> None:
        print(self.client_address, "New connection")
        self.last_displayed_lines: list[bytes] | None = None
        self.disconnecting = False
        self._rotate_counter_clockwise = False

    def new_block(self) -> None:
        self.moving_block_letter = random.choice(list(BLOCK_SHAPES.keys()))

        index = self.server.playing_clients.index(self)
        self.moving_block_x = WIDTH_PER_PLAYER // 2 + index * WIDTH_PER_PLAYER
        self.moving_block_y = -1
        self._rotation = 0

    def get_moving_block_coords(
        self, rotation: int | None = None
    ) -> list[tuple[int, int]]:
        if rotation is None:
            rotation = self._rotation

        result = []

        for rel_x, rel_y in BLOCK_SHAPES[self.moving_block_letter]:
            for iteration in range(rotation % 4):
                rel_x, rel_y = -rel_y, rel_x
            result.append((self.moving_block_x + rel_x, self.moving_block_y + rel_y))

        return result

    def render_game(self, *, blink: list[int] = [], blink_color: int = 0) -> None:
        score_y = 5
        rotate_dir_y = 6
        game_over_y = 8  # Leave a visible gap above, to highlight game over text

        if self.server.playing_clients:
            header_line = b"o"
            name_line = b" "
            for client in self.server.playing_clients:
                header_line += COLOR % client.color
                name_line += COLOR % client.color

                if client == self:
                    header_line += b"==" * WIDTH_PER_PLAYER
                else:
                    header_line += b"--" * WIDTH_PER_PLAYER
                name_line += client.name.center(2 * WIDTH_PER_PLAYER).encode("utf-8")

            name_line += COLOR % 0
            header_line += COLOR % 0
            header_line += b"o"

            lines = [name_line, header_line]

            for blink_y, row in enumerate(self.server.get_square_colors()):
                line = b"|"
                for row_color in row:
                    actual_color = blink_color if blink_y in blink else row_color
                    if actual_color is None:
                        line += b"  "
                    else:
                        line += COLOR % actual_color
                        line += b"  "
                        line += COLOR % 0
                line += b"|"
                lines.append(line)

            lines.append(b"o" + b"--" * self.server.get_width() + b"o")

            lines[score_y] += f"  Score: {self.server.score}".encode("ascii")
            if self._rotate_counter_clockwise:
                lines[rotate_dir_y] += b"  Counter-clockwise"
            if self in self.server.game_over_clients:
                lines[game_over_y] += b"  GAME OVER"

        else:
            # Game over for everyone, keep displaying status when it wasn't over yet
            assert self.last_displayed_lines is not None
            lines = self.last_displayed_lines.copy()
            # ... but with an additional game over message, even for the last player
            if not lines[game_over_y].endswith(b"GAME OVER"):
                lines[game_over_y] += b"  GAME OVER"

        if self.last_displayed_lines is None:
            self.last_displayed_lines = [b""] * len(lines)
        assert len(lines) == len(self.last_displayed_lines)

        # Send it all at once, so that hopefully cursor won't be in a
        # temporary place for long times, even if internet is slow
        to_send = b""

        for y, (old_line, new_line) in enumerate(zip(self.last_displayed_lines, lines)):
            if old_line != new_line:
                to_send += MOVE_CURSOR % (y + 1, 1)
                to_send += new_line
                to_send += CLEAR_TO_END_OF_LINE
        self.last_displayed_lines = lines.copy()

        # Wipe bottom of terminal and leave cursor there.
        # This way, if user types something, it will be wiped.
        to_send += MOVE_CURSOR % (24, 1)
        to_send += CLEAR_TO_END_OF_LINE

        self.request.sendall(to_send)

    def _moving_block_coords_are_possible(self, coords: list[tuple[int, int]]) -> bool:
        other_blocks = self.server.get_square_colors(exclude_player=self)
        return all(
            x in range(self.server.get_width())
            and y < HEIGHT
            and (y < 0 or other_blocks[y][x] is None)
            for x, y in coords
        )

    def move_if_possible(self, dx: int, dy: int) -> bool:
        if not self._moving_block_coords_are_possible(
            [(x + dx, y + dy) for x, y in self.get_moving_block_coords()]
        ):
            return False

        self.moving_block_x += dx
        self.moving_block_y += dy
        return True

    def _move_block_down_all_the_way(self) -> None:
        while self.move_if_possible(dx=0, dy=1):
            pass

    # FIXME: What happens if moving blocks start to overlap because of this?
    def keep_moving_block_between_walls(self) -> None:
        left = min(x for x, y in self.get_moving_block_coords())
        right = max(x for x, y in self.get_moving_block_coords()) + 1
        if left < 0:
            self.moving_block_x += abs(left)
        elif right > self.server.get_width():
            self.moving_block_x -= right - self.server.get_width()

    def _rotate(self) -> None:
        if self.moving_block_letter == "O":
            return

        if self._rotate_counter_clockwise:
            new_rotation = self._rotation - 1
        else:
            new_rotation = self._rotation + 1

        if self.moving_block_letter in "ISZ":
            new_rotation %= 2

        new_coords = self.get_moving_block_coords(rotation=new_rotation)
        if self._moving_block_coords_are_possible(new_coords):
            self._rotation = new_rotation

    def handle_disconnect(self, reason: str) -> None:
        if not self.disconnecting:
            self.disconnecting = True
            print(self.client_address, "Disconnect:", reason)
            try:
                self.request.sendall(SHOW_CURSOR)
                self.request.sendall(MOVE_CURSOR % (24, 1))
                self.request.sendall(CLEAR_FROM_CURSOR_TO_END_OF_SCREEN)
                self.request.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass

    def _receive_bytes(self, maxsize: int) -> bytes | None:
        try:
            result = self.request.recv(maxsize)
        except OSError as e:
            self.handle_disconnect(str(e))
            return None

        # Checking ESC key here is a bad idea.
        # Arrow keys are sent as ESC + other bytes, and recv() can sometimes
        # return only some of the sent data.
        if result in {CONTROL_C, CONTROL_D, CONTROL_Q, b""}:
            self.handle_disconnect(f"received {result!r}")
            return None

        return result

    # returns error message, or None for success
    def _start_playing(self, name: str) -> str | None:
        if not name:
            return "Please write a name before pressing Enter."

        with self.server.state_change():
            available_colors = PLAYER_COLORS - {
                c.color for c in self.server.playing_clients
            }
            if not available_colors:
                return "Server is full. Please try again later."

            if name in (c.name for c in self.server.playing_clients):
                return "This name in use. Try a different name."

            self.name: str = name
            self.color: int = min(available_colors)

            self.server.playing_clients.append(self)
            for row in self.server.landed_blocks:
                row.extend([None] * WIDTH_PER_PLAYER)
            self.new_block()

        return None

    def _show_prompt_error(self, error: str) -> None:
        self.request.sendall(MOVE_CURSOR % (8, 2))
        self.request.sendall(COLOR % 31)  # red
        self.request.sendall(error.encode("utf-8"))
        self.request.sendall(COLOR % 0)
        self.request.sendall(CLEAR_TO_END_OF_LINE)

    def _prompt_name(self) -> bool:
        self.request.sendall(CLEAR_SCREEN)
        self.request.sendall(MOVE_CURSOR % (5, 5))

        message = f"Name (max {2*WIDTH_PER_PLAYER} letters): "
        self.request.sendall(message.encode("ascii"))
        name_start_pos = (5, 5 + len(message))

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
                error = self._start_playing(
                    _name_to_string(name)[: 2 * WIDTH_PER_PLAYER]
                )
                if error is None:
                    return True
                self._show_prompt_error(error)
            elif byte == BACKSPACE:
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
            command = self._receive_bytes(10)
            if command is None:
                # User disconnected, stop waiting for timeout in handle()
                # TODO: I don't like how this looks right now
                with self.server.state_change():
                    pass
                break

            with self.server.state_change():
                if command in (b"A", b"a", LEFT_ARROW_KEY):
                    self.move_if_possible(dx=-1, dy=0)
                if command in (b"D", b"d", RIGHT_ARROW_KEY):
                    self.move_if_possible(dx=1, dy=0)
                if command in (b"W", b"w", UP_ARROW_KEY, b"\r"):
                    self._rotate()
                if command in (b"S", b"s", DOWN_ARROW_KEY, b" "):
                    self._move_block_down_all_the_way()
                if command in (b"R", b"r"):
                    self._rotate_counter_clockwise = not self._rotate_counter_clockwise

    def end_game(self) -> None:
        i = self.server.playing_clients.index(self)
        del self.server.playing_clients[i]
        self.server.game_over_clients.append(self)

        for row in self.server.landed_blocks:
            del row[i * WIDTH_PER_PLAYER : (i + 1) * WIDTH_PER_PLAYER]

        for client_on_right in self.server.playing_clients[i:]:
            client_on_right.moving_block_x -= WIDTH_PER_PLAYER

        for other_client in self.server.playing_clients:
            other_client.keep_moving_block_between_walls()

    def handle(self) -> None:
        try:
            if not self._prompt_name():
                return
        except OSError as e:
            self.handle_disconnect(str(e))
            return

        print(
            self.client_address,
            f"starting game: name {self.name!r}, color {self.color}",
        )
        threading.Thread(target=self._input_thread).start()

        try:
            self.request.sendall(CLEAR_SCREEN)
            self.request.sendall(HIDE_CURSOR)

            while True:
                self.render_game()
                if self.disconnecting:
                    break

                self.server.wait_for_update()
                if self.disconnecting:
                    break

        except OSError as e:
            self.handle_disconnect(str(e))

        finally:
            with self.server.state_change():
                if self in self.server.playing_clients:
                    self.end_game()
                self.server.game_over_clients.remove(self)


class TetrisServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True

    def __init__(self, port: int):
        super().__init__(("", port), TetrisClient)
        self._needs_update = threading.Condition()

        # TODO: I don't like how this has to be RLock just for the blinking feature
        self._lock = threading.RLock()
        self.playing_clients: list[TetrisClient] = []
        self.game_over_clients: list[TetrisClient] = []
        self.landed_blocks: list[list[int | None]] = [[] for y in range(HEIGHT)]
        self.score = 0

        # TODO: relying on _threads isn't great
        if self._threads is None:  # type: ignore
            self._threads = []
        t = threading.Thread(target=self._move_blocks_down_thread)
        t.start()
        self._threads.append(t)

    # Must be called from within state_change()
    def _clear_full_lines(self) -> None:
        full_lines = [
            y for y, row in enumerate(self.landed_blocks) if row and None not in row
        ]
        if full_lines:
            for color in [47, 0, 47, 0]:
                # TODO: add lock for rendering?
                for client in (self.playing_clients + self.game_over_clients).copy():
                    try:
                        client.render_game(blink=full_lines, blink_color=color)
                    except OSError as e:
                        # FIXME: should call end_game()?
                        client.handle_disconnect(str(e))
                time.sleep(0.1)

        if self.playing_clients:
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
            self.score += len(self.playing_clients) * single_player_score
        else:
            # Score resets when no players remain, this is how the games end
            self.score = 0

        self.landed_blocks = [row for row in self.landed_blocks if None in row]
        while len(self.landed_blocks) < HEIGHT:
            self.landed_blocks.insert(0, [None] * self.get_width())

    def _move_blocks_down_thread(self) -> None:
        next_time = time.monotonic()
        while True:
            with self.state_change():
                # Blocks of different users can be on each other's way, but should
                # still be moved if the bottommost block will move.
                #
                # Solution: repeatedly try to move each one, and stop when nothing moves.
                todo = set(self.playing_clients)
                while True:
                    something_moved = False
                    for client in todo.copy():
                        moved = client.move_if_possible(dx=0, dy=1)
                        if moved:
                            something_moved = True
                            todo.remove(client)
                    if not something_moved:
                        break

                # Blocks of remaining clients can't be moved, even if other clients
                # move first. This is because they hit bottom of game or landed blocks.
                for client in todo:
                    coords = client.get_moving_block_coords()
                    if any(y < 0 for x, y in coords):
                        client.end_game()
                    else:
                        for x, y in coords:
                            self.landed_blocks[y][x] = BLOCK_COLORS[
                                client.moving_block_letter
                            ]
                        client.new_block()

                self._clear_full_lines()

            # Take in account the time it takes to do everything above.
            # Someone's slow-ish internet shouldn't slow the gameplay for everyone.
            next_time += 0.5 / (1 + self.score / 2000)
            delay = next_time - time.monotonic()
            if delay > 0:
                time.sleep(delay)

    def get_width(self) -> int:
        return WIDTH_PER_PLAYER * len(self.playing_clients)

    @contextlib.contextmanager
    def state_change(self) -> Iterator[None]:
        with self._lock:
            yield
        with self._needs_update:
            self._needs_update.notify_all()

    def wait_for_update(self, timeout: float | None = None) -> bool:
        with self._needs_update:
            return self._needs_update.wait(timeout=timeout)

    def get_square_colors(
        self, *, exclude_player: TetrisClient | None = None
    ) -> list[list[int | None]]:
        result = copy.deepcopy(self.landed_blocks)

        with self._lock:
            for client in self.playing_clients:
                for x, y in client.get_moving_block_coords():
                    if y >= 0 and client != exclude_player:
                        result[y][x] = BLOCK_COLORS[client.moving_block_letter]

        return result


server = TetrisServer(12345)
print("Listening on port 12345...")
server.serve_forever()
