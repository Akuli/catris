from __future__ import annotations
import asyncio
import copy
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


class Square:
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y
        # The offset is a vector from current position (x, y) to center of rotation
        self.offset_x = 0
        self.offset_y = 0
        self.wrap_around_end = False  # for ring mode

    def rotate(self, counter_clockwise: bool) -> None:
        self.x += self.offset_x
        self.y += self.offset_y
        if counter_clockwise:
            self.offset_x, self.offset_y = self.offset_y, -self.offset_x
        else:
            self.offset_x, self.offset_y = -self.offset_y, self.offset_x
        self.x -= self.offset_x
        self.y -= self.offset_y

    @abstractmethod
    def get_text(self, landed: bool) -> bytes:
        raise NotImplementedError


class NormalSquare(Square):
    def __init__(self, x: int, y: int, shape_letter: str) -> None:
        super().__init__(x, y)
        self.shape_letter = shape_letter
        self.next_rotate_goes_backwards = False

    def get_text(self, landed: bool) -> bytes:
        return (COLOR % BLOCK_COLORS[self.shape_letter]) + b"  " + (COLOR % 0)

    def rotate(self, counter_clockwise: bool) -> None:
        if self.shape_letter == "O":
            return
        elif self.shape_letter in "ISZ":
            if self.next_rotate_goes_backwards:
                super().rotate(counter_clockwise=False)
            else:
                super().rotate(counter_clockwise=True)
            self.next_rotate_goes_backwards = not self.next_rotate_goes_backwards
        else:
            super().rotate(counter_clockwise)


class BombSquare(Square):
    def __init__(self, x: int, y: int) -> None:
        super().__init__(x, y)
        self.timer = 15

    def get_text(self, landed: bool) -> bytes:
        # red middle text when bomb about to explode
        color = 31 if self.timer <= 3 else 33
        text = str(self.timer).center(2).encode("ascii")
        return (COLOR % color) + text + (COLOR % 0)

    # Do not rotate
    def rotate(self, counter_clockwise: bool) -> None:
        pass


class BottleSeparatorSquare(Square):
    def __init__(self, x: int, y: int) -> None:
        super().__init__(x, y)

    def get_text(self, landed: bool) -> bytes:
        return b"||"


DRILL_HEIGHT = 5
DRILL_PICTURES = rb"""

| /|
|/ |
| .|
|. |
 \/

|/ |
| .|
|. |
| /|
 \/

| .|
|. |
| /|
|/ |
 \/

|. |
| /|
|/ |
| .|
 \/

"""


class DrillSquare(Square):
    def __init__(self, x: int, y: int) -> None:
        super().__init__(x, y)
        self.picture_x = 0
        self.picture_y = 0
        self.picture_counter = 0

    def get_text(self, landed: bool) -> bytes:
        picture_list = DRILL_PICTURES.strip().split(b"\n\n")
        picture = picture_list[self.picture_counter % len(picture_list)]
        start = 2 * self.picture_x
        end = 2 * (self.picture_x + 1)

        result = picture.splitlines()[self.picture_y].ljust(4)[start:end]
        if landed:
            return (COLOR % 100) + result + (COLOR % 0)
        return result

    # Do not rotate
    def rotate(self, counter_clockwise: bool) -> None:
        pass


def create_moving_squares(player: Player, score: int) -> set[Square]:
    bomb_probability_as_percents = score / 800 + 1
    drill_probability_as_percents = score / 2000

    if random.uniform(0, 100) < bomb_probability_as_percents:
        print("Adding special bomb block")
        center_square: Square = BombSquare(
            player.moving_block_start_x, player.moving_block_start_y
        )
        relative_coords = [(-1, 0), (0, 0), (0, -1), (-1, -1)]
    elif random.uniform(0, 100) < drill_probability_as_percents:
        center_square = DrillSquare(
            player.moving_block_start_x, player.moving_block_start_y
        )
        relative_coords = [(x, y) for x in (-1, 0) for y in range(1 - DRILL_HEIGHT, 1)]
    else:
        shape_letter = random.choice(list(BLOCK_SHAPES.keys()))
        center_square = NormalSquare(
            player.moving_block_start_x, player.moving_block_start_y, shape_letter
        )
        relative_coords = BLOCK_SHAPES[shape_letter]

    result = set()

    for player_x, player_y in relative_coords:
        # Orient initial block so that it always looks the same.
        # Otherwise may create subtle bugs near end of game, where freshly
        # added block overlaps with landed blocks.
        x, y = player.player_to_world(player_x, player_y)

        square = copy.copy(center_square)
        square.x = player.moving_block_start_x + x
        square.y = player.moving_block_start_y + y
        square.offset_x = -x
        square.offset_y = -y
        if isinstance(square, DrillSquare):
            square.picture_x = 1 + player_x
            square.picture_y = DRILL_HEIGHT - 1 + player_y
        result.add(square)

    return result


@dataclasses.dataclass(eq=False)
class MovingBlock:
    player: Player
    squares: set[Square]
    fast_down: bool = False


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

    def __post_init__(self) -> None:
        # score=0 is wrong when a new player joins an existing game.
        # But it's good enough and accessing the score from here is hard.
        self.next_moving_squares = create_moving_squares(self, score=0)

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

        flipping_squares = self.next_moving_squares.copy()
        if isinstance(self.moving_block_or_wait_counter, MovingBlock):
            flipping_squares |= self.moving_block_or_wait_counter.squares

        for square in flipping_squares:
            square.x *= -1
            square.y *= -1
            square.offset_x *= -1
            square.offset_y *= -1


class Game:
    NAME: ClassVar[str]
    HIGH_SCORES_FILE: ClassVar[str]
    TERMINAL_HEIGHT_NEEDED: ClassVar[int]
    MAX_PLAYERS: ClassVar[int]

    def __init__(self) -> None:
        self.start_time = time.monotonic_ns()
        self.players: list[Player] = []
        self.score = 0
        self.valid_landed_coordinates: set[tuple[int, int]] = set()
        self.landed_squares: set[Square] = set()
        self.tasks: list[asyncio.Task[Any]] = []
        self.tasks.append(asyncio.create_task(self._move_blocks_down_task(False)))
        self.tasks.append(asyncio.create_task(self._move_blocks_down_task(True)))
        self.tasks.append(asyncio.create_task(self._bomb_task()))
        self.tasks.append(asyncio.create_task(self._drilling_task()))
        self.need_render_event = asyncio.Event()
        self.player_has_a_connected_client: Callable[[Player], bool]  # set in Server

        # Hold this when wiping full lines or exploding a bomb or similar.
        # Prevents moving blocks down and causing weird bugs.
        self.flashing_lock = asyncio.Lock()
        self.flashing_squares: dict[tuple[int, int], int] = {}

    def _get_moving_blocks(self) -> list[MovingBlock]:
        return [
            player.moving_block_or_wait_counter
            for player in self.players
            if isinstance(player.moving_block_or_wait_counter, MovingBlock)
        ]

    def _get_all_squares(self) -> set[Square]:
        return self.landed_squares | {
            square for block in self._get_moving_blocks() for square in block.squares
        }

    def is_valid(self) -> bool:
        squares = self._get_all_squares()
        if len(squares) != len(set((square.x, square.y) for square in squares)):
            # print("Invalid state: duplicate squares")
            return False
        if not all(
            (square.x, square.y) in self.valid_landed_coordinates
            for square in self.landed_squares
        ):
            # print("Invalid state: landed squares outside valid area")
            return False
        return True

    def game_is_over(self) -> bool:
        return bool(self.players) and not any(
            isinstance(p.moving_block_or_wait_counter, MovingBlock)
            for p in self.players
        )

    def new_block(self, player: Player) -> None:
        assert self.is_valid()
        player.moving_block_or_wait_counter = MovingBlock(
            player, player.next_moving_squares
        )
        player.next_moving_squares = create_moving_squares(player, self.score)
        if not self.is_valid():
            # New block overlaps with someone else's moving block
            self.start_please_wait_countdown(player)
            assert self.is_valid()

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
    def find_and_then_wipe_full_lines(self) -> Iterator[set[Square]]:
        pass

    def finish_wiping_full_lines(self) -> None:
        # When landed blocks move, they can go on top of moving blocks.
        # This is quite rare, but results in invalid state errors.
        # When this happens, just delete the landed block.
        bad_coords = {
            (square.x, square.y)
            for block in self._get_moving_blocks()
            for square in block.squares
        }
        for square in self.landed_squares.copy():
            if (square.x, square.y) in bad_coords:
                self.landed_squares.remove(square)
            else:
                bad_coords.add((square.x, square.y))  # delete duplicates

        assert self.is_valid()

    def move_if_possible(
        self,
        player: Player,
        dx: int,
        dy: int,
        in_player_coords: bool,
        *,
        can_drill: bool = False,
    ) -> bool:
        assert self.is_valid()
        if in_player_coords:
            dx, dy = player.player_to_world(dx, dy)

        if isinstance(player.moving_block_or_wait_counter, MovingBlock):
            drilled = set()
            for square in player.moving_block_or_wait_counter.squares:
                square.x += dx
                square.y += dy
                self.fix_moving_square(player, square)

                if can_drill and isinstance(square, DrillSquare):
                    for other_square in self.landed_squares.copy():
                        if (
                            isinstance(other_square, NormalSquare)
                            and other_square.x == square.x
                            and other_square.y == square.y
                        ):
                            self.landed_squares.remove(other_square)
                            drilled.add(other_square)

            if self.is_valid():
                self.need_render_event.set()
                return True

            self.landed_squares |= drilled
            for square in player.moving_block_or_wait_counter.squares:
                square.x -= dx
                square.y -= dy
                self.fix_moving_square(player, square)
            assert self.is_valid()

        return False

    # RingGame overrides this to get blocks to wrap back to top
    def fix_moving_square(self, player: Player, square: Square) -> None:
        pass

    def rotate(self, player: Player, counter_clockwise: bool) -> None:
        if isinstance(player.moving_block_or_wait_counter, MovingBlock):
            for square in player.moving_block_or_wait_counter.squares:
                square.rotate(counter_clockwise)
                self.fix_moving_square(player, square)
            if not self.is_valid():
                for square in player.moving_block_or_wait_counter.squares:
                    square.rotate(not counter_clockwise)
                    self.fix_moving_square(player, square)

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
            self.new_block(player)
            self.need_render_event.set()
        return player

    def player_can_join(self, name: str) -> bool:
        return len(self.players) < self.MAX_PLAYERS or name.lower() in (
            p.name.lower() for p in self.players
        )

    def get_square_texts(self) -> dict[tuple[int, int], bytes]:
        assert self.is_valid()

        result = {}
        for square in self.landed_squares:
            result[square.x, square.y] = square.get_text(landed=True)
        for block in self._get_moving_blocks():
            for square in block.squares:
                result[square.x, square.y] = square.get_text(landed=False)

        for point, color in self.flashing_squares.items():
            if point in self.valid_landed_coordinates:
                result[point] = (COLOR % color) + b"  " + (COLOR % 0)

        return result

    @abstractmethod
    def get_lines_to_render(self, rendering_for_this_player: Player) -> list[bytes]:
        pass

    async def _bomb_task(self) -> None:
        while True:
            await asyncio.sleep(1)

            bombs: list[BombSquare] = [
                square
                for square in self._get_all_squares()
                if isinstance(square, BombSquare)
            ]

            exploding_points = set()
            for bomb in bombs:
                bomb.timer -= 1
                if bomb.timer == 0:
                    print("Bomb explodes! BOOOOOOMM!!!11!")

                    radius = 3.5
                    exploding_points |= {
                        (x, y)
                        for x, y in self.valid_landed_coordinates
                        if (x - bomb.x) ** 2 + (y - bomb.y) ** 2 < radius**2
                    }

            if exploding_points:
                async with self.flashing_lock:
                    await self.flash(exploding_points, 41)
                    for square in self.landed_squares.copy():
                        if (square.x, square.y) in exploding_points:
                            self.landed_squares.remove(square)
                    for player in self.players:
                        block = player.moving_block_or_wait_counter
                        if isinstance(block, MovingBlock):
                            for square in block.squares.copy():
                                if (square.x, square.y) in exploding_points:
                                    block.squares.remove(square)
                            if not block.squares:
                                self.new_block(player)

            if bombs:
                self.need_render_event.set()

    async def _drilling_task(self) -> None:
        while True:
            await asyncio.sleep(0.1)
            squares = set()
            for block in self._get_moving_blocks():
                squares |= block.squares
            for player in self.players:
                squares |= player.next_moving_squares

            for square in squares:
                if isinstance(square, DrillSquare):
                    square.picture_counter += 1
                    self.need_render_event.set()

    async def _please_wait_countdown(self, player: Player) -> None:
        assert isinstance(player.moving_block_or_wait_counter, int)

        while player.moving_block_or_wait_counter > 0:
            await asyncio.sleep(1)
            assert isinstance(player.moving_block_or_wait_counter, int)
            player.moving_block_or_wait_counter -= 1
            self.need_render_event.set()

        if self.player_has_a_connected_client(player):
            for square in self.landed_squares.copy():
                if self.square_belongs_to_player(player, square.x, square.y):
                    self.landed_squares.remove(square)
            self.new_block(player)
        else:
            player.moving_block_or_wait_counter = None

        self.need_render_event.set()

    def start_please_wait_countdown(self, player: Player) -> None:
        # Get rid of moving block immediately to prevent invalid state after
        # adding a moving block that overlaps someone else's moving block.
        player.moving_block_or_wait_counter = 10
        self.need_render_event.set()
        self.tasks.append(asyncio.create_task(self._please_wait_countdown(player)))

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
                moved = self.move_if_possible(
                    player, dx=0, dy=1, in_player_coords=True, can_drill=True
                )
                if moved:
                    something_moved = True
                    todo.remove(player)
            if not something_moved:
                break

        for player in todo:
            block = player.moving_block_or_wait_counter
            assert isinstance(block, MovingBlock)

            if block.fast_down:
                block.fast_down = False
            elif all(
                (square.x, square.y) in self.valid_landed_coordinates
                for square in block.squares
            ):
                self.landed_squares |= block.squares
                self.new_block(player)
            else:
                self.start_please_wait_countdown(player)

        async with self.flashing_lock:
            full_lines_iter = self.find_and_then_wipe_full_lines()
            full_squares = next(full_lines_iter)
            self.need_render_event.set()

            if full_squares:
                await self.flash({(s.x, s.y) for s in full_squares}, 47)
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


def calculate_traditional_score(game: Game, full_row_count: int) -> int:
    if full_row_count == 0:
        single_player_score = 0
    elif full_row_count == 1:
        single_player_score = 10
    elif full_row_count == 2:
        single_player_score = 30
    elif full_row_count == 3:
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
    n = len(game.players)
    if n == 0:  # avoid floats
        # TODO: does this ever happen?
        return 0
    return single_player_score * 2 ** (n - 1)


class TraditionalGame(Game):
    NAME = "Traditional game"
    HIGH_SCORES_FILE = "high_scores.txt"
    TERMINAL_HEIGHT_NEEDED = 24

    # Width varies as people join/leave. If you adjust these, please make sure
    # the game fits in 80 columns.
    HEIGHT = 20
    WIDTH_PER_PLAYER = 7
    MAX_PLAYERS = 4

    def square_belongs_to_player(self, player: Player, x: int, y: int) -> bool:
        index = self.players.index(player)
        x_min = self.WIDTH_PER_PLAYER * index
        x_max = x_min + self.WIDTH_PER_PLAYER
        return x in range(x_min, x_max)

    def _get_width(self) -> int:
        return self.WIDTH_PER_PLAYER * len(self.players)

    def is_valid(self) -> bool:
        if self.players:
            assert self.valid_landed_coordinates == {
                (x, y) for x in range(self._get_width()) for y in range(self.HEIGHT)
            }

        return super().is_valid() and all(
            square.x in range(self._get_width()) and square.y < self.HEIGHT
            for block in self._get_moving_blocks()
            for square in block.squares
        )

    def find_and_then_wipe_full_lines(self) -> Iterator[set[Square]]:
        full_rows = {}

        for y in range(self.HEIGHT):
            row = {square for square in self.landed_squares if square.y == y}
            if len(row) == self._get_width() and self._get_width() != 0:
                print("Clearing full row:", y)
                full_rows[y] = row

        yield {square for squares in full_rows.values() for square in squares}
        self.score += calculate_traditional_score(self, len(full_rows))

        for full_y, squares in sorted(full_rows.items()):
            self.landed_squares -= squares
            for square in self.landed_squares:
                if square.y < full_y:
                    square.y += 1

        self.finish_wiping_full_lines()

    def add_player(self, name: str, color: int) -> Player:
        x_min = len(self.players) * self.WIDTH_PER_PLAYER
        x_max = x_min + self.WIDTH_PER_PLAYER
        for y in range(self.HEIGHT):
            for x in range(x_min, x_max):
                assert (x, y) not in self.valid_landed_coordinates
                self.valid_landed_coordinates.add((x, y))

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


# TODO:
#   - color the names
#   - bigger bottom area
class BottleGame(Game):
    NAME = "Bottle game"
    HIGH_SCORES_FILE = "bottle_high_scores.txt"
    TERMINAL_HEIGHT_NEEDED = 24

    # Please make sure the game fits in 80 columns
    MAX_PLAYERS = 3
    BOTTLE = rb"""
xxxx|          |yyyy
xxxx|          |yyyy
xxxx|          |yyyy
xxxx|          |yyyy
xxxx|          |yyyy
xxxx/          \yyyy
xxx/.          .\yyy
xx|              |yy
xx|              |yy
xx|              |yy
xx|              |yy
xx|              |yy
x/.              .\y
/                  \
|                  |
|                  |
|                  |
|                  |
|                  |
|                  |
|                  |
|                  |
o------------------o
""".strip().splitlines()

    BOTTLE_INNER_WIDTH = 9
    BOTTLE_OUTER_WIDTH = 10

    def _get_width(self) -> int:
        # -1 at the end is the leftmost and rightmost "|" borders
        return self.BOTTLE_OUTER_WIDTH * len(self.players) - 1

    # Boundaries between bottles belong to neither neighbor player
    def square_belongs_to_player(self, player: Player, x: int, y: int) -> bool:
        i = self.players.index(player)
        left = self.BOTTLE_OUTER_WIDTH * i
        right = left + self.BOTTLE_INNER_WIDTH
        return x in range(left, right)

    def is_valid(self) -> bool:
        return super().is_valid() and all(
            (square.x, max(0, square.y)) in self.valid_landed_coordinates
            for block in self._get_moving_blocks()
            for square in block.squares
        )

    def find_and_then_wipe_full_lines(self) -> Iterator[set[Square]]:
        if not self.players:
            # TODO: can this happen?
            yield set()
            return

        full_areas = []
        for y, row in enumerate(self.BOTTLE):
            if row.startswith(b"o---") and row.endswith(b"---o"):
                continue

            if row.startswith(b"|") and row.endswith(b"|"):
                # Whole line
                squares = {
                    square
                    for square in self.landed_squares
                    if square.y == y
                }
                if len(squares) == self._get_width():
                    full_areas.append(squares)
            else:
                # Player-specific parts
                for player in self.players:
                    points = {
                        (x, y)
                        for x in range(self._get_width())
                        if (x, y) in self.valid_landed_coordinates
                        and self.square_belongs_to_player(player, x, y)
                    }
                    squares = {
                        square
                        for square in self.landed_squares
                        if (square.x, square.y) in points
                    }
                    if len(squares) == len(points):
                        full_areas.append(squares)

        yield {square for square_set in full_areas for square in square_set}
        self.score += calculate_traditional_score(self, len(full_areas))

        # This loop must be in the correct order, top to bottom.
        for removed_squares in full_areas:
            self.landed_squares -= removed_squares
            y = list(removed_squares)[0].y
            for landed in self.landed_squares:
                if landed.y < y:
                    landed.y += 1

        self.finish_wiping_full_lines()

    def add_player(self, name: str, color: int) -> Player:
        x_offset = self.BOTTLE_OUTER_WIDTH * len(self.players)
        for y, row in enumerate(self.BOTTLE):
            for x in range(self.BOTTLE_INNER_WIDTH):
                if row[2 * x + 1 : 2 * x + 3] == b"  ":
                    assert (x + x_offset, y) not in self.valid_landed_coordinates
                    self.valid_landed_coordinates.add((x + x_offset, y))

        if self.players:
            # Not the first player. Add squares to boundary.
            for y, row in enumerate(self.BOTTLE):
                if row.startswith(b"|") and row.endswith(b"|"):
                    self.landed_squares.add(BottleSeparatorSquare(x_offset - 1, y))
                    self.valid_landed_coordinates.add((x_offset - 1, y))

        player = Player(
            name,
            color,
            up_x=0,
            up_y=-1,
            moving_block_start_x=(
                len(self.players) * self.BOTTLE_OUTER_WIDTH
                + (self.BOTTLE_INNER_WIDTH // 2)
            ),
            moving_block_start_y=-1,
        )
        self.players.append(player)
        return player

    def get_lines_to_render(self, rendering_for_this_player: Player) -> list[bytes]:
        name_iterators = {}
        for player in self.players:
            for byte in b"xy":
                count = b"".join(self.BOTTLE).count(byte)
                # centering helps with displaying very short names
                name = player.get_name_string(max_length=count).center(4)
                name_iterators[player, byte] = iter(name)

        square_bytes = self.get_square_texts()

        result = []
        for y, bottle_row in enumerate(self.BOTTLE):
            repeated_row = bottle_row * len(self.players)

            # With multiple players, separators between bottles are Squares
            repeated_row = repeated_row.replace(b"||", b"  ")

            line = b""
            for index, bottle_byte in enumerate(repeated_row):
                if bottle_byte in b"xy":
                    player = self.players[index // len(bottle_row)]
                    iterator = name_iterators[player, bottle_byte]
                    try:
                        line += next(iterator).encode("utf-8")
                    except StopIteration:
                        line += b" "
                elif bottle_byte in b" ":
                    if index % 2 == 1:
                        x = index // 2
                        line += square_bytes.get((x, y), b"  ")
                else:
                    line += bytes([bottle_byte])
            result.append(line)

        return result


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

    MAX_PLAYERS = 4

    def __init__(self) -> None:
        super().__init__()
        self.valid_landed_coordinates = {
            (x, y)
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

    def is_valid(self) -> bool:
        if not super().is_valid():
            return False

        for block in self._get_moving_blocks():
            for square in block.squares:
                if max(abs(square.x), abs(square.y)) <= self.MIDDLE_AREA_RADIUS:
                    # print("Invalid state: moving block inside middle area")
                    return False
                player_x, player_y = block.player.world_to_player(square.x, square.y)
                if player_x < -self.GAME_RADIUS or player_x > self.GAME_RADIUS:
                    # print("Invalid state: moving block out of horizontal bounds")
                    return False
        return True

    # In ring mode, full lines are actually full squares, represented by radiuses.
    def find_and_then_wipe_full_lines(self) -> Iterator[set[Square]]:
        all_radiuses_with_duplicates = [
            max(abs(square.x), abs(square.y)) for square in self.landed_squares
        ]
        full_radiuses = [
            r
            for r in range(self.MIDDLE_AREA_RADIUS + 1, self.GAME_RADIUS + 1)
            if all_radiuses_with_duplicates.count(r) == 8 * r
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

        landed_squares_by_location = {
            (square.x, square.y): square for square in self.landed_squares
        }
        full_lines = []
        for dir_x, dir_y, points in lines:
            if all(p in landed_squares_by_location for p in points):
                squares = [landed_squares_by_location[p] for p in points]
                full_lines.append((dir_x, dir_y, squares))

        yield (
            {square for dx, dy, squares in full_lines for square in squares}
            | {
                square
                for square in self.landed_squares
                if max(abs(square.x), abs(square.y)) in full_radiuses
            }
        )

        self.score += 10 * len(full_lines) + 100 * len(full_radiuses)

        # Remove lines in order where removing first line doesn't mess up
        # coordinates of second, etc
        def sorting_key(line: tuple[int, int, list[Square]]) -> int:
            dir_x, dir_y, points = line
            square = squares[0]  # any square would do
            return dir_x * square.x + dir_y * square.y

        for dir_x, dir_y, squares in sorted(full_lines, key=sorting_key):
            self._delete_line(dir_x, dir_y, squares)
        for r in full_radiuses:  # must be in the correct order!
            self._delete_ring(r)

        self.finish_wiping_full_lines()

    def _delete_line(self, dir_x: int, dir_y: int, squares: list[Square]) -> None:
        # dot product describes where it is along the direction, and is same for all points
        # determinant describes where it is in the opposite direction
        point_and_dir_dot_product = dir_x * squares[0].x + dir_y * squares[0].y
        point_and_dir_determinants = [dir_y * s.x - dir_x * s.y for s in squares]

        self.landed_squares -= set(squares)
        for square in self.landed_squares:
            # If square aligns with the line and the direction points towards
            # the line, then move it
            if (
                dir_y * square.x - dir_x * square.y in point_and_dir_determinants
                and square.x * dir_x + square.y * dir_y < point_and_dir_dot_product
            ):
                square.x += dir_x
                square.y += dir_y

    def _delete_ring(self, r: int) -> None:
        for square in self.landed_squares.copy():
            # preserve squares inside the ring
            if max(abs(square.x), abs(square.y)) < r:
                continue

            # delete squares on the ring
            if max(abs(square.x), abs(square.y)) == r:
                self.landed_squares.remove(square)
                continue

            # Move towards center. Squares at a diagonal direction from center
            # have abs(x) == abs(y) move in two different directions.
            # Two squares can move into the same place. That's fine.
            move_left = square.x > 0 and abs(square.x) >= abs(square.y)
            move_right = square.x < 0 and abs(square.x) >= abs(square.y)
            move_up = square.y > 0 and abs(square.y) >= abs(square.x)
            move_down = square.y < 0 and abs(square.y) >= abs(square.x)
            if move_left:
                square.x -= 1
            if move_right:
                square.x += 1
            if move_up:
                square.y -= 1
            if move_down:
                square.y += 1

    def square_belongs_to_player(self, player: Player, x: int, y: int) -> bool:
        # Let me know if you need to understand how this works. I'll explain.
        dot = x * player.up_x + y * player.up_y
        return dot >= 0 and 2 * dot**2 >= x * x + y * y

    def fix_moving_square(self, player: Player, square: Square) -> None:
        x, y = player.world_to_player(square.x, square.y)

        # Moving blocks don't initially wrap, but they start wrapping once they
        # go below the midpoint
        if y > 0:
            square.wrap_around_end = True

        if square.wrap_around_end:
            y += self.GAME_RADIUS
            y %= 2 * self.GAME_RADIUS + 1
            y -= self.GAME_RADIUS
            square.x, square.y = player.player_to_world(x, y)

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
                line += square_bytes.get(
                    rendering_for_this_player.player_to_world(x, y), b"  "
                )

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


GAME_CLASSES: list[type[Game]] = [TraditionalGame, RingGame, BottleGame]


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
        self._client._send_bytes(HIDE_CURSOR)
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
            self._client._send_bytes(CLEAR_SCREEN)
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


def get_block_preview(player: Player) -> list[bytes]:
    squares_by_location = {
        player.world_to_player(square.x, square.y): square
        for square in player.next_moving_squares
    }
    min_x = min(x for x, y in squares_by_location.keys())
    min_y = min(y for x, y in squares_by_location.keys())
    max_x = max(x for x, y in squares_by_location.keys())
    max_y = max(y for x, y in squares_by_location.keys())

    result = []
    for y in range(min_y, max_y + 1):
        row = b""
        for x in range(min_x, max_x + 1):
            if (x, y) in squares_by_location:
                row += squares_by_location[x, y].get_text(landed=False)
            else:
                row += b"  "
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
        for index, row in enumerate(get_block_preview(self.player), start=9):
            lines[index] += b"   " + row
        if isinstance(self.player.moving_block_or_wait_counter, int):
            n = self.player.moving_block_or_wait_counter
            lines[16] += f"  Please wait: {n}".encode("ascii")
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
            self._send_bytes(
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
        self._send_bytes(to_send)

    def _send_bytes(self, b: bytes) -> None:
        self.writer.write(b)

        # Prevent filling the server's memory if client sends but never receives.
        # I don't use .drain() because one client's slowness shouldn't slow others.
        if self.writer.transport.get_write_buffer_size() > 64 * 1024:  # type: ignore
            print("More than 64K of data in send buffer, disconnecting:", self.name)
            self.writer.transport.close()

    async def _receive_bytes(self) -> bytes | None:
        await asyncio.sleep(0)  # Makes game playable while fuzzer is running

        if self.writer.transport.is_closing():
            return None

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

        try:
            if len(self.server.clients) >= sum(
                klass.MAX_PLAYERS for klass in GAME_CLASSES
            ):
                print("Sending server full message")
                self._send_bytes(b"The server is full. Please try again later.\r\n")
                return

            self.server.clients.add(self)
            self._send_bytes(CLEAR_SCREEN)
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
            self.server.clients.discard(self)
            if isinstance(self.view, PlayingView) and isinstance(
                self.view.player.moving_block_or_wait_counter, MovingBlock
            ):
                self.view.player.moving_block_or_wait_counter = None
                self.view.game.need_render_event.set()

            # \r moves cursor to start of line
            self._send_bytes(b"\r" + CLEAR_FROM_CURSOR_TO_END_OF_SCREEN + SHOW_CURSOR)

            try:
                await asyncio.wait_for(self.writer.drain(), timeout=3)
            except (OSError, asyncio.TimeoutError):
                pass
            self.writer.transport.close()


async def main() -> None:
    my_server = Server()
    asyncio_server = await asyncio.start_server(my_server.handle_connection, port=12345)
    async with asyncio_server:
        print("Listening on port 12345...")
        await asyncio_server.serve_forever()


asyncio.run(main())
