from __future__ import annotations
import dataclasses
import time
import contextlib
import socketserver
import textwrap
import threading
import socket
import random
import queue
from abc import abstractmethod
from typing import ClassVar, Iterator

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

# Limited to 4 players:
#   - Traditional mode: must fit in 80 columns
#   - Ring mode: for obvious reasons
PLAYER_COLORS = {31, 32, 33, 34}

# If you mess up, how many seconds should you wait?
WAIT_TIME = 10

# Longest allowed name will get truncated, that's fine
NAME_MAX_LENGTH = 15


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
    def __init__(self, player: Player):
        self.player = player
        self.shape_letter = random.choice(list(BLOCK_SHAPES.keys()))
        self.center_x = player.moving_block_start_x
        self.center_y = player.moving_block_start_y

        # Orient initial block so that it always looks the same.
        # Otherwise may create subtle bugs near end of game, where freshly
        # added block overlaps with landed blocks.
        self.rotation = {
            (0, -1): 0,
            (1, 0): 1,
            (0, 1): 2,
            (-1, 0): 3,
        }[player.up_x, player.up_y]

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
    # What direction is up in the player's view? The up vector always has length 1.
    up_x: int
    up_y: int
    # These should be barely above the top of the game.
    # For example, in traditional tetris, that means moving_block_start_y = -1.
    moving_block_start_x: int
    moving_block_start_y: int
    moving_block_or_wait_counter: MovingBlock | int | None = None

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
            assert name
            name = name[:-1]

    # In ring mode, player's view is rotated so that blocks fall down.
    def world_to_player(self, x: int, y: int) -> tuple[int, int]:
        return (
            (-self.up_y * x + self.up_x * y),
            (-self.up_x * x - self.up_y * y),
        )

    def player_to_world(self, x: int, y: int) -> tuple[int, int]:
        return (
            (-self.up_y * x - self.up_x * y),
            (self.up_x * x - self.up_y * y),
        )


class Game:
    landed_blocks: dict[tuple[int, int], int | None]
    NAME: ClassVar[str]
    HIGH_SCORES_FILE: ClassVar[str]
    TERMINAL_HEIGHT_NEEDED: ClassVar[int]

    def __init__(self) -> None:
        self.reset()

    def reset(self) -> None:
        self.start_time = time.monotonic_ns()
        self.players: list[Player] = []
        self.score = 0

    def is_valid(self) -> bool:
        seen = {
            point for point, color in self.landed_blocks.items() if color is not None
        }

        for block in self._get_moving_blocks():
            coords = block.get_coords()
            if coords & seen:
                return False
            seen.update(coords)

        return True

    def game_is_over(self) -> bool:
        return bool(self.players) and not any(
            isinstance(p.moving_block_or_wait_counter, MovingBlock)
            for p in self.players
        )

    def _get_moving_blocks(self) -> list[MovingBlock]:
        result = []
        for player in self.players:
            if isinstance(player.moving_block_or_wait_counter, MovingBlock):
                result.append(player.moving_block_or_wait_counter)
        return result

    # For clearing squares when a player's wait time ends
    @abstractmethod
    def square_belongs_to_player(self, player: Player, x: int, y: int) -> bool:
        pass

    def end_waiting(self, player: Player, client_currently_connected: bool) -> None:
        assert player.moving_block_or_wait_counter == 0
        if not client_currently_connected:
            player.moving_block_or_wait_counter = None
            return

        for x, y in self.landed_blocks.keys():
            if self.square_belongs_to_player(player, x, y):
                self.landed_blocks[x, y] = None
        player.moving_block_or_wait_counter = MovingBlock(player)

    # This method should:
    #   1. Yield the points that are about to be removed. The yielded value
    #      will be used for the flashing animation.
    #   2. Remove them.
    #   3. Call finish_wiping_full_lines().
    #
    # In ring mode, a full "line" can be a line or a ring. That's why returning
    # a list of full lines would be unnecessarily difficult.
    #
    # When this method is done, moving and landed blocks may overlap.
    @abstractmethod
    def find_and_then_wipe_full_lines(self) -> Iterator[set[tuple[int, int]]]:
        pass

    def finish_wiping_full_lines(self, count: int) -> None:
        if count == 0:
            single_player_score = 0
        elif count == 1:
            single_player_score = 10
        elif count == 2:
            single_player_score = 30
        elif count == 3:
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
            self.score += single_player_score * 3 ** (n - 1)

        # When landed blocks move, they can go on top of moving blocks.
        # This is quite rare, but results in invalid state errors.
        # When this happens, just delete the landed block.
        for moving_block in self._get_moving_blocks():
            for point in moving_block.get_coords():
                if self.landed_blocks.get(point, None) is not None:
                    self.landed_blocks[point] = None

        assert self.is_valid()

    def move_if_possible(
        self, player: Player, dx: int, dy: int, in_player_coords: bool
    ) -> bool:
        assert self.is_valid()
        if in_player_coords:
            dx, dy = player.player_to_world(dx, dy)

        if isinstance(player.moving_block_or_wait_counter, MovingBlock):
            player.moving_block_or_wait_counter.center_x += dx
            player.moving_block_or_wait_counter.center_y += dy
            if self.is_valid():
                return True
            player.moving_block_or_wait_counter.center_x -= dx
            player.moving_block_or_wait_counter.center_y -= dy

        return False

    def move_down_all_the_way(self, player: Player) -> None:
        while self.move_if_possible(player, dx=0, dy=1, in_player_coords=True):
            pass

    def rotate(self, player: Player, counter_clockwise: bool) -> None:
        if isinstance(player.moving_block_or_wait_counter, MovingBlock):
            block = player.moving_block_or_wait_counter
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

    @abstractmethod
    def add_player(self, name: str, color: int) -> Player:
        pass

    # Name can exist already, if player quits and comes back
    def get_existing_player_or_add_new_player(self, name: str) -> Player:
        print(f"{name!r} joins a game with {len(self.players)} existing players")
        if not self.players:
            self.reset()

        game_over = self.game_is_over()

        for player in self.players:
            if player.name.lower() == name.lower():
                # Let's say your caps lock was on accidentally and you type
                # "aKULI" as name when you intended to type "Akuli".
                # If that happens, you can leave the game and join back.
                player.name = name
                break
        else:
            # Add new player
            color = min(PLAYER_COLORS - {p.color for p in self.players})
            player = self.add_player(name, color)

        if not game_over and not isinstance(player.moving_block_or_wait_counter, int):
            player.moving_block_or_wait_counter = MovingBlock(player)
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
                moved = self.move_if_possible(player, dx=0, dy=1, in_player_coords=True)
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

            if any(point not in self.landed_blocks.keys() for point in coords):
                needs_wait_counter.add(player)
            else:
                for point in coords:
                    assert point in self.landed_blocks
                    self.landed_blocks[point] = BLOCK_COLORS[letter]
                player.moving_block_or_wait_counter = MovingBlock(player)

        for player in needs_wait_counter:
            player.moving_block_or_wait_counter = WAIT_TIME
        return needs_wait_counter

    def get_square_colors(self) -> dict[tuple[int, int], int | None]:
        assert self.is_valid()
        result = self.landed_blocks.copy()
        for moving_block in self._get_moving_blocks():
            for point in moving_block.get_coords():
                if point in result:
                    result[point] = BLOCK_COLORS[moving_block.shape_letter]

        return result

    @abstractmethod
    def get_lines_to_render(self, rendering_for_this_player: Player) -> list[bytes]:
        pass


class TraditionalGame(Game):
    NAME = "Traditional game"
    HIGH_SCORES_FILE = "high_scores.txt"
    TERMINAL_HEIGHT_NEEDED = 24

    # Width varies as people join/leave
    HEIGHT = 20
    WIDTH_PER_PLAYER = 7

    def reset(self) -> None:
        super().reset()
        self.landed_blocks = {}

    def square_belongs_to_player(self, player: Player, x: int, y: int) -> bool:
        index = self.players.index(player)
        x_min = self.WIDTH_PER_PLAYER * index
        x_max = x_min + self.WIDTH_PER_PLAYER
        return x in range(x_min, x_max)

    def _get_width(self) -> int:
        return self.WIDTH_PER_PLAYER * len(self.players)

    def is_valid(self) -> bool:
        if self.players:
            assert self.landed_blocks.keys() == {
                (x, y) for x in range(self._get_width()) for y in range(self.HEIGHT)
            }

        return super().is_valid() and all(
            x in range(self._get_width()) and y < self.HEIGHT
            for block in self._get_moving_blocks()
            for x, y in block.get_coords()
        )

    def find_and_then_wipe_full_lines(self) -> Iterator[set[tuple[int, int]]]:
        y_coords = []
        points: set[tuple[int, int]] = set()

        for y in range(self.HEIGHT):
            row = [
                color for point, color in self.landed_blocks.items() if point[1] == y
            ]
            if row and None not in row:
                y_coords.append(y)
                points.update((x, y) for x in range(self._get_width()))

        yield points

        for full_y in sorted(y_coords):
            new_landed_blocks = {}
            for (x, y), color in self.landed_blocks.items():
                if y < full_y:
                    new_landed_blocks[x, y + 1] = color
                if y > full_y:
                    new_landed_blocks[x, y] = color
            self.landed_blocks = {
                point: new_landed_blocks.get(point, None)
                for point in self.landed_blocks.keys()
            }

        self.finish_wiping_full_lines(len(y_coords))

    def add_player(self, name: str, color: int) -> Player:
        x_min = len(self.players) * self.WIDTH_PER_PLAYER
        x_max = x_min + self.WIDTH_PER_PLAYER
        for y in range(self.HEIGHT):
            for x in range(x_min, x_max):
                assert (x, y) not in self.landed_blocks.keys()
                self.landed_blocks[x, y] = None

        player = Player(
            name,
            color,
            up_x=0,
            up_y=-1,
            moving_block_start_x=(
                len(self.players) * self.WIDTH_PER_PLAYER + (self.WIDTH_PER_PLAYER // 2)
            ),
            moving_block_start_y=-1,
        )
        self.players.append(player)
        return player

    def get_lines_to_render(self, rendering_for_this_player: Player) -> list[bytes]:
        header_line = b"o"
        name_line = b" "
        for player in self.players:
            name_text = player.get_name_string(max_length=2 * self.WIDTH_PER_PLAYER)

            color_bytes = COLOR % player.color
            header_line += color_bytes
            name_line += color_bytes

            if player == rendering_for_this_player:
                header_line += b"==" * self.WIDTH_PER_PLAYER
            else:
                header_line += b"--" * self.WIDTH_PER_PLAYER
            name_line += name_text.center(2 * self.WIDTH_PER_PLAYER).encode("utf-8")

        name_line += COLOR % 0
        header_line += COLOR % 0
        header_line += b"o"

        lines = [name_line, header_line]
        square_colors = self.get_square_colors()

        for y in range(self.HEIGHT):
            line = b"|"
            for x in range(self._get_width()):
                color = square_colors[x, y]
                if color is None:
                    line += b"  "
                else:
                    line += COLOR % color
                    line += b"  "
                    line += COLOR % 0
            line += b"|"
            lines.append(line)

        lines.append(b"o" + b"--" * self._get_width() + b"o")
        return lines


class RingGame(Game):
    NAME = "Ring game"
    HIGH_SCORES_FILE = "ring_high_scores.txt"

    # Game size is actually 2*GAME_RADIUS + 1 in each direction.
    GAME_RADIUS = 14  # chosen to fit 80 column terminal (windows)
    TERMINAL_HEIGHT_NEEDED = 2 * GAME_RADIUS + 4

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

    @classmethod
    def get_middle_area_content(
        cls, players_by_letter: dict[str, Player]
    ) -> list[bytes]:
        wrapped_names = {}
        colors = {}

        for letter in "wasd":
            widths = [line.count(letter) for line in cls.MIDDLE_AREA if letter in line]

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
        for template_line_string in cls.MIDDLE_AREA:
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

    def reset(self) -> None:
        super().reset()
        self.landed_blocks = {
            (x, y): None
            for x in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1)
            for y in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1)
            if max(abs(x), abs(y)) > self.MIDDLE_AREA_RADIUS
        }

    def square_belongs_to_player(self, player: Player, x: int, y: int) -> bool:
        # Let me know if you need to understand how this works. I'll explain.
        dot = x * player.up_x + y * player.up_y
        return dot >= 0 and 2 * dot ** 2 >= x * x + y * y

    def is_valid(self) -> bool:
        assert self.landed_blocks.keys() == {
            (x, y)
            for x in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1)
            for y in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1)
            if max(abs(x), abs(y)) > self.MIDDLE_AREA_RADIUS
        }

        if not super().is_valid():
            return False

        for block in self._get_moving_blocks():
            for x, y in block.get_coords():
                if max(abs(x), abs(y)) <= self.MIDDLE_AREA_RADIUS:
                    return False
                player_x, player_y = block.player.world_to_player(x, y)
                if (
                    player_x < -self.GAME_RADIUS
                    or player_x > self.GAME_RADIUS
                    or player_y > 0
                ):
                    return False
        return True

    # In ring mode, full lines are actually full squares, represented by radiuses.
    def find_and_then_wipe_full_lines(self) -> Iterator[set[tuple[int, int]]]:
        radiuses = [
            r
            for r in range(self.MIDDLE_AREA_RADIUS + 1, self.GAME_RADIUS + 1)
            if not any(
                color is None
                for (x, y), color in self.landed_blocks.items()
                if max(abs(x), abs(y)) == r
            )
        ]

        # Lines represented as (dir_x, dir_y, list_of_points) tuples.
        # Direction vector is how other landed blocks will be moved.
        lines = []

        # Horizontal lines
        for y in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1):
            dir_x = 0
            dir_y = -1 if y > 0 else 1
            if abs(y) > self.MIDDLE_AREA_RADIUS:
                points = [
                    (x, y) for x in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1)
                ]
                lines.append((dir_x, dir_y, points))
            else:
                # left side
                points = [
                    (x, y) for x in range(-self.GAME_RADIUS, -self.MIDDLE_AREA_RADIUS)
                ]
                lines.append((dir_x, dir_y, points))
                # right side
                points = [
                    (x, y)
                    for x in range(self.MIDDLE_AREA_RADIUS + 1, self.GAME_RADIUS + 1)
                ]
                lines.append((dir_x, dir_y, points))

        # Vertical lines
        for x in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1):
            dir_x = -1 if x > 0 else 1
            dir_y = 0
            if abs(x) > self.MIDDLE_AREA_RADIUS:
                points = [
                    (x, y) for y in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1)
                ]
                lines.append((dir_x, dir_y, points))
            else:
                # top side
                points = [
                    (x, y) for y in range(-self.GAME_RADIUS, -self.MIDDLE_AREA_RADIUS)
                ]
                lines.append((dir_x, dir_y, points))
                # bottom side
                points = [
                    (x, y)
                    for y in range(self.MIDDLE_AREA_RADIUS + 1, self.GAME_RADIUS + 1)
                ]
                lines.append((dir_x, dir_y, points))

        full_lines = [
            (dir_x, dir_y, points)
            for dir_x, dir_y, points in lines
            if None not in (self.landed_blocks[p] for p in points)
        ]

        yield ( {
            point for dx, dy, points in full_lines for point in points
        } | {
            (x, y)
            for x, y in self.landed_blocks.keys()
            if max(abs(x), abs(y)) in radiuses
        })

        # Remove lines in order where removing first line doesn't mess up
        # coordinates of second, etc
        def sorting_key(line: tuple[int, int, list[tuple[int, int]]]) -> int:
            dir_x, dir_y, points = line
            x, y = points[0]  # any point would do
            return dir_x * x + dir_y * y

        for dir_x, dir_y, points in sorted(full_lines, key=sorting_key):
            self._delete_line(dir_x, dir_y, points)
        for r in radiuses:
            self._delete_ring(r)

        self.finish_wiping_full_lines(len(lines) + len(radiuses))

    def _delete_line(
        self, dir_x: int, dir_y: int, points: list[tuple[int, int]]
    ) -> None:
        # dot product describes where it is along the direction, and is same for all points
        # determinant describes where it is in the opposite direction
        point_and_dir_dot_product = dir_x * points[0][0] + dir_y * points[0][1]
        point_and_dir_determinants = [dir_y * x - dir_x * y for x, y in points]

        new_landed_blocks: dict[tuple[int, int], int | None] = {
            (x, y): None for x, y in self.landed_blocks.keys()
        }
        for (x, y), color in self.landed_blocks.items():
            if color is None or (x, y) in points:
                continue

            # If (x, y) aligns with the line and moving in the direction would
            # bring it closer to the line, then move it
            if (
                dir_y * x - dir_x * y in point_and_dir_determinants
                and x * dir_x + y * dir_y < point_and_dir_dot_product
            ):
                x += dir_x
                y += dir_y

            new_landed_blocks[x, y] = color

        self.landed_blocks = new_landed_blocks

    def _delete_ring(self, r: int) -> None:
        new_landed_blocks = {}
        for (x, y), color in self.landed_blocks.items():
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

        self.landed_blocks = {
            (x, y): new_landed_blocks.get((x, y), None)
            for x, y in self.landed_blocks.keys()
        }

    def delete_full_lines_raw(self, full_lines: list[int]) -> None:
        for r in sorted(full_lines, reverse=True):
            self._delete_ring(r)

    def add_player(self, name: str, color: int) -> Player:
        used_directions = {(p.up_x, p.up_y) for p in self.players}
        opposites_of_used_directions = {(-x, -y) for x, y in used_directions}
        unused_directions = {(0, -1), (0, 1), (-1, 0), (1, 0)} - used_directions

        # If possible, pick a direction opposite to existing player.
        # Choose a direction consistently, for reproducible debugging.
        try:
            up_x, up_y = min(opposites_of_used_directions & unused_directions)
        except ValueError:
            up_x, up_y = min(unused_directions)

        player = Player(
            name,
            color,
            up_x,
            up_y,
            moving_block_start_x=(self.GAME_RADIUS + 1) * up_x,
            moving_block_start_y=(self.GAME_RADIUS + 1) * up_y,
        )
        self.players.append(player)
        return player

    def get_lines_to_render(self, rendering_for_this_player: Player) -> list[bytes]:
        lines = []
        lines.append(b"o" + b"--" * (2 * self.GAME_RADIUS + 1) + b"o")

        players_by_letter = {}
        for player in self.players:
            relative_direction = rendering_for_this_player.world_to_player(
                player.up_x, player.up_y
            )
            letter = {
                (0, -1): "w",
                (-1, 0): "a",
                (0, 1): "s",
                (1, 0): "d",
            }[relative_direction]
            players_by_letter[letter] = player

        middle_area_content = self.get_middle_area_content(players_by_letter)
        square_colors = self.get_square_colors()

        for y in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1):
            insert_middle_area_here = None
            line = b"|"
            for x in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1):
                if max(abs(x), abs(y)) <= self.MIDDLE_AREA_RADIUS:
                    insert_middle_area_here = len(line)
                    continue

                color = square_colors[rendering_for_this_player.player_to_world(x, y)]
                if color is None:
                    line += b"  "
                else:
                    line += COLOR % color
                    line += b"  "
                    line += COLOR % 0

            line += b"|"

            if insert_middle_area_here is not None:
                line = (
                    line[:insert_middle_area_here]
                    + middle_area_content[y + self.MIDDLE_AREA_RADIUS]
                    + line[insert_middle_area_here:]
                )

            lines.append(line)

        lines.append(b"o" + b"--" * (2 * self.GAME_RADIUS + 1) + b"o")
        return lines


GAME_CLASSES: list[type[Game]] = [TraditionalGame, RingGame]


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True

    def __init__(self, port: int):
        super().__init__(("", port), Client)

        # RLock because state usage triggers rendering, which uses state
        self.lock = threading.RLock()  # clients and __games are locked with this
        self.clients: set[Client] = set()
        self.__games = {}

        for klass in GAME_CLASSES:
            self.__games[klass] = klass()
            threading.Thread(target=self._move_blocks_down_thread, args=[klass]).start()

    def _add_high_score(self, file_name: str, hs: HighScore) -> list[HighScore]:
        high_scores = []
        try:
            with open(file_name, "r", encoding="utf-8") as file:
                for line in file:
                    score, duration, *players = line.strip("\n").split("\t")
                    high_scores.append(
                        HighScore(
                            score=int(score),
                            duration_sec=float(duration),
                            players=players,
                        )
                    )
        except FileNotFoundError:
            print("Creating", file_name)
        except (ValueError, OSError) as e:
            print(f"Reading {file_name} failed:", e)
        else:
            print("Found high scores file:", file_name)

        try:
            with open(file_name, "a", encoding="utf-8") as file:
                print(hs.score, hs.duration_sec, *hs.players, file=file, sep="\t")
        except OSError as e:
            print(f"Writing to {file_name} failed:", e)

        high_scores.append(hs)
        return high_scores

    @contextlib.contextmanager
    def access_game(
        self, game_class: type[Game], *, render: bool = True
    ) -> Iterator[Game]:
        with self.lock:
            game = self.__games[game_class]
            assert game.is_valid()
            assert not game.game_is_over()
            yield game

            assert game.is_valid()
            if game.game_is_over():
                duration_ns = time.monotonic_ns() - game.start_time
                high_score = HighScore(
                    score=game.score,
                    duration_sec=duration_ns / (1000 * 1000 * 1000),
                    players=[p.name for p in game.players],
                )
                print("Game over!", high_score)
                game.players.clear()

                playing_clients = [
                    c for c in self.clients if isinstance(c.view, PlayingView)
                ]

                assert render
                if playing_clients:
                    all_high_scores = self._add_high_score(
                        game.HIGH_SCORES_FILE, high_score
                    )
                    all_high_scores.sort(key=(lambda hs: hs.score), reverse=True)
                    best5 = all_high_scores[:5]
                    for client in playing_clients:
                        client.view = GameOverView(
                            client, type(game), high_score, best5
                        )
                        client.render()
                else:
                    print("Not adding high score because everyone disconnected")

            elif render:
                for client in self.clients:
                    if isinstance(client.view, (PlayingView, ChooseGameView)):
                        client.render()

    # TODO: instantiate new Game instead of resetting, so won't need to pass start_time
    def _countdown(self, player: Player, original_game: Game, start_time: int) -> None:
        while True:
            time.sleep(1)
            with self.access_game(type(original_game)) as game:
                if game.start_time != start_time:
                    return

                assert isinstance(player.moving_block_or_wait_counter, int)
                player.moving_block_or_wait_counter -= 1
                if player.moving_block_or_wait_counter == 0:
                    client_currently_connected = any(
                        isinstance(client.view, PlayingView)
                        and client.view.player == player
                        for client in self.clients
                    )
                    game.end_waiting(player, client_currently_connected)
                    return

    def _move_blocks_down_once(self, game_class: type[Game]) -> None:
        with self.access_game(game_class) as game:
            start_time = game.start_time
            needs_wait_counter = game.move_blocks_down()
            full_lines_iter = game.find_and_then_wipe_full_lines()
            full_points = next(full_lines_iter)
            for player in needs_wait_counter:
                threading.Thread(
                    target=self._countdown, args=[player, game, start_time]
                ).start()

        if full_points:
            print(f"Flashing and wiping {len(full_points)} points")
            for color in [47, 0, 47, 0]:
                with self.access_game(game_class) as game:
                    if game.start_time != start_time:
                        return
                    for point in full_points:
                        game.landed_blocks[point] = color
                time.sleep(0.1)
            with self.access_game(game_class) as game:
                if game.start_time != start_time:
                    return
                try:
                    next(full_lines_iter)  # run past yield, which deletes points
                except StopIteration:
                    pass  # function ended without a second yield

    def _move_blocks_down_thread(self, game_class: type[Game]) -> None:
        while True:
            self._move_blocks_down_once(game_class)
            with self.access_game(game_class, render=False) as game:
                score = game.score
            time.sleep(0.5 / (1 + score / 1000))


class AskNameView:
    def __init__(self, client: Client):
        assert client.name is None
        self._client = client
        self._name_so_far = b""
        self._error: str | None = None
        self._backslash_r_received = False

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
            self._on_enter_pressed()
            self._backslash_r_received = True
        elif received == b"\n":
            if not self._backslash_r_received:
                self._error = "Your terminal doesn't seem to be in raw mode. Run 'stty raw' and try again."
        elif received in BACKSPACE:
            # Don't just delete last byte, so that non-ascii can be erased
            # with a single backspace press
            self._name_so_far = self._get_name()[:-1].encode("utf-8")
        else:
            if len(self._name_so_far) < NAME_MAX_LENGTH:
                self._name_so_far += received

    def _on_enter_pressed(self) -> None:
        name = self._get_name().strip()
        if not name:
            self._error = "Please write a name before pressing Enter."
            return
        if any(c.isspace() and c != " " for c in name):
            self._error = (
                "The name can contain spaces, but not other whitespace characters."
            )
            return

        # Must lock while assigning name and color, so can't get duplicates
        with self._client.server.lock:
            # Prevent two simultaneous clients with the same name.
            # But it's fine if you leave and then join back with the same name.
            names_of_connected_players = {
                client.name
                for client in self._client.server.clients
                if client.name is not None
            }
            if name.lower() in (n.lower() for n in names_of_connected_players):
                self._error = "This name is in use. Try a different name."
                return

            print(self._client.client_address, f"name asking done: {name!r}")
            self._client.send_queue.put(HIDE_CURSOR)
            self._client.name = name
            self._client.view = ChooseGameView(self._client)


class MenuView:
    def __init__(self) -> None:
        self.menu_items: list[str] = []
        self.selected_index = 0

    def get_lines_to_render(self) -> list[bytes]:
        item_width = 30
        result = [b"", b""]
        for index, item in enumerate(self.menu_items):
            display_text = item.center(item_width).encode("utf-8")
            if index == self.selected_index:
                display_text = (COLOR % 47) + display_text  # white background
                display_text = (COLOR % 30) + display_text  # black foreground
                display_text += COLOR % 0
            result.append(b" " * ((80 - item_width) // 2) + display_text)
        return result

    # Return True to quit the game
    @abstractmethod
    def on_enter_pressed(self) -> bool | None:
        pass

    def handle_key_press(self, received: bytes) -> bool:
        if received in (UP_ARROW_KEY, b"W", b"w") and self.selected_index > 0:
            self.selected_index -= 1
        if received in (DOWN_ARROW_KEY, b"S", b"s") and self.selected_index + 1 < len(
            self.menu_items
        ):
            self.selected_index += 1
        if received == b"\r":
            return bool(self.on_enter_pressed())
        return False  # do not quit yet


class ChooseGameView(MenuView):
    def __init__(
        self, client: Client, previous_game_class: type[Game] = GAME_CLASSES[0]
    ):
        super().__init__()
        self._client = client
        self.selected_index = GAME_CLASSES.index(previous_game_class)

    def get_lines_to_render(self) -> list[bytes]:
        with self._client.server.lock:
            self.menu_items.clear()
            for game_class in GAME_CLASSES:
                text = game_class.NAME
                with self._client.server.access_game(game_class, render=False) as game:
                    if len(game.players) == 1:
                        text += " (1 player)"
                    else:
                        text += f" ({len(game.players)} players)"
                self.menu_items.append(text)
            self.menu_items.append("Quit")
            return (
                ASCII_ART.encode("ascii").split(b"\n") + super().get_lines_to_render()
            )

    def on_enter_pressed(self) -> bool:
        if self.menu_items[self.selected_index] == "Quit":
            return True
        self._client.view = CheckTerminalSizeView(
            self._client, GAME_CLASSES[self.selected_index]
        )
        return False


class CheckTerminalSizeView:
    def __init__(self, client: Client, game_class: type[Game]):
        self._client = client
        self._game_class = game_class

        # Terminal needs to be refreshed frequently as the user resizes it.
        threading.Thread(target=self._refresh_loop).start()

    def _refresh_loop(self) -> None:
        # Wait until the view is assigned to the client (lol)
        time.sleep(0.1)
        assert self._client.view == self

        while True:
            with self._client.server.lock:
                if self._client.view != self:
                    return
                self._client.last_displayed_lines.clear()
                self._client.render()
            time.sleep(0.5)

    def get_lines_to_render(self) -> list[bytes]:
        width = 80
        height = self._game_class.TERMINAL_HEIGHT_NEEDED

        text = """
        Please adjust your terminal size so that you can
        see the entire rectangle. Press Enter when done.
        """
        text_lines = [
            line.strip().encode("ascii") for line in text.strip().splitlines()
        ]

        lines = [b"|" + b" " * (width - 2) + b"|"] * height
        lines[0] = lines[-1] = b"o" + b"-" * (width - 2) + b"o"
        for index, line in enumerate(text_lines):
            lines[2 + index] = b"|" + line.center(width - 2) + b"|"
            lines[-2 - len(text_lines) + index] = b"|" + line.center(width - 2) + b"|"

        return lines

    def handle_key_press(self, received: bytes) -> None:
        if received == b"\r":
            with self._client.server.access_game(self._game_class) as game:
                assert self._client.name is not None
                player = game.get_existing_player_or_add_new_player(self._client.name)
                self._client.view = PlayingView(self._client, game, player)


class GameOverView(MenuView):
    def __init__(
        self,
        client: Client,
        game_class: type[Game],
        new_high_score: HighScore,
        high_scores: list[HighScore],
    ):
        super().__init__()
        self.menu_items.extend(["New Game", "Choose a different game", "Quit"])
        self._client = client
        self._game_class = game_class
        self._new_high_score = new_high_score
        self._high_scores = high_scores

    def get_lines_to_render(self) -> list[bytes]:
        lines = [b"", b"", b""]
        lines.append(b"Game Over :(".center(80).rstrip())
        lines.append(
            f"Your score was {self._new_high_score.score}.".encode("ascii")
            .center(80)
            .rstrip()
        )

        lines.extend(super().get_lines_to_render())

        lines.append(b"")
        lines.append(b"")
        lines.append(b"=== HIGH SCORES ".ljust(80, b"="))
        lines.append(b"")
        lines.append(b"| Score | Duration | Players")
        lines.append(b"|-------|----------|-------".ljust(80, b"-"))

        for hs in self._high_scores:
            player_string = ", ".join(hs.players)
            line_string = (
                f"| {hs.score:<6}| {hs.get_duration_string():<9}| {player_string}"
            )
            line = line_string.encode("utf-8")
            if hs == self._new_high_score:
                lines.append((COLOR % 42) + line)
            else:
                lines.append((COLOR % 0) + line)

        lines.append(COLOR % 0)  # Needed if last score was highlighted
        return lines

    def on_enter_pressed(self) -> bool:
        text = self.menu_items[self.selected_index]
        if text == "New Game":
            assert self._client.name is not None
            with self._client.server.access_game(self._game_class) as game:
                player = game.get_existing_player_or_add_new_player(self._client.name)
                self._client.view = PlayingView(self._client, game, player)
        elif text == "Choose a different game":
            self._client.view = ChooseGameView(self._client, self._game_class)
        elif text == "Quit":
            return True
        else:
            raise NotImplementedError(text)

        return False


class PlayingView:
    def __init__(self, client: Client, game: Game, player: Player):
        self._client = client
        self.game = game
        self.player = player

    def get_lines_to_render(self) -> list[bytes]:
        with self._client.server.access_game(type(self.game), render=False) as game:
            lines = game.get_lines_to_render(self.player)
            lines[5] += f"  Score: {game.score}".encode("ascii")
            if self._client.rotate_counter_clockwise:
                lines[6] += b"  Counter-clockwise"
            if isinstance(self.player.moving_block_or_wait_counter, int):
                n = self.player.moving_block_or_wait_counter
                lines[8] += f"  Please wait: {n}".encode("ascii")
            return lines

    def handle_key_press(self, received: bytes) -> None:
        if received in (b"A", b"a", LEFT_ARROW_KEY):
            with self._client.server.access_game(type(self.game)) as game:
                game.move_if_possible(self.player, dx=-1, dy=0, in_player_coords=True)
        elif received in (b"D", b"d", RIGHT_ARROW_KEY):
            with self._client.server.access_game(type(self.game)) as game:
                game.move_if_possible(self.player, dx=1, dy=0, in_player_coords=True)
        elif received in (b"W", b"w", UP_ARROW_KEY, b"\r"):
            with self._client.server.access_game(type(self.game)) as game:
                game.rotate(self.player, self._client.rotate_counter_clockwise)
        elif received in (b"S", b"s", DOWN_ARROW_KEY, b" "):
            with self._client.server.access_game(type(self.game)) as game:
                game.move_down_all_the_way(self.player)
        elif received in (b"R", b"r"):
            self._client.rotate_counter_clockwise = (
                not self._client.rotate_counter_clockwise
            )
        elif received in (b"F", b"f") and isinstance(self.game, RingGame):
            with self._client.server.access_game(RingGame) as state:
                if len(state.players) == 1:
                    old_landed_blocks = state.landed_blocks.copy()
                    state.landed_blocks = {
                        (-x, -y): color for (x, y), color in state.landed_blocks.items()
                    }
                    if not state.is_valid():
                        state.landed_blocks = old_landed_blocks


class Client(socketserver.BaseRequestHandler):
    server: Server
    request: socket.socket

    def setup(self) -> None:
        print(self.client_address, "New connection")
        self.last_displayed_lines: list[bytes] = []
        self.send_queue: queue.Queue[bytes | None] = queue.Queue()
        self.name: str | None = None
        self.view: (
            AskNameView
            | ChooseGameView
            | CheckTerminalSizeView
            | PlayingView
            | GameOverView
        ) = AskNameView(self)
        self.rotate_counter_clockwise = False

    def render(self) -> None:
        if isinstance(self.view, CheckTerminalSizeView):
            # Very different from other views
            self.last_displayed_lines.clear()
            self.send_queue.put(
                CLEAR_SCREEN
                + (MOVE_CURSOR % (1, 1))
                + b"\r\n".join(self.view.get_lines_to_render())
            )
            return

        if isinstance(self.view, AskNameView):
            lines, cursor_pos = self.view.get_lines_to_render_and_cursor_pos()
        else:
            # Bottom of view. If user types something, it's unlikely to be
            # noticed here before it gets wiped by the next refresh.
            lines = self.view.get_lines_to_render()
            cursor_pos = (len(lines) + 1, 1)

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

            with self.server.lock:
                self.server.clients.remove(self)
                if isinstance(self.view, PlayingView) and isinstance(
                    self.view.player.moving_block_or_wait_counter, MovingBlock
                ):
                    with self.server.access_game(type(self.view.game)):
                        self.view.player.moving_block_or_wait_counter = None

            print(self.client_address, "Disconnect")
            try:
                self.request.sendall(SHOW_CURSOR)
                self.request.sendall(b"\r")  # move cursor to start of line
                self.request.sendall(CLEAR_FROM_CURSOR_TO_END_OF_SCREEN)
            except OSError as e:
                print(self.client_address, e)
            try:
                self.request.shutdown(socket.SHUT_RDWR)
            except OSError as e:
                print(self.client_address, e)
            break

    def handle(self) -> None:
        with self.server.lock:
            if len(self.server.clients) >= len(GAME_CLASSES) * len(PLAYER_COLORS):
                full = True
            else:
                full = False
                self.server.clients.add(self)

        # do not send while locked, would freeze ongoing games
        if full:
            print(self.client_address, "Sending server full message")
            self.request.sendall(b"The server is full. Please try again later.\r\n")
            return

        send_queue_thread = threading.Thread(target=self._send_queue_thread)
        send_queue_thread.start()

        try:
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


def main() -> None:
    server = Server(12345)
    print("Listening on port 12345...")
    server.serve_forever()


main()
