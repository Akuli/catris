from __future__ import annotations
import copy
import time
import contextlib
import socketserver
import threading
import socket
import random
import queue
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


class GameState:
    def __init__(self) -> None:
        self.score = 0
        self.names: list[str] = []
        self._moving_blocks: dict[str, MovingBlock] = {}
        self._landed_blocks: list[list[int | None]] = [[] for y in range(HEIGHT)]

    def get_width(self) -> int:
        assert len(self.names) == len(self._moving_blocks)
        return WIDTH_PER_PLAYER * len(self.names)

    def is_valid(self) -> bool:
        seen = set()

        for y, row in enumerate(self._landed_blocks):
            for x, color in enumerate(row):
                if color is not None:
                    seen.add((x, y))

        for name, block in self._moving_blocks.items():
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
            self._landed_blocks[y] = [color] * len(self._landed_blocks[y])

    def clear_lines(self, full_lines: list[int]) -> None:
        if self._moving_blocks:
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
            self.score += len(self._moving_blocks) * single_player_score
        else:
            # Score resets when no players remain, this is how the games end
            self.score = 0

        self._landed_blocks = [
            row for y, row in enumerate(self._landed_blocks) if y not in full_lines
        ]
        while len(self._landed_blocks) < HEIGHT:
            self._landed_blocks.insert(0, [None] * self.get_width())

    def get_square_colors(self) -> list[list[int | None]]:
        assert self.is_valid()
        result = copy.deepcopy(self._landed_blocks)
        for moving_block in self._moving_blocks.values():
            for x, y in moving_block.get_coords():
                if y >= 0:
                    result[y][x] = BLOCK_COLORS[moving_block.shape_letter]
        return result

    def move_if_possible(self, name: str, dx: int, dy: int) -> bool:
        assert self.is_valid()
        self._moving_blocks[name].center_x += dx
        self._moving_blocks[name].center_y += dy
        if not self.is_valid():
            self._moving_blocks[name].center_x -= dx
            self._moving_blocks[name].center_y -= dy
            return False
        return True

    def move_down_all_the_way(self, name: str) -> None:
        while self.move_if_possible(name, dx=0, dy=1):
            pass

    def rotate(self, name: str, counter_clockwise: bool) -> None:
        block = self._moving_blocks[name]
        if block.shape_letter == "O":
            return

        old_rotation = block.rotation
        if counter_clockwise:
            new_rotation = old_rotation - 1
        else:
            new_rotation = old_rotation + 1

        if block.shape_letter in "ISZ":
            new_rotation %= 2

        assert self.is_valid()
        block.rotation = new_rotation
        if not self.is_valid():
            block.rotation = old_rotation

    def add_player(self, name: str) -> None:
        assert name not in self.names
        self.names.append(name)
        for row in self._landed_blocks:
            row.extend([None] * WIDTH_PER_PLAYER)
        self._moving_blocks[name] = MovingBlock(self.names.index(name))

    def remove_player(self, name: str) -> None:
        assert self.is_valid()

        x_start = self.names.index(name) * WIDTH_PER_PLAYER
        x_end = x_start + WIDTH_PER_PLAYER

        self.names.remove(name)
        del self._moving_blocks[name]
        for row in self._landed_blocks:
            del row[x_start:x_end]

        for moving_block in self._moving_blocks.values():
            if moving_block.center_x in range(x_start, x_end):
                moving_block.center_x = x_start
            elif moving_block.center_x >= x_end:
                moving_block.center_x -= WIDTH_PER_PLAYER

            left = min(x for x, y in moving_block.get_coords())
            right = max(x + 1 for x, y in moving_block.get_coords())
            if left < 0:
                moving_block.center_x += abs(left)
            elif right > self.get_width():
                moving_block.center_x -= right - self.get_width()

        # FIXME: This is a terrible way to handle overlaps (#10)
        while not self.is_valid():
            random.choice(list(self._moving_blocks.values())).center_y -= 1

    def move_blocks_down(self) -> None:
        # Blocks of different users can be on each other's way, but should
        # still be moved if the bottommost block will move.
        #
        # Solution: repeatedly try to move each one, and stop when nothing moves.
        todo = set(self.names)
        while True:
            something_moved = False
            for name in todo.copy():
                moved = self.move_if_possible(name, dx=0, dy=1)
                if moved:
                    something_moved = True
                    todo.remove(name)
            if not something_moved:
                break

        for name in todo:
            letter = self._moving_blocks[name].shape_letter
            coords = self._moving_blocks[name].get_coords()

            if any(y < 0 for x, y in coords):
                print("Game over for player:", name)
                self.remove_player(name)
            else:
                for x, y in coords:
                    self._landed_blocks[y][x] = BLOCK_COLORS[letter]
                self._moving_blocks[name] = MovingBlock(self.names.index(name))


# If you want to play with more than 4 players, use bigger terminal
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

    # Must hold the lock when calling
    def find_client(self, name: str) -> Client:
        [client] = [c for c in self.clients if c.name == name]
        return client

    @contextlib.contextmanager
    def access_game_state(self, *, render: bool = True) -> Iterator[GameState]:
        with self._lock:
            yield self.__state
            if render:
                for client in self.clients:
                    client.render_game()

    def _move_blocks_down_thread(self) -> None:
        while True:
            with self.access_game_state() as state:
                state.move_blocks_down()
                full_lines = state.find_full_lines()

            if full_lines:
                for color in [47, 0, 47, 0]:
                    with self.access_game_state() as state:
                        state.set_color_of_lines(full_lines, color)
                    time.sleep(0.1)

            with self.access_game_state() as state:
                state.clear_lines(full_lines)
                score = state.score

            time.sleep(0.5 / (1 + score / 2000))


class Client(socketserver.BaseRequestHandler):
    server: Server
    request: socket.socket

    def __repr__(self) -> str:
        return f"<Client name={self.name!r} color={self.color!r}>"

    def setup(self) -> None:
        print(self.client_address, "New connection")
        self.last_displayed_lines: list[bytes] | None = None
        self._send_queue: queue.Queue[bytes | None] = queue.Queue()
        self.rotate_counter_clockwise = False
        self.name: str | None = None
        self.color: int | None = None

    def render_game(self) -> None:
        with self.server.access_game_state(render=False) as state:
            score_y = 5
            rotate_dir_y = 6
            game_over_y = 8  # Leave a visible gap above, to highlight game over text

            if state.names:
                header_line = b"o"
                name_line = b" "
                for name in state.names:
                    color_bytes = COLOR % self.server.find_client(name).color
                    header_line += color_bytes
                    name_line += color_bytes

                    if name == self.name:
                        header_line += b"==" * WIDTH_PER_PLAYER
                    else:
                        header_line += b"--" * WIDTH_PER_PLAYER
                    name_line += name.center(2 * WIDTH_PER_PLAYER).encode("utf-8")

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
                if self.rotate_counter_clockwise:
                    lines[rotate_dir_y] += b"  Counter-clockwise"
                if self.name not in state.names:
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
        if len(name) > 2 * WIDTH_PER_PLAYER:
            return "The name is too long."

        # Must lock while assigning self.name and self.color, so can't get duplicates
        with self.server.access_game_state() as state:
            if name in (c.name for c in self.server.clients):
                return "This name in use. Try a different name."

            available_colors = PLAYER_COLORS - {
                self.server.find_client(name).color for name in state.names
            }
            if not available_colors:
                return "Server is full. Please try again later."

            self.name = name
            self.color = min(available_colors)
            state.add_player(name)
            self.server.clients.add(self)

        return None

    def _show_prompt_error(self, error: str) -> None:
        self._send_queue.put(MOVE_CURSOR % (8, 2))
        self._send_queue.put(COLOR % 31)  # red
        self._send_queue.put(error.encode("utf-8"))
        self._send_queue.put(COLOR % 0)
        self._send_queue.put(CLEAR_TO_END_OF_LINE)

    def _prompt_name(self) -> bool:
        self._send_queue.put(MOVE_CURSOR % (5, 5))

        message = "Name: "
        self._send_queue.put(message.encode("ascii"))
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

            self._send_queue.put(MOVE_CURSOR % name_start_pos)
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
                if self.name in state.names:
                    state.remove_player(self.name)
                self.server.clients.discard(self)

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
            assert self.name is not None
            self._send_queue.put(HIDE_CURSOR)

            print(
                self.client_address,
                f"starting game: name {self.name!r}, color {self.color}",
            )

            while True:
                command = self._receive_bytes(10)
                if command is None:
                    break

                if command in (b"A", b"a", LEFT_ARROW_KEY):
                    with self.server.access_game_state() as state:
                        if self.name in state.names:
                            state.move_if_possible(self.name, dx=-1, dy=0)
                if command in (b"D", b"d", RIGHT_ARROW_KEY):
                    with self.server.access_game_state() as state:
                        if self.name in state.names:
                            state.move_if_possible(self.name, dx=1, dy=0)
                if command in (b"W", b"w", UP_ARROW_KEY, b"\r"):
                    with self.server.access_game_state() as state:
                        if self.name in state.names:
                            state.rotate(self.name, self.rotate_counter_clockwise)
                if command in (b"S", b"s", DOWN_ARROW_KEY, b" "):
                    with self.server.access_game_state() as state:
                        if self.name in state.names:
                            state.move_down_all_the_way(self.name)
                if command in (b"R", b"r"):
                    self.rotate_counter_clockwise = not self.rotate_counter_clockwise
                    self.render_game()

        except OSError as e:
            print(self.client_address, e)

        finally:
            self._send_queue.put(None)
            send_queue_thread.join()  # Don't close until stuff is sent


server = Server(12345)
print("Listening on port 12345...")
server.serve_forever()
