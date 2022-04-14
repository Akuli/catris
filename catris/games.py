from __future__ import annotations

import asyncio
import textwrap
import time
from abc import abstractmethod
from typing import Any, Callable, ClassVar, Iterator

from catris.ansi import COLOR
from catris.player import MovingBlock, Player
from catris.squares import (
    BombSquare,
    BottleSeparatorSquare,
    DrillSquare,
    NormalSquare,
    Square,
    create_moving_squares,
)

PLAYER_COLORS = {31, 32, 33, 34}


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
                await asyncio.sleep(0.025)
            else:
                await asyncio.sleep(0.5 / (1 + self.score / 1000))
            await self._move_blocks_down_once(fast)


def _calculate_traditional_score(game: Game, full_row_count: int) -> int:
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
    result: int = single_player_score * 2 ** (n - 1)
    return result


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
        self.score += _calculate_traditional_score(self, len(full_rows))

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
        square_texts = self.get_square_texts()

        for y in range(self.HEIGHT):
            line = b"|"
            for x in range(self._get_width()):
                line += square_texts.get((x, y), b"  ")
            line += b"|"
            lines.append(line)

        lines.append(b"o" + b"--" * self._get_width() + b"o")
        return lines


class BottleGame(Game):
    NAME = "Bottle game"
    HIGH_SCORES_FILE = "bottle_high_scores.txt"
    TERMINAL_HEIGHT_NEEDED = 24

    # Please make sure the game fits in 80 columns
    MAX_PLAYERS = 3
    BOTTLE = [
        rb"    |xxxxxxxxxx|    ",
        rb"    |xxxxxxxxxx|    ",
        rb"    |xxxxxxxxxx|    ",
        rb"    |xxxxxxxxxx|    ",
        rb"    /xxxxxxxxxx\    ",
        rb"   /.xxxxxxxxxx.\   ",
        rb"  /xxxxxxxxxxxxxx\  ",
        rb" /.xxxxxxxxxxxxxx.\ ",
        rb"/xxxxxxxxxxxxxxxxxx\ ".rstrip(),  # python syntax weirdness
        rb"|xxxxxxxxxxxxxxxxxx|",
        rb"|xxxxxxxxxxxxxxxxxx|",
        rb"|xxxxxxxxxxxxxxxxxx|",
        rb"|xxxxxxxxxxxxxxxxxx|",
        rb"|xxxxxxxxxxxxxxxxxx|",
        rb"|xxxxxxxxxxxxxxxxxx|",
        rb"|xxxxxxxxxxxxxxxxxx|",
        rb"|xxxxxxxxxxxxxxxxxx|",
        rb"|xxxxxxxxxxxxxxxxxx|",
        rb"|xxxxxxxxxxxxxxxxxx|",
        rb"|xxxxxxxxxxxxxxxxxx|",
        rb"|xxxxxxxxxxxxxxxxxx|",
    ]

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
            if row.startswith(b"|") and row.endswith(b"|"):
                # Whole line
                squares = {square for square in self.landed_squares if square.y == y}
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
        self.score += _calculate_traditional_score(self, len(full_areas))

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
                if row[2 * x + 1 : 2 * x + 3] == b"xx":
                    assert (x + x_offset, y) not in self.valid_landed_coordinates
                    self.valid_landed_coordinates.add((x + x_offset, y))

        if self.players:
            # Not the first player. Add squares to boundary.
            for y, row in enumerate(self.BOTTLE):
                if row.startswith(b"|") and row.endswith(b"|"):
                    self.landed_squares.add(
                        BottleSeparatorSquare(
                            x_offset - 1, y, self.players[-1].color, color
                        )
                    )
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
        square_texts = self.get_square_texts()

        result = []
        for y, bottle_row in enumerate(self.BOTTLE):
            repeated_row = bottle_row * len(self.players)

            # With multiple players, separators between bottles are already in square_texts
            repeated_row = repeated_row.replace(b"||", b"xx")

            line = b""
            color = 0

            for index, bottle_byte in enumerate(repeated_row):
                if bottle_byte in b"x":
                    if index % 2 == 0:
                        continue
                    if color != 0:
                        line += COLOR % 0
                        color = 0
                    line += square_texts.get((index // 2, y), b"  ")
                else:
                    player = self.players[index // len(bottle_row)]
                    if color != player.color:
                        line += COLOR % player.color
                        color = player.color
                    line += bytes([bottle_byte])

            if color != 0:
                line += COLOR % 0
            result.append(line)

        bottom_line = b""
        name_line = b""
        for player in self.players:
            bottom_line += COLOR % player.color
            name_line += COLOR % player.color

            bottom_line += b"o"
            if player == rendering_for_this_player:
                bottom_line += b"==" * self.BOTTLE_INNER_WIDTH
            else:
                bottom_line += b"--" * self.BOTTLE_INNER_WIDTH
            bottom_line += b"o"

            name_text = player.get_name_string(max_length=2 * self.BOTTLE_OUTER_WIDTH)
            name_line += name_text.center(2 * self.BOTTLE_OUTER_WIDTH).encode("utf-8")

        result.append(bottom_line + (COLOR % 0))
        result.append(name_line + (COLOR % 0))
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
            letter = {(0, -1): "w", (-1, 0): "a", (0, 1): "s", (1, 0): "d"}[
                relative_direction
            ]
            players_by_letter[letter] = player

        middle_area_content = self._get_middle_area_content(players_by_letter)
        square_texts = self.get_square_texts()

        for y in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1):
            insert_middle_area_here = None
            line = b"|"
            for x in range(-self.GAME_RADIUS, self.GAME_RADIUS + 1):
                if max(abs(x), abs(y)) <= self.MIDDLE_AREA_RADIUS:
                    insert_middle_area_here = len(line)
                    continue
                line += square_texts.get(
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


GAME_CLASSES: list[type[Game]] = [TraditionalGame, BottleGame, RingGame]
