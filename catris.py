from __future__ import annotations
import asyncio
import collections
import dataclasses
import time
import sys
import textwrap
import random
from abc import abstractmethod
from typing import Any, ClassVar, Iterator, Callable

if sys.version_info >= (3, 9):
    from asyncio import to_thread
else:
    # copied from source code with slight modifications
    async def to_thread(func: Any, *args: Any, **kwargs: Any) -> Any:
        import contextvars, functools

        loop = asyncio.get_running_loop()
        ctx = contextvars.copy_context()
        func_call = functools.partial(ctx.run, func, *args, **kwargs)
        return await loop.run_in_executor(None, func_call)


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
    "BOMB": [(x, y) for x in (-1, 0, 1) for y in (-2, -1, 0)],
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
    "BOMB": 33,  # yellow text, others are background colors
}

# Limited to 4 players:
#   - Traditional mode: must fit in 80 columns
#   - Ring mode: for obvious reasons
PLAYER_COLORS = {31, 32, 33, 34}

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


class Bomb:
    def __init__(self) -> None:
        self.timer = 15

    def copy(self) -> Bomb:
        result = Bomb()
        result.timer = self.timer
        return result

    def get_text(self) -> bytes:
        if self.timer <= 3:
            # red middle text, bomb about to explode
            color = 31
        else:
            color = BLOCK_COLORS["BOMB"]
        text = str(self.timer).center(2).encode("ascii")
        return (COLOR % color) + text + (COLOR % 0)


def choose_shape() -> str:
    if random.random() < 0.01:
        print("Adding special bomb block")
        return "BOMB"

    choices = list(BLOCK_SHAPES.keys())
    choices.remove("BOMB")
    return random.choice(choices)


class MovingBlock:
    def __init__(self, player: Player):
        self.player = player
        self.shape_id = player.next_shape_id
        player.next_shape_id = choose_shape()

        if self.shape_id == "BOMB":
            self.bomb: Bomb | None = Bomb()
        else:
            self.bomb = None

        self.center_x = player.moving_block_start_x
        self.center_y = player.moving_block_start_y
        self.fast_down = False

        # Orient initial block so that it always looks the same.
        # Otherwise may create subtle bugs near end of game, where freshly
        # added block overlaps with landed blocks.
        self.rotation = {
            (0, -1): 0,
            (1, 0): 1,
            (0, 1): 2,
            (-1, 0): 3,
        }[player.up_x, player.up_y]


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
    next_shape_id: str = dataclasses.field(default_factory=choose_shape)

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

    def set_fast_down(self, value: bool) -> None:
        if isinstance(self.moving_block_or_wait_counter, MovingBlock):
            self.moving_block_or_wait_counter.fast_down = value

    # This is called only when there's one player.
    # Could instead flip the world around, but it would be difficult if there's
    # currently a flashing row.
    def flip_view(self) -> None:
        self.up_x *= -1
        self.up_y *= -1
        self.moving_block_start_x *= -1
        self.moving_block_start_y *= -1
        if isinstance(self.moving_block_or_wait_counter, MovingBlock):
            self.moving_block_or_wait_counter.center_x *= -1
            self.moving_block_or_wait_counter.center_y *= -1
            self.moving_block_or_wait_counter.rotation += 2


class Game:
    NAME: ClassVar[str]
    HIGH_SCORES_FILE: ClassVar[str]
    TERMINAL_HEIGHT_NEEDED: ClassVar[int]

    def __init__(self) -> None:
        self.start_time = time.monotonic_ns()
        self.players: list[Player] = []
        self.score = 0
        self.landed_blocks: dict[tuple[int, int], int | Bomb | None] = {}
        self.tasks: list[asyncio.Task[Any]] = []
        self.tasks.append(asyncio.create_task(self._move_blocks_down_task(False)))
        self.tasks.append(asyncio.create_task(self._move_blocks_down_task(True)))
        self.tasks.append(asyncio.create_task(self._bomb_task()))
        self.need_render_event = asyncio.Event()
        self.player_has_a_connected_client: Callable[[Player], bool]  # set in Server

        # Hold this when wiping full lines or exploding a bomb or similar.
        # Prevents moving blocks down and causing weird bugs.
        self.flashing_lock = asyncio.Lock()
        self.flashing_squares: dict[tuple[int, int], int] = {}

    def is_valid(self) -> bool:
        seen = {
            point for point, value in self.landed_blocks.items() if value is not None
        }

        for block in self._get_moving_blocks():
            coords = self.get_moving_block_coords(block)
            if coords & seen:
                return False
            seen.update(coords)

        return True

    def game_is_over(self) -> bool:
        return bool(self.players) and not any(
            isinstance(p.moving_block_or_wait_counter, MovingBlock)
            for p in self.players
        )

    # in this class, so that RingGame can override it
    @classmethod
    def get_moving_block_coords(cls, block: MovingBlock) -> set[tuple[int, int]]:
        result = set()
        for rel_x, rel_y in BLOCK_SHAPES[block.shape_id]:
            for iteration in range(block.rotation % 4):
                rel_x, rel_y = -rel_y, rel_x
            result.add((block.center_x + rel_x, block.center_y + rel_y))
        return result

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

    # This method should:
    #   1. Yield the points that are about to be removed. The yielded value
    #      will be used for the flashing animation.
    #   2. Remove them.
    #   3. Increment score.
    #   4. Call finish_wiping_full_lines().
    #
    # In ring mode, a full "line" can be a line or a ring. That's why returning
    # a list of full lines would be unnecessarily difficult.
    #
    # When this method is done, moving and landed blocks may overlap.
    @abstractmethod
    def find_and_then_wipe_full_lines(self) -> Iterator[set[tuple[int, int]]]:
        pass

    def finish_wiping_full_lines(self) -> None:
        # When landed blocks move, they can go on top of moving blocks.
        # This is quite rare, but results in invalid state errors.
        # When this happens, just delete the landed block.
        for moving_block in self._get_moving_blocks():
            for point in self.get_moving_block_coords(moving_block):
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
                self.need_render_event.set()
                return True
            player.moving_block_or_wait_counter.center_x -= dx
            player.moving_block_or_wait_counter.center_y -= dy

        return False

    def rotate(self, player: Player, counter_clockwise: bool) -> None:
        if isinstance(player.moving_block_or_wait_counter, MovingBlock):
            block = player.moving_block_or_wait_counter
            if block.shape_id == "O":
                return

            old_rotation = block.rotation
            if counter_clockwise:
                new_rotation = old_rotation - 1
            else:
                new_rotation = old_rotation + 1

            if block.shape_id in "ISZ":
                new_rotation %= 2

            assert self.is_valid()
            block.rotation = new_rotation
            if self.is_valid():
                self.need_render_event.set()
            else:
                block.rotation = old_rotation

    @abstractmethod
    def add_player(self, name: str, color: int) -> Player:
        pass

    # Name can exist already, if player quits and comes back
    def get_existing_player_or_add_new_player(self, name: str) -> Player | None:
        if not self.player_can_join(name):
            return None

        print(f"{name!r} joins a game with {len(self.players)} existing players")
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
            self.need_render_event.set()
        return player

    def player_can_join(self, name: str) -> bool:
        return len(self.players) < len(PLAYER_COLORS) or name.lower() in (
            p.name.lower() for p in self.players
        )

    def move_blocks_down(self, fast: bool) -> set[Player]:
        # Blocks of different users can be on each other's way, but should
        # still be moved if the bottommost block will move.
        #
        # Solution: repeatedly try to move each one, and stop when nothing moves.
        todo = {
            player
            for player in self.players
            if isinstance(player.moving_block_or_wait_counter, MovingBlock)
            and player.moving_block_or_wait_counter.fast_down == fast
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
            block = player.moving_block_or_wait_counter
            assert isinstance(block, MovingBlock)
            coords = self.get_moving_block_coords(block)

            if block.fast_down:
                block.fast_down = False
            elif coords.issubset(self.landed_blocks.keys()):
                for point in coords:
                    if block.bomb is None:
                        self.landed_blocks[point] = BLOCK_COLORS[block.shape_id]
                    else:
                        self.landed_blocks[point] = block.bomb.copy()
                player.moving_block_or_wait_counter = MovingBlock(player)
            else:
                needs_wait_counter.add(player)

        return needs_wait_counter

    def get_square_texts(self) -> dict[tuple[int, int], bytes]:
        assert self.is_valid()
        result = {
            point: (
                value.get_text()
                if isinstance(value, Bomb)
                else (COLOR % value) + b"  " + (COLOR % 0)
            )
            for point, value in self.landed_blocks.items()
            if value is not None
        }
        for moving_block in self._get_moving_blocks():
            for point in self.get_moving_block_coords(moving_block):
                if point in self.landed_blocks:
                    if moving_block.bomb is None:
                        result[point] = (
                            (COLOR % BLOCK_COLORS[moving_block.shape_id])
                            + b"  "
                            + (COLOR % 0)
                        )
                    else:
                        result[point] = moving_block.bomb.get_text()

        for point, color in self.flashing_squares.items():
            if point in self.landed_blocks:
                result[point] = (COLOR % color) + b"  " + (COLOR % 0)

        return result

    @abstractmethod
    def get_lines_to_render(self, rendering_for_this_player: Player) -> list[bytes]:
        pass

    async def _bomb_task(self) -> None:
        while True:
            await asyncio.sleep(1)

            bombs: list[tuple[int, int, Bomb, Player | None]] = []
            for (x, y), value in self.landed_blocks.items():
                if isinstance(value, Bomb):
                    bombs.append((x, y, value, None))

            for player in self.players:
                block = player.moving_block_or_wait_counter
                if isinstance(block, MovingBlock) and block.bomb is not None:
                    bombs.append((block.center_x, block.center_y, block.bomb, player))

            exploding_points = set()
            for bomb_x, bomb_y, bomb, player_who_moves_bomb in bombs:
                bomb.timer -= 1
                if bomb.timer == 0:
                    print("Bomb explodes! BOOOOOOMM!!!11!")

                    radius = 3.5
                    exploding_points |= {
                        (x, y)
                        for x, y in self.landed_blocks.keys()
                        if (x - bomb_x) ** 2 + (y - bomb_y) ** 2 < radius**2
                    }
                    if player_who_moves_bomb is not None:
                        player_who_moves_bomb.moving_block_or_wait_counter = (
                            MovingBlock(player_who_moves_bomb)
                        )

            if exploding_points:
                async with self.flashing_lock:
                    await self.flash(exploding_points, 41)
                    for point in exploding_points:
                        self.landed_blocks[point] = None

            if bombs:
                self.need_render_event.set()

    async def _countdown(self, player: Player) -> None:
        player.moving_block_or_wait_counter = 10
        self.need_render_event.set()

        while player.moving_block_or_wait_counter > 0:
            await asyncio.sleep(1)
            assert isinstance(player.moving_block_or_wait_counter, int)
            player.moving_block_or_wait_counter -= 1
            self.need_render_event.set()

        if self.player_has_a_connected_client(player):
            for x, y in self.landed_blocks.keys():
                if self.square_belongs_to_player(player, x, y):
                    self.landed_blocks[x, y] = None
            player.moving_block_or_wait_counter = MovingBlock(player)
        else:
            player.moving_block_or_wait_counter = None

        self.need_render_event.set()

    # Make sure to hold flashing_lock.
    # If you want to erase landed blocks, do that too while holding the lock.
    async def flash(self, points: set[tuple[int, int]], color: int) -> None:
        for display_color in [color, 0, color, 0]:
            for point in points:
                self.flashing_squares[point] = display_color
            self.need_render_event.set()
            await asyncio.sleep(0.1)

        for point in points:
            try:
                del self.flashing_squares[point]
            except KeyError:
                # can happen with simultaneous overlapping flashes
                pass
        self.need_render_event.set()

    async def _move_blocks_down_once(self, fast: bool) -> None:
        needs_wait_counter = self.move_blocks_down(fast)
        async with self.flashing_lock:
            full_lines_iter = self.find_and_then_wipe_full_lines()
            full_points = next(full_lines_iter)
            for player in needs_wait_counter:
                self.tasks.append(asyncio.create_task(self._countdown(player)))
            self.need_render_event.set()

            if full_points:
                await self.flash(full_points, 47)
                try:
                    # run past yield, which deletes points
                    next(full_lines_iter)
                except StopIteration:
                    # This means function ended without a second yield.
                    # It's expected, and in fact happens every time.
                    pass

            self.need_render_event.set()

    async def _move_blocks_down_task(self, fast: bool) -> None:
        while True:
            if fast:
                await asyncio.sleep(0.02)
            else:
                await asyncio.sleep(0.5 / (1 + self.score / 1000))
            await self._move_blocks_down_once(fast)


class TraditionalGame(Game):
    NAME = "Traditional game"
    HIGH_SCORES_FILE = "high_scores.txt"
    TERMINAL_HEIGHT_NEEDED = 24

    # Width varies as people join/leave
    HEIGHT = 20
    WIDTH_PER_PLAYER = 7

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
            for x, y in self.get_moving_block_coords(block)
        )

    def find_and_then_wipe_full_lines(self) -> Iterator[set[tuple[int, int]]]:
        y_coords = []
        points: set[tuple[int, int]] = set()

        for y in range(self.HEIGHT):
            row = [
                color for point, color in self.landed_blocks.items() if point[1] == y
            ]
            if row and None not in row:
                print("Clearing full row:", row)
                y_coords.append(y)
                points.update((x, y) for x in range(self._get_width()))

        yield points

        if len(y_coords) == 0:
            single_player_score = 0
        elif len(y_coords) == 1:
            single_player_score = 10
        elif len(y_coords) == 2:
            single_player_score = 30
        elif len(y_coords) == 3:
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

        for full_y in sorted(y_coords):
            new_landed_blocks = {}
            for (x, y), value in self.landed_blocks.items():
                if y < full_y:
                    new_landed_blocks[x, y + 1] = value
                if y > full_y:
                    new_landed_blocks[x, y] = value
            self.landed_blocks = {
                point: new_landed_blocks.get(point, None)
                for point in self.landed_blocks.keys()
            }

        self.finish_wiping_full_lines()

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
        square_bytes = self.get_square_texts()

        for y in range(self.HEIGHT):
            line = b"|"
            for x in range(self._get_width()):
                line += square_bytes.get((x, y), b"  ")
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

    def __init__(self) -> None:
        super().__init__()
        self.landed_blocks = {
            (x, y): None
            for x in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1)
            for y in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1)
            if max(abs(x), abs(y)) > self.MIDDLE_AREA_RADIUS
        }

    @classmethod
    def _get_middle_area_content(
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

    def square_belongs_to_player(self, player: Player, x: int, y: int) -> bool:
        # Let me know if you need to understand how this works. I'll explain.
        dot = x * player.up_x + y * player.up_y
        return dot >= 0 and 2 * dot**2 >= x * x + y * y

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
            for x, y in self.get_moving_block_coords(block):
                if max(abs(x), abs(y)) <= self.MIDDLE_AREA_RADIUS:
                    return False
                player_x, player_y = block.player.world_to_player(x, y)
                if player_x < -self.GAME_RADIUS or player_x > self.GAME_RADIUS:
                    return False
        return True

    @classmethod
    def get_moving_block_coords(cls, block: MovingBlock) -> set[tuple[int, int]]:
        result = set()
        down_x = -block.player.up_x
        down_y = -block.player.up_y
        for x, y in super().get_moving_block_coords(block):
            # Wrap back to top, if coordinates go too far down
            while x * down_x + y * down_y > cls.GAME_RADIUS:
                x += (2 * cls.GAME_RADIUS + 1) * block.player.up_x
                y += (2 * cls.GAME_RADIUS + 1) * block.player.up_y
            result.add((x, y))
        return result

    # In ring mode, full lines are actually full squares, represented by radiuses.
    def find_and_then_wipe_full_lines(self) -> Iterator[set[tuple[int, int]]]:
        radiuses = [
            r
            for r in range(self.MIDDLE_AREA_RADIUS + 1, self.GAME_RADIUS + 1)
            if not any(
                value is None
                for (x, y), value in self.landed_blocks.items()
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

        yield (
            {point for dx, dy, points in full_lines for point in points}
            | {
                (x, y)
                for x, y in self.landed_blocks.keys()
                if max(abs(x), abs(y)) in radiuses
            }
        )

        self.score += 10 * len(full_lines) + 100 * len(radiuses)

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

        self.finish_wiping_full_lines()

    def _delete_line(
        self, dir_x: int, dir_y: int, points: list[tuple[int, int]]
    ) -> None:
        # dot product describes where it is along the direction, and is same for all points
        # determinant describes where it is in the opposite direction
        point_and_dir_dot_product = dir_x * points[0][0] + dir_y * points[0][1]
        point_and_dir_determinants = [dir_y * x - dir_x * y for x, y in points]

        new_landed_blocks: dict[tuple[int, int], int | Bomb | None] = {
            (x, y): None for x, y in self.landed_blocks.keys()
        }
        for (x, y), value in self.landed_blocks.items():
            if value is None or (x, y) in points:
                continue

            # If (x, y) aligns with the line and moving in the direction would
            # bring it closer to the line, then move it
            if (
                dir_y * x - dir_x * y in point_and_dir_determinants
                and x * dir_x + y * dir_y < point_and_dir_dot_product
            ):
                x += dir_x
                y += dir_y

            new_landed_blocks[x, y] = value

        self.landed_blocks = new_landed_blocks

    def _delete_ring(self, r: int) -> None:
        new_landed_blocks = {}
        for (x, y), value in self.landed_blocks.items():
            if value is None:
                continue

            # preserve squares inside the ring
            if max(abs(x), abs(y)) < r:
                new_landed_blocks[x, y] = value

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

            new_landed_blocks[x, y] = value

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

        middle_area_content = self._get_middle_area_content(players_by_letter)
        square_bytes = self.get_square_texts()

        for y in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1):
            insert_middle_area_here = None
            line = b"|"
            for x in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1):
                if max(abs(x), abs(y)) <= self.MIDDLE_AREA_RADIUS:
                    insert_middle_area_here = len(line)
                    continue
                line += square_bytes.get(rendering_for_this_player.player_to_world(x, y), b"  ")

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


class Server:
    def __init__(self) -> None:
        self.clients: set[Client] = set()
        self.games: set[Game] = set()

    def start_game(self, client: Client, game_class: type[Game]) -> None:
        assert client in self.clients

        existing_games = [game for game in self.games if isinstance(game, game_class)]
        if existing_games:
            [game] = existing_games
        else:
            game = game_class()
            game.player_has_a_connected_client = self._player_has_a_connected_client
            game.tasks.append(asyncio.create_task(self._render_task(game)))
            self.games.add(game)

        assert client.name is not None
        player = game.get_existing_player_or_add_new_player(client.name)
        if player is None:
            client.view = ChooseGameView(client, game_class)
        else:
            client.view = PlayingView(client, game, player)

        # ChooseGameViews display how many players are currently playing each game
        for client in self.clients:
            if isinstance(client.view, ChooseGameView):
                client.render()

    def _player_has_a_connected_client(self, player: Player) -> bool:
        return any(
            isinstance(client.view, PlayingView) and client.view.player == player
            for client in self.clients
        )

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

    async def _high_score_task(self, game: Game, high_score: HighScore) -> None:
        high_scores = await to_thread(
            self._add_high_score, game.HIGH_SCORES_FILE, high_score
        )
        high_scores.sort(key=(lambda hs: hs.score), reverse=True)
        best5 = high_scores[:5]
        for client in self.clients:
            if isinstance(client.view, GameOverView) and client.view.game == game:
                client.view.set_high_scores(best5)
                client.render()

    async def _render_task(self, game: Game) -> None:
        while True:
            await game.need_render_event.wait()
            game.need_render_event.clear()
            self.render_game(game)

    def render_game(self, game: Game) -> None:
        assert game in self.games
        assert game.is_valid()

        playing_clients = [
            c
            for c in self.clients
            if isinstance(c.view, PlayingView) and c.view.game == game
        ]

        game.tasks = [t for t in game.tasks if not t.done()]

        if game.game_is_over():
            self.games.remove(game)
            for task in game.tasks:
                task.cancel()

            duration_ns = time.monotonic_ns() - game.start_time
            high_score = HighScore(
                score=game.score,
                duration_sec=duration_ns / (1000 * 1000 * 1000),
                players=[p.name for p in game.players],
            )
            print("Game over!", high_score)
            game.players.clear()

            if playing_clients:
                for client in playing_clients:
                    client.view = GameOverView(client, game, high_score)
                    client.render()
                asyncio.create_task(self._high_score_task(game, high_score))
            else:
                print("Not adding high score because everyone disconnected")

        else:
            for client in playing_clients:
                client.render()

        # ChooseGameViews display how many players are currently playing each game
        for client in self.clients:
            if isinstance(client.view, ChooseGameView):
                client.render()

    async def handle_connection(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        client = Client(self, reader, writer)
        await client.handle()


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

        # Prevent two simultaneous clients with the same name.
        # But it's fine if you leave and then join back with the same name.
        if name.lower() in (
            client.name.lower()
            for client in self._client.server.clients
            if client.name is not None
        ):
            self._error = "This name is in use. Try a different name."
            return

        print(f"name asking done: {name!r}")
        self._client.writer.write(HIDE_CURSOR)
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
        self._fill_menu()

    def _should_show_cannot_join_error(self) -> bool:
        assert self._client.name is not None
        return self.selected_index < len(GAME_CLASSES) and any(
            isinstance(g, GAME_CLASSES[self.selected_index])
            and not g.player_can_join(self._client.name)
            for g in self._client.server.games
        )

    def _fill_menu(self) -> None:
        self.menu_items.clear()
        for game_class in GAME_CLASSES:
            ongoing_games = [
                g for g in self._client.server.games if isinstance(g, game_class)
            ]
            if ongoing_games:
                [game] = ongoing_games
                player_count = len(game.players)
            else:
                player_count = 0

            text = game_class.NAME
            if player_count == 1:
                text += " (1 player)"
            else:
                text += f" ({player_count} players)"
            self.menu_items.append(text)

        self.menu_items.append("Quit")

    def get_lines_to_render(self) -> list[bytes]:
        self._fill_menu()
        result = ASCII_ART.encode("ascii").split(b"\n") + super().get_lines_to_render()
        if self._should_show_cannot_join_error():
            result.append(b"")
            result.append(b"")
            result.append(
                (COLOR % 31) + b"This game is full.".center(80).rstrip() + (COLOR % 0)
            )
        return result

    def on_enter_pressed(self) -> bool:
        if self.menu_items[self.selected_index] == "Quit":
            return True

        if not self._should_show_cannot_join_error():
            self._client.view = CheckTerminalSizeView(
                self._client, GAME_CLASSES[self.selected_index]
            )
        return False


class CheckTerminalSizeView:
    def __init__(self, client: Client, game_class: type[Game]):
        self._client = client
        self._game_class = game_class

    def get_lines_to_render(self) -> list[bytes]:
        width = 80
        height = self._game_class.TERMINAL_HEIGHT_NEEDED

        text_lines = [
            b"Please adjust your terminal size so that you can",
            b"see the entire rectangle. Press Enter when done.",
        ]

        lines = [b"|" + b" " * (width - 2) + b"|"] * height
        lines[0] = lines[-1] = b"o" + b"-" * (width - 2) + b"o"
        for index, line in enumerate(text_lines):
            lines[2 + index] = b"|" + line.center(width - 2) + b"|"
            lines[-2 - len(text_lines) + index] = b"|" + line.center(width - 2) + b"|"

        return lines

    def handle_key_press(self, received: bytes) -> None:
        if received == b"\r":
            # rendering this view is a bit special :)
            #
            # Make sure screen clears before changing view, even if the next
            # view isn't actually as tall as this view. This can happen if a
            # game was full and you're thrown back to main menu.
            self._client.writer.write(CLEAR_SCREEN)
            self._client.server.start_game(self._client, self._game_class)


class GameOverView(MenuView):
    def __init__(
        self,
        client: Client,
        game: Game,
        new_high_score: HighScore,
    ):
        super().__init__()
        self.menu_items.extend(["New Game", "Choose a different game", "Quit"])
        self._client = client
        self.game = game
        self.new_high_score = new_high_score
        self._high_scores: list[HighScore] | None = None

    def set_high_scores(self, high_scores: list[HighScore]) -> None:
        self._high_scores = high_scores

    def get_lines_to_render(self) -> list[bytes]:
        if self._high_scores is None:
            return [b"", b"", b"Loading...".center(80).rstrip()]

        lines = [b"", b"", b""]
        lines.append(b"Game Over :(".center(80).rstrip())
        lines.append(
            f"Your score was {self.new_high_score.score}.".encode("ascii")
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
            if hs == self.new_high_score:
                lines.append((COLOR % 42) + line)
            else:
                lines.append((COLOR % 0) + line)

        lines.append(COLOR % 0)  # Needed if last score was highlighted
        return lines

    def on_enter_pressed(self) -> bool:
        if self._high_scores is None:
            return False

        text = self.menu_items[self.selected_index]
        if text == "New Game":
            assert self._client.name is not None
            self._client.server.start_game(self._client, type(self.game))
        elif text == "Choose a different game":
            self._client.view = ChooseGameView(self._client, type(self.game))
        elif text == "Quit":
            return True
        else:
            raise NotImplementedError(text)

        return False


def get_block_preview(shape_id: str) -> list[bytes]:
    if shape_id == "BOMB":
        return [(COLOR % BLOCK_COLORS["BOMB"]) + b"BOMB!!!" + (COLOR % 0)]

    points = BLOCK_SHAPES[shape_id]
    color_number = BLOCK_COLORS[shape_id]

    result = []
    for y in (-1, 0):
        row = b""
        color = False
        for x in (-2, -1, 0, 1):
            if (x, y) in points and not color:
                row += COLOR % color_number
                color = True
            elif (x, y) not in points and color:
                row += COLOR % 0
                color = False
            row += b"  "
        if color:
            row += COLOR % 0
        result.append(row)

    return result


class PlayingView:
    def __init__(self, client: Client, game: Game, player: Player):
        self._client = client
        self._server = client.server
        # no idea why these need explicit type annotations
        self.game: Game = game
        self.player: Player = player

    def get_lines_to_render(self) -> list[bytes]:
        lines = self.game.get_lines_to_render(self.player)
        lines[5] += f"  Score: {self.game.score}".encode("ascii")
        if self._client.rotate_counter_clockwise:
            lines[6] += b"  Counter-clockwise"

        lines[7] += b"  Next:"
        for index, row in enumerate(
            get_block_preview(self.player.next_shape_id), start=9
        ):
            lines[index] += b"   " + row
        if isinstance(self.player.moving_block_or_wait_counter, int):
            n = self.player.moving_block_or_wait_counter
            lines[14] += f"  Please wait: {n}".encode("ascii")
        return lines

    def handle_key_press(self, received: bytes) -> None:
        if received in (b"A", b"a", LEFT_ARROW_KEY):
            self.game.move_if_possible(self.player, dx=-1, dy=0, in_player_coords=True)
            self.player.set_fast_down(False)
        elif received in (b"D", b"d", RIGHT_ARROW_KEY):
            self.game.move_if_possible(self.player, dx=1, dy=0, in_player_coords=True)
            self.player.set_fast_down(False)
        elif received in (b"W", b"w", UP_ARROW_KEY, b"\r"):
            self.game.rotate(self.player, self._client.rotate_counter_clockwise)
            self.player.set_fast_down(False)
        elif received in (b"S", b"s", DOWN_ARROW_KEY, b" "):
            self.player.set_fast_down(True)
        elif received in (b"R", b"r"):
            self._client.rotate_counter_clockwise = (
                not self._client.rotate_counter_clockwise
            )
            self._client.render()
        elif (
            received in (b"F", b"f")
            and isinstance(self.game, RingGame)
            and len(self.game.players) == 1
        ):
            self.game.players[0].flip_view()
            if self.game.is_valid():
                self.game.need_render_event.set()
            else:
                # Can't flip, blocks are on top of each other. Flip again to undo.
                self.game.players[0].flip_view()


class Client:
    def __init__(
        self, server: Server, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        self.server = server
        self._reader = reader
        self.writer = writer
        self._recv_stats: collections.deque[tuple[float, int]] = collections.deque()

        self.last_displayed_lines: list[bytes] = []
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
            self.writer.write(
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

        # Hide user's key press at cursor location. Needs to be done at
        # whatever cursor location is currently, before we move it.
        to_send += b"\r"  # move cursor to start of line
        to_send += CLEAR_TO_END_OF_LINE

        for y, (old_line, new_line) in enumerate(
            zip(self.last_displayed_lines, lines), start=1
        ):
            # Re-rendering cursor line helps with AskNameView
            if old_line != new_line or y == cursor_pos[0]:
                to_send += MOVE_CURSOR % (y, 1)
                to_send += new_line
                to_send += CLEAR_TO_END_OF_LINE
        self.last_displayed_lines = lines.copy()

        to_send += MOVE_CURSOR % cursor_pos
        self.writer.write(to_send)

    async def _receive_bytes(self) -> bytes | None:
        await asyncio.sleep(0)  # Makes game playable while fuzzer is running
        try:
            result = await self._reader.read(100)
        except OSError as e:
            print("Receive error:", self.name, e)
            return None

        # Prevent 100% cpu usage if someone sends a lot of data
        now = time.monotonic()
        self._recv_stats.append((now, len(result)))
        while self._recv_stats and self._recv_stats[0][0] < now - 1:
            self._recv_stats.popleft()
        if sum(length for timestamp, length in self._recv_stats) > 2000:
            print("Received more than 2KB/sec, disconnecting:", self.name)
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
            return None

        return result

    async def handle(self) -> None:
        print("New connection")

        if len(self.server.clients) >= len(GAME_CLASSES) * len(PLAYER_COLORS):
            print("Sending server full message")
            self.writer.write(b"The server is full. Please try again later.\r\n")
            return
        self.server.clients.add(self)

        try:
            self.writer.write(CLEAR_SCREEN)
            received = b""

            while True:
                self.render()

                new_chunk = await self._receive_bytes()
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

        finally:
            print("Closing connection:", self.name)
            self.server.clients.remove(self)
            if isinstance(self.view, PlayingView) and isinstance(
                self.view.player.moving_block_or_wait_counter, MovingBlock
            ):
                self.view.player.moving_block_or_wait_counter = None
                self.view.game.need_render_event.set()

            # \r moves cursor to start of line
            self.writer.write(b"\r" + CLEAR_FROM_CURSOR_TO_END_OF_SCREEN + SHOW_CURSOR)
            try:
                await self.writer.drain()
            except OSError:
                pass
            self.writer.close()


async def main() -> None:
    my_server = Server()
    asyncio_server = await asyncio.start_server(my_server.handle_connection, port=12345)
    async with asyncio_server:
        print("Listening on port 12345...")
        await asyncio_server.serve_forever()


asyncio.run(main())
