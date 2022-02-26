# TODO: terminal size check before game starts
from __future__ import annotations
import copy
import dataclasses
import time
import contextlib
import socketserver
import textwrap
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
BACKSPACE = (b"\x08", b"\x7f")  # \x08 on windows
UP_ARROW_KEY = CSI + b"A"
DOWN_ARROW_KEY = CSI + b"B"
RIGHT_ARROW_KEY = CSI + b"C"
LEFT_ARROW_KEY = CSI + b"D"

# IF TRADITIONAL
# Width varies as people join/leave
HEIGHT = 20
WIDTH_PER_PLAYER = 7

# END IF
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

# IF TRADITIONAL
# Limited to 4 players, because must fit on 80x24 terminal
# END IF
# IF RING
# Max 4 players
# END IF
PLAYER_COLORS = {31, 32, 33, 34}

# IF RING
# Longest allowed name will get truncated, that's fine
NAME_MAX_LENGTH = 15

# END IF
# If you mess up, how many seconds should you wait?
WAIT_TIME = 10

# IF TRADITIONAL
# Should be in characters to fit screen, but is actually checked in bytes
NAME_MAX_LENGTH = 2 * WIDTH_PER_PLAYER - len(str(WAIT_TIME)) - len("[] ")
# END IF
# IF RING
# Game size is actually 2*GAME_RADIUS + 1 in each direction.
GAME_RADIUS = 14  # chosen to fit 80 column terminal (windows)
MIDDLE_AREA_RADIUS = 3
MIDDLE_AREA = [
    "o============o",
    "|wwwwwwwwwwww|",
    "|aaaaaadddddd|",
    "|aaaaaadddddd|",
    "|aaaaaadddddd|",
    "|ssssssssssss|",
    "o------------o",
]


def get_middle_area_content(players_by_letter: dict[str, Player]) -> list[bytes]:
    wrapped_names = {}
    colors = {}

    for letter in "wasd":
        widths = [line.count(letter) for line in MIDDLE_AREA if letter in line]

        if letter in players_by_letter:
            colors[letter] = players_by_letter[letter].color
            text = players_by_letter[letter].get_name_string(max_length=sum(widths))
        else:
            colors[letter] = 0
            text = ""

        wrapped = textwrap.wrap(text, min(widths))
        if len(wrapped) > len(widths):
            # We must ignore word boundaries to make it fit
            wrapped = []
            for w in widths:
                wrapped.append(text[:w])
                text = text[w:]
                if not text:
                    break
            assert not text

        lines_to_add = len(widths) - len(wrapped)
        prepend_count = lines_to_add // 2
        append_count = lines_to_add - prepend_count
        wrapped = [""] * prepend_count + wrapped + [""] * append_count

        if letter == "a":
            wrapped = [line.ljust(width) for width, line in zip(widths, wrapped)]
        elif letter == "d":
            wrapped = [line.rjust(width) for width, line in zip(widths, wrapped)]
        else:
            wrapped = [line.center(width) for width, line in zip(widths, wrapped)]

        wrapped_names[letter] = wrapped

    result = []
    for template_line_string in MIDDLE_AREA:
        template_line = template_line_string.encode("ascii")

        # Apply colors to lines surrounding the middle area
        template_line = template_line.replace(
            b"o==", b"o" + (COLOR % colors["w"]) + b"=="
        )
        template_line = template_line.replace(b"==o", b"==" + (COLOR % 0) + b"o")
        template_line = template_line.replace(
            b"o--", b"o" + (COLOR % colors["s"]) + b"--"
        )
        template_line = template_line.replace(b"--o", b"--" + (COLOR % 0) + b"o")
        if template_line.startswith(b"|"):
            template_line = (
                (COLOR % colors["a"]) + b"|" + (COLOR % 0) + template_line[1:]
            )
        if template_line.endswith(b"|"):
            template_line = (
                template_line[:-1] + (COLOR % colors["d"]) + b"|" + (COLOR % 0)
            )

        result_line = b""
        while template_line:
            if template_line[0] in b"wasd":
                letter = template_line[:1].decode("ascii")
                result_line += (
                    (COLOR % colors[letter])
                    + wrapped_names[letter].pop(0).encode("utf-8")
                    + (COLOR % 0)
                )
                template_line = template_line.replace(template_line[:1], b"")
            else:
                result_line += template_line[:1]
                template_line = template_line[1:]
        result.append(result_line)

    return result
# END IF


@dataclasses.dataclass
class HighScore:
    score: int
    duration_sec: float
    players: list[str]

    def get_duration_string(self) -> str:
        seconds = int(self.duration_sec)
        minutes = seconds // 60
        hours = minutes // 60

        if hours:
            return f"{hours}h"
        if minutes:
            return f"{minutes}min"
        return f"{seconds}sec"


class MovingBlock:
# IF TRADITIONAL
    def __init__(self, player_index: int):
# END IF
# IF RING
    def __init__(self, player: Player):
        self.player = player
# END IF
        self.shape_letter = random.choice(list(BLOCK_SHAPES.keys()))
# IF TRADITIONAL
        self.center_x = (WIDTH_PER_PLAYER * player_index) + (WIDTH_PER_PLAYER // 2)
        self.center_y = -1
        self.rotation = 0
# END IF
# IF RING
        self.center_x, self.center_y = player.player_to_world(0, -GAME_RADIUS - 1)

        # Orient initial block so that it always looks the same.
        # Otherwise may create subtle bugs near end of game, where freshly
        # added block overlaps with landed blocks.
        self.rotation = {
            (0, -1): 0,
            (1, 0): 1,
            (0, 1): 2,
            (-1, 0): 3,
        }[(player.direction_x, player.direction_y)]
# END IF

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
# IF RING
    direction_x: int
    direction_y: int
# END IF
    rotate_counter_clockwise: bool = False
    moving_block_or_wait_counter: MovingBlock | int | None = None

# IF RING
    def get_name_string(self, max_length: int) -> str:
        if self.moving_block_or_wait_counter is None:
            format = "[%s]"
        elif isinstance(self.moving_block_or_wait_counter, int):
            format = "[%s] " + str(self.moving_block_or_wait_counter)
        else:
            format = "%s"

        name = self.name
        while True:
            if len(format % name) <= max_length:
                return format % name
            name = name[:-1]

    # Player's view is rotated, so that blocks always appear to fall down.
    def world_to_player(self, x: int, y: int) -> tuple[int, int]:
        return (
            (-self.direction_y * x + self.direction_x * y),
            (-self.direction_x * x - self.direction_y * y),
        )

    def player_to_world(self, x: int, y: int) -> tuple[int, int]:
        return (
            (-self.direction_y * x - self.direction_x * y),
            (self.direction_x * x - self.direction_y * y),
        )

# END IF

class GameState:
    def __init__(self) -> None:
        self.reset()

    def reset(self) -> None:
        self.start_time = time.monotonic_ns()
        self.players: list[Player] = []
# IF TRADITIONAL
        self._landed_blocks: list[list[int | None]] = [[] for y in range(HEIGHT)]
# END IF
        self.score = 0
# IF RING
        self._landed_blocks: dict[tuple[int, int], int | None] = {
            (x, y): None
            for x in range(-GAME_RADIUS, GAME_RADIUS + 1)
            for y in range(-GAME_RADIUS, GAME_RADIUS + 1)
            if max(abs(x), abs(y)) > MIDDLE_AREA_RADIUS
        }
# END IF

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

# IF TRADITIONAL
        index = self.players.index(player)
        x_min = WIDTH_PER_PLAYER * index
        x_max = x_min + WIDTH_PER_PLAYER
        for row in self._landed_blocks:
            row[x_min:x_max] = [None] * WIDTH_PER_PLAYER
        player.moving_block_or_wait_counter = MovingBlock(index)
# END IF
# IF RING
        for x, y in self._landed_blocks.keys():
            # Math magic to check if (x,y) is in the player's triangle-shaped area.
            # Have fun figuring out how it works :)
            dot = x * player.direction_x + y * player.direction_y
            if dot >= 0 and 2 * dot ** 2 >= x * x + y * y:
                self._landed_blocks[x, y] = None
# END IF

# IF TRADITIONAL
    def get_width(self) -> int:
        return WIDTH_PER_PLAYER * len(self.players)
# END IF
# IF RING
        player.moving_block_or_wait_counter = MovingBlock(player)
# END IF

    def _get_moving_blocks(self) -> list[MovingBlock]:
        result = []
        for player in self.players:
            if isinstance(player.moving_block_or_wait_counter, MovingBlock):
                result.append(player.moving_block_or_wait_counter)
        return result

    def is_valid(self) -> bool:
# IF TRADITIONAL
        seen = set()
# END IF
# IF RING
        assert self._landed_blocks.keys() == {
            (x, y)
            for x in range(-GAME_RADIUS, GAME_RADIUS + 1)
            for y in range(-GAME_RADIUS, GAME_RADIUS + 1)
            if max(abs(x), abs(y)) > MIDDLE_AREA_RADIUS
        }
# END IF

# IF TRADITIONAL
        for y, row in enumerate(self._landed_blocks):
            for x, color in enumerate(row):
                if color is not None:
                    seen.add((x, y))
# END IF
# IF RING
        seen = {
            point for point, color in self._landed_blocks.items() if color is not None
        }
# END IF

        for block in self._get_moving_blocks():
# IF TRADITIONAL
            coords = block.get_coords()
            if coords & seen or not all(
                x in range(self.get_width()) and y < HEIGHT for x, y in coords
            ):
                return False
            seen.update(coords)
# END IF
# IF RING
            for x, y in block.get_coords():
                if (x, y) in seen or max(abs(x), abs(y)) <= MIDDLE_AREA_RADIUS:
                    return False
                seen.add((x, y))

                player_x, player_y = block.player.world_to_player(x, y)
                if player_x < -GAME_RADIUS or player_x > GAME_RADIUS or player_y > 0:
                    return False

# END IF
        return True

# IF TRADITIONAL
    def find_full_lines(self) -> list[int]:
# END IF
# IF RING
    def find_full_radiuses(self) -> list[int]:
# END IF
        return [
# IF TRADITIONAL
            y for y, row in enumerate(self._landed_blocks) if row and None not in row
# END IF
# IF RING
            r
            for r in range(MIDDLE_AREA_RADIUS + 1, GAME_RADIUS + 1)
            if not any(
                color is None
                for (x, y), color in self._landed_blocks.items()
                if max(abs(x), abs(y)) == r
            )
# END IF
        ]

# IF TRADITIONAL
    # Between find_full_lines and clear_full_lines, there's a flashing animation.
    # Color can't be None, because then it would be possible to put blocks to a flashing line.
    def set_color_of_lines(self, full_lines: list[int], color: int) -> None:
        for y in full_lines:
            self._landed_blocks[y] = [color] * self.get_width()
# END IF
# IF RING
    # Between find_full_radiuses and clear_full_radiuses, there's a flashing animation.
    # Color can't be None, otherwise it is possible to put blocks into a currently flashing ring.
    def set_color_of_rings(self, full_radiuses: list[int], color: int) -> None:
        for x, y in self._landed_blocks.keys():
            if max(abs(x), abs(y)) in full_radiuses:
                self._landed_blocks[x, y] = color

    def delete_ring(self, r: int) -> None:
        new_landed_blocks = {}
        for (x, y), color in self._landed_blocks.items():
            if color is None:
                continue

            # preserve squares inside the ring
            if max(abs(x), abs(y)) < r:
                new_landed_blocks[x, y] = color

            # delete squares on the ring
            if max(abs(x), abs(y)) == r:
                continue

            # Move towards center. Squares at a diagonal direction from center
            # have abs(x) == abs(y) move in two different directions.
            # Two squares can move into the same place. That's fine.
            move_left = x > 0 and abs(x) >= abs(y)
            move_right = x < 0 and abs(x) >= abs(y)
            move_up = y > 0 and abs(y) >= abs(x)
            move_down = y < 0 and abs(y) >= abs(x)
            if move_left:
                x -= 1
            if move_right:
                x += 1
            if move_up:
                y -= 1
            if move_down:
                y += 1

            new_landed_blocks[x, y] = color

        self._landed_blocks = {
            (x, y): new_landed_blocks.get((x, y), None)
            for x in range(-GAME_RADIUS, GAME_RADIUS + 1)
            for y in range(-GAME_RADIUS, GAME_RADIUS + 1)
        }
# END IF

# IF TRADITIONAL
    def clear_lines(self, full_lines: list[int]) -> None:
        if len(full_lines) == 0:
# END IF
# IF RING
    def clear_rings(self, full_radiuses: list[int]) -> None:
        if len(full_radiuses) == 0:
# END IF
            single_player_score = 0
# IF TRADITIONAL
        elif len(full_lines) == 1:
# END IF
# IF RING
        elif len(full_radiuses) == 1:
# END IF
            single_player_score = 10
# IF TRADITIONAL
        elif len(full_lines) == 2:
# END IF
# IF RING
        elif len(full_radiuses) == 2:
# END IF
            single_player_score = 30
# IF TRADITIONAL
        elif len(full_lines) == 3:
# END IF
# IF RING
        elif len(full_radiuses) == 3:
# END IF
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

# IF TRADITIONAL
        self._landed_blocks = [
            row for y, row in enumerate(self._landed_blocks) if y not in full_lines
        ]
        while len(self._landed_blocks) < HEIGHT:
            self._landed_blocks.insert(0, [None] * self.get_width())
# END IF
# IF RING
        for r in sorted(full_radiuses, reverse=True):
            self.delete_ring(r)
# END IF

# IF TRADITIONAL
        # When landed blocks move down, they can go on top of moving blocks.
# END IF
# IF RING
        # When landed blocks move, they can go on top of moving blocks.
# END IF
        # This is quite rare, but results in invalid state errors.
        # When this happens, just delete the landed block.
        for moving_block in self._get_moving_blocks():
# IF TRADITIONAL
            for x, y in moving_block.get_coords():
                if y >= 0:
                    self._landed_blocks[y][x] = None
# END IF
# IF RING
            for point in moving_block.get_coords():
                if self._landed_blocks.get(point) is not None:
                    self._landed_blocks[point] = None
# END IF
        assert self.is_valid()

# IF TRADITIONAL
    def get_square_colors(self) -> list[list[int | None]]:
# END IF
# IF RING
    def get_square_colors(self) -> dict[tuple[int, int], int | None]:
# END IF
        assert self.is_valid()
# IF TRADITIONAL
        result = copy.deepcopy(self._landed_blocks)
# END IF
# IF RING
        result = self._landed_blocks.copy()
# END IF
        for moving_block in self._get_moving_blocks():
# IF TRADITIONAL
            for x, y in moving_block.get_coords():
                if y >= 0:
                    result[y][x] = BLOCK_COLORS[moving_block.shape_letter]
# END IF
# IF RING
            for point in moving_block.get_coords():
                if point in result:
                    result[point] = BLOCK_COLORS[moving_block.shape_letter]

# END IF
        return result

# IF TRADITIONAL
    def move_if_possible(self, player: Player, dx: int, dy: int) -> bool:
# END IF
# IF RING
    def move_if_possible(
        self, player: Player, dx: int, dy: int, *, in_player_coords: bool
    ) -> bool:
# END IF
        assert self.is_valid()
# IF RING
        if in_player_coords:
            dx, dy = player.player_to_world(dx, dy)

# END IF
        if isinstance(player.moving_block_or_wait_counter, MovingBlock):
            player.moving_block_or_wait_counter.center_x += dx
            player.moving_block_or_wait_counter.center_y += dy
            if self.is_valid():
                return True
            player.moving_block_or_wait_counter.center_x -= dx
            player.moving_block_or_wait_counter.center_y -= dy

        return False

    def move_down_all_the_way(self, player: Player) -> None:
# IF TRADITIONAL
        while self.move_if_possible(player, dx=0, dy=1):
# END IF
# IF RING
        while self.move_if_possible(player, dx=0, dy=1, in_player_coords=True):
# END IF
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
        print(f"{name!r} joins a game with {len(self.players)} existing players")
        if not self.players:
            self.reset()

        game_over = self.game_is_over()

        # Name can exist already, if player quits and comes back
        for player in self.players:
            if player.name.lower() == name.lower():
                # Let's say your caps lock was on accidentally and you type
                # "aKULI" as name when you intended to type "Akuli".
                # If that happens, you can leave the game and join back.
                player.name = name
                break
        else:
# IF RING
            # Add new player
# END IF
            color = min(PLAYER_COLORS - {p.color for p in self.players})
# IF TRADITIONAL
            player = Player(name, color)
# END IF
# IF RING

            used_directions = {(p.direction_x, p.direction_y) for p in self.players}
            opposites_of_used_directions = {(-x, -y) for x, y in used_directions}
            unused_directions = {(0, -1), (0, 1), (-1, 0), (1, 0)} - used_directions

            # If possible, pick a direction opposite to existing player.
            # Choose a direction consistently, for reproducible debugging.
            try:
                dir_x, dir_y = min(opposites_of_used_directions & unused_directions)
            except ValueError:
                dir_x, dir_y = min(unused_directions)

            player = Player(name, color, dir_x, dir_y)
# END IF
            self.players.append(player)
# IF TRADITIONAL
            for row in self._landed_blocks:
                row.extend([None] * WIDTH_PER_PLAYER)
# END IF

        if not game_over and not isinstance(player.moving_block_or_wait_counter, int):
# IF TRADITIONAL
            player.moving_block_or_wait_counter = MovingBlock(
                self.players.index(player)
            )
# END IF
# IF RING
            player.moving_block_or_wait_counter = MovingBlock(player)
# END IF
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
# IF TRADITIONAL
                moved = self.move_if_possible(player, dx=0, dy=1)
# END IF
# IF RING
                moved = self.move_if_possible(player, dx=0, dy=1, in_player_coords=True)
# END IF
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

# IF TRADITIONAL
            if any(y < 0 for x, y in coords):
# END IF
# IF RING
            if any(player.world_to_player(x, y)[1] < -GAME_RADIUS for x, y in coords):
# END IF
                needs_wait_counter.add(player)
            else:
# IF TRADITIONAL
                for x, y in coords:
                    self._landed_blocks[y][x] = BLOCK_COLORS[letter]
                index = self.players.index(player)
                player.moving_block_or_wait_counter = MovingBlock(index)
# END IF
# IF RING
                for point in coords:
                    assert point in self._landed_blocks
                    self._landed_blocks[point] = BLOCK_COLORS[letter]
                player.moving_block_or_wait_counter = MovingBlock(player)
# END IF

        for player in needs_wait_counter:
            player.moving_block_or_wait_counter = WAIT_TIME
        return needs_wait_counter


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True

    def __init__(self, port: int, high_score_file: str):
        super().__init__(("", port), Client)

        # RLock because state usage triggers rendering, which uses state
        self.lock = threading.RLock()
        # All of the below are locked with self.lock:
        self.__state = GameState()  # see access_game_state()
        self.clients: set[Client] = set()

        threading.Thread(target=self._move_blocks_down_thread).start()

        self.high_scores = []
        try:
            with open(high_score_file, "r", encoding="utf-8") as file:
                for line in file:
                    score, duration, *players = line.strip("\n").split("\t")
                    self.high_scores.append(
                        HighScore(
                            score=int(score),
                            duration_sec=float(duration),
                            players=players,
                        )
                    )
        except FileNotFoundError:
            print(high_score_file, "will be created when a game ends")

        self.high_scores.sort(key=(lambda hs: hs.score), reverse=True)
        self._high_score_file = high_score_file

    def _add_high_score(self, hs: HighScore) -> None:
        self.high_scores.append(hs)
        self.high_scores.sort(key=(lambda hs: hs.score), reverse=True)

        try:
            with open(self._high_score_file, "a", encoding="utf-8") as file:
                print(hs.score, hs.duration_sec, *hs.players, file=file, sep="\t")
        except OSError as e:
            print("Writing high score to file failed:", e)

    @contextlib.contextmanager
    def access_game_state(self, *, render: bool = True) -> Iterator[GameState]:
        with self.lock:
            assert self.__state.is_valid()
            assert not self.__state.game_is_over()
            yield self.__state

            assert self.__state.is_valid()
            if self.__state.game_is_over():
                duration_ns = time.monotonic_ns() - self.__state.start_time
                hs = HighScore(
                    score=self.__state.score,
                    duration_sec=duration_ns / (1000 * 1000 * 1000),
                    players=[p.name for p in self.__state.players],
                )
                print("Game over!", hs)
                self.__state.players.clear()

                playing_clients = [
                    c for c in self.clients if isinstance(c.view, PlayingView)
                ]

                assert render
                if playing_clients:
                    self._add_high_score(hs)
                    for client in playing_clients:
                        client.view = GameOverView(client, hs)
                        client.render()
                else:
                    print("Not adding high score because everyone disconnected")

            elif render:
                for client in self.clients:
                    if isinstance(client.view, PlayingView):
                        client.render()

    def _countdown(self, player: Player, start_time: int) -> None:
        while True:
            time.sleep(1)
            with self.access_game_state() as state:
                if state.start_time != start_time:
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
            start_time = state.start_time
            needs_wait_counter = state.move_blocks_down()
# IF TRADITIONAL
            full_lines = state.find_full_lines()
# END IF
# IF RING
            full_radiuses = state.find_full_radiuses()
# END IF
            for player in needs_wait_counter:
                threading.Thread(
                    target=self._countdown, args=[player, start_time]
                ).start()

# IF TRADITIONAL
        if full_lines:
# END IF
# IF RING
        if full_radiuses:
            print("Full:", full_radiuses)
# END IF
            for color in [47, 0, 47, 0]:
                with self.access_game_state() as state:
                    if state.start_time != start_time:
                        return
# IF TRADITIONAL
                    state.set_color_of_lines(full_lines, color)
# END IF
# IF RING
                    state.set_color_of_rings(full_radiuses, color)
# END IF
                time.sleep(0.1)
            with self.access_game_state() as state:
                if state.start_time != start_time:
                    return
# IF TRADITIONAL
                state.clear_lines(full_lines)
# END IF
# IF RING
                state.clear_rings(full_radiuses)
# END IF

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
        # Enter presses can get sent in different ways...
        # Linux/MacOS raw mode: b"\r"
        # Linux/MacOS cooked mode (not supported): b"YourName\n"
        # Windows: b"\r\n" (handled as if it was \r and \n separately)
        if received == b"\r":
            self._start_playing()  # Will change view, so we won't receive \n
        elif received == b"\n":
            self._error = "Your terminal doesn't seem to be in raw mode. Run 'stty raw' and try again."
        elif received in BACKSPACE:
            # Don't just delete last byte, so that non-ascii can be erased
            # with a single backspace press
            self._name_so_far = self._get_name()[:-1].encode("utf-8")
        else:
            if len(self._name_so_far) < NAME_MAX_LENGTH:
                self._name_so_far += received

    def _start_playing(self) -> None:
        name = self._get_name().strip()
        if not name:
            self._error = "Please write a name before pressing Enter."
            return
        if "\r" in name or "\n" in name:
            self._error = "The name must not contain newline characters."
            return
        if "\t" in name:
            self._error = "The name must not contain tab characters."
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
            if name.lower() in (n.lower() for n in names_of_connected_players):
                self._error = "This name is in use. Try a different name."
                return

            print(self._client.client_address, f"name asking done: {name!r}")
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
# IF TRADITIONAL
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
# END IF
# IF RING
            lines = []
            lines.append(b"o" + b"--" * (2 * GAME_RADIUS + 1) + b"o")
# END IF

# IF TRADITIONAL
                color_bytes = COLOR % player.color
                header_line += color_bytes
                name_line += color_bytes
# END IF
# IF RING
            square_colors = state.get_square_colors()
# END IF

# IF TRADITIONAL
                if player == self.player:
                    header_line += b"==" * WIDTH_PER_PLAYER
                else:
                    header_line += b"--" * WIDTH_PER_PLAYER
                name_line += display_name.center(2 * WIDTH_PER_PLAYER).encode("utf-8")
# END IF
# IF RING
            players_by_letter = {}
# END IF

# IF TRADITIONAL
            name_line += COLOR % 0
            header_line += COLOR % 0
            header_line += b"o"
# END IF
# IF RING
            for player in state.players:
                relative_direction = self.player.world_to_player(
                    player.direction_x, player.direction_y
                )
                letter = {
                    (0, -1): "w",
                    (-1, 0): "a",
                    (0, 1): "s",
                    (1, 0): "d",
                }[relative_direction]
                players_by_letter[letter] = player
# END IF

# IF TRADITIONAL
            lines = [name_line, header_line]
# END IF
# IF RING
            middle_area_content = get_middle_area_content(players_by_letter)
# END IF

# IF TRADITIONAL
            for blink_y, row in enumerate(state.get_square_colors()):
# END IF
# IF RING
            for y in range(-GAME_RADIUS, GAME_RADIUS + 1):
                insert_middle_area_here = None
# END IF
                line = b"|"
# IF TRADITIONAL
                for color in row:
# END IF
# IF RING
                for x in range(-GAME_RADIUS, GAME_RADIUS + 1):
                    if max(abs(x), abs(y)) <= MIDDLE_AREA_RADIUS:
                        insert_middle_area_here = len(line)
                        continue

                    color = square_colors[self.player.player_to_world(x, y)]
# END IF
                    if color is None:
                        line += b"  "
                    else:
                        line += COLOR % color
                        line += b"  "
                        line += COLOR % 0
# IF RING

# END IF
                line += b"|"
# IF RING

                if insert_middle_area_here is not None:
                    line = (
                        line[:insert_middle_area_here]
                        + middle_area_content[y + MIDDLE_AREA_RADIUS]
                        + line[insert_middle_area_here:]
                    )

# END IF
                lines.append(line)

# IF TRADITIONAL
            lines.append(b"o" + b"--" * state.get_width() + b"o")
# END IF
# IF RING
            lines.append(b"o" + b"--" * (2 * GAME_RADIUS + 1) + b"o")
# END IF

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
# IF TRADITIONAL
                state.move_if_possible(self.player, dx=-1, dy=0)
# END IF
# IF RING
                state.move_if_possible(self.player, dx=-1, dy=0, in_player_coords=True)
# END IF
        elif received in (b"D", b"d", RIGHT_ARROW_KEY):
            with self._client.server.access_game_state() as state:
# IF TRADITIONAL
                state.move_if_possible(self.player, dx=1, dy=0)
# END IF
# IF RING
                state.move_if_possible(self.player, dx=1, dy=0, in_player_coords=True)
# END IF
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
# IF RING
        # TODO: remove, for development only
        elif received == b"2":
            with self._client.server.access_game_state() as state:
                state._landed_blocks = {
                    (-x, -y): color for (x, y), color in state._landed_blocks.items()
                }
                if not state.is_valid():
                    state._landed_blocks = {
                        (-x, -y): color
                        for (x, y), color in state._landed_blocks.items()
                    }
# END IF


class GameOverView:
    def __init__(self, client: Client, high_score: HighScore):
        self._client = client
        self._high_score = high_score
        self._all_menu_items = ["New Game", "Quit"]
        self._selected_item = "New Game"

    def get_lines_to_render(self) -> list[bytes]:
        lines = [b""] * 7
        lines[3] = b"Game Over :(".center(80).rstrip()
        lines[4] = (
            f"Your score was {self._high_score.score}.".encode("ascii")
            .center(80)
            .rstrip()
        )

        item_width = 20

        for menu_item in self._all_menu_items:
            display_text = menu_item.center(item_width).encode("utf-8")
            if menu_item == self._selected_item:
                display_text = (COLOR % 47) + display_text  # white background
                display_text = (COLOR % 30) + display_text  # black foreground
                display_text += COLOR % 0
            lines.append(b" " * ((80 - item_width) // 2) + display_text)

        lines.append(b"")
        lines.append(b"")
        lines.append(b"=== HIGH SCORES ".ljust(80, b"="))
        lines.append(b"")
        lines.append(b"| Score | Duration | Players")
        lines.append(b"|-------|----------|-------".ljust(80, b"-"))

        for hs in self._client.server.high_scores[:5]:
            player_string = ", ".join(hs.players)
            line_string = (
                f"| {hs.score:<6}| {hs.get_duration_string():<9}| {player_string}"
            )
            line = line_string.encode("utf-8")
            if hs == self._high_score:
                lines.append((COLOR % 42) + line)
            else:
                lines.append((COLOR % 0) + line)

        lines.append(COLOR % 0)  # Needed if last score was highlighted
        return lines

    def handle_key_press(self, received: bytes) -> bool:
        i = self._all_menu_items.index(self._selected_item)
        if received in (UP_ARROW_KEY, b"W", b"w") and i > 0:
            self._selected_item = self._all_menu_items[i - 1]
        # fmt: off
        if received in (DOWN_ARROW_KEY, b"S", b"s") and i+1 < len(self._all_menu_items):
            self._selected_item = self._all_menu_items[i + 1]
        # fmt: on
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
# IF TRADITIONAL
        # Bottom of terminal. If user types something, it's unlikely to be
# END IF
# IF RING
        # Bottom of game. If user types something, it's unlikely to be
# END IF
        # noticed here before it gets wiped by the next refresh.
# IF TRADITIONAL
        cursor_pos = (24, 1)
# END IF
# IF RING
        cursor_pos = (2 * GAME_RADIUS + 4, 1)
# END IF

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

    def _receive_bytes(self) -> bytes | None:
        try:
            result = self.request.recv(10)
        except OSError as e:
            print(self.client_address, e)
            self.send_queue.put(None)
            return None

        # Checking ESC key here is a bad idea.
        # Arrow keys are sent as ESC + other bytes, and recv() can sometimes
        # return only some of the sent data.
        if (
            not result
            or CONTROL_C in result
            or CONTROL_D in result
            or CONTROL_Q in result
        ):
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

            with self.server.access_game_state():
                self.server.clients.remove(self)
                if isinstance(self.view, PlayingView) and isinstance(
                    self.view.player.moving_block_or_wait_counter, MovingBlock
                ):
                    self.view.player.moving_block_or_wait_counter = None

            print(self.client_address, "Disconnect")
            try:
                self.request.sendall(SHOW_CURSOR)
# IF TRADITIONAL
                self.request.sendall(MOVE_CURSOR % (24, 1))
# END IF
# IF RING
                self.request.sendall(MOVE_CURSOR % (2 * GAME_RADIUS + 4, 1))
# END IF
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
            with self.server.lock:
                self.server.clients.add(self)
            self.send_queue.put(CLEAR_SCREEN)

            received = b""

            while True:
                with self.server.lock:
                    self.render()

                new_chunk = self._receive_bytes()
                if new_chunk is None:
                    break
                received += new_chunk

                # Arrow key presses are received as 3 bytes. The first two of
                # them are CSI, aka ESC [. If we have received a part of an
                # arrow key press, don't process it yet, wait for the rest to
                # arrive instead.
                while received not in (b"", ESC, CSI):
                    if received.startswith(CSI):
                        handle_result = self.view.handle_key_press(received[:3])
                        received = received[3:]
                    else:
                        handle_result = self.view.handle_key_press(received[:1])
                        received = received[1:]

                    if handle_result:
                        return

        except OSError as e:
            print(self.client_address, e)

        finally:
            self.send_queue.put(None)
            send_queue_thread.join()  # Don't close until stuff is sent


# IF TRADITIONAL
server = Server(12345, "high_scores.txt")
# END IF
# IF RING
server = Server(12345, "ring_high_scores.txt")
# END IF
print("Listening on port 12345...")
server.serve_forever()
