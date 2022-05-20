# TODO: prevent joining game if already quit twice? so that you can't "cheat" that way
from __future__ import annotations

import asyncio
import contextlib
import copy
import time
from abc import abstractmethod
from typing import Any, Callable, ClassVar, Generator, Iterator

from catris.ansi import COLOR
from catris.player import MovingBlock, Player
from catris.squares import BombSquare, DrillSquare, Square, create_moving_squares


def _player_has_a_drill(player: Player) -> bool:
    return isinstance(player.moving_block_or_wait_counter, MovingBlock) and any(
        isinstance(square, DrillSquare)
        for square in player.moving_block_or_wait_counter.squares
    )


class Game:
    NAME: ClassVar[str]
    ID: ClassVar[str]
    TERMINAL_WIDTH_NEEDED: ClassVar[int] = 80
    TERMINAL_HEIGHT_NEEDED: ClassVar[int]
    MAX_PLAYERS: ClassVar[int]

    def __init__(self) -> None:
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

        self._pause_event = asyncio.Event()
        self._unpause_event = asyncio.Event()
        self._unpause_event.set()
        self._start_time = time.monotonic_ns()
        self._time_spent_in_pause = 0
        self._last_pause_start = 0

        # Hold this when wiping full lines or exploding a bomb or similar.
        # Prevents moving blocks down and causing weird bugs.
        self.flashing_lock = asyncio.Lock()
        self.flashing_squares: dict[tuple[int, int], int] = {}

    @property
    def is_paused(self) -> bool:
        return self._pause_event.is_set()

    def toggle_pause(self) -> None:
        if self.is_paused:
            self._pause_event.clear()
            self._unpause_event.set()
            self._time_spent_in_pause += time.monotonic_ns() - self._last_pause_start
        else:
            self._pause_event.set()
            self._unpause_event.clear()
            self._last_pause_start = time.monotonic_ns()
        self.need_render_event.set()

    def get_duration_sec(self) -> float:
        end_time = self._last_pause_start if self.is_paused else time.monotonic_ns()
        duration_ns = end_time - self._start_time - self._time_spent_in_pause
        return duration_ns / (1000 * 1000 * 1000)

    async def pause_aware_sleep(self, sleep_time: float) -> None:
        while True:
            # Waiting while game is paused does not decrement sleep time
            await self._unpause_event.wait()

            start = time.monotonic()
            try:
                await asyncio.wait_for(self._pause_event.wait(), timeout=sleep_time)
            except asyncio.TimeoutError:
                # sleep completed without pausing
                return
            # Game was paused. Let's see how long we slept before that happened.
            unpaused_sleep_time = time.monotonic() - start
            sleep_time -= unpaused_sleep_time

    def _get_moving_blocks(self) -> dict[Player, MovingBlock]:
        return {
            player: player.moving_block_or_wait_counter
            for player in self.players
            if isinstance(player.moving_block_or_wait_counter, MovingBlock)
        }

    def _get_all_squares(self) -> set[Square]:
        return self.landed_squares | {
            square
            for block in self._get_moving_blocks().values()
            for square in block.squares
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

    # Inside this context manager, you can get the game to invalid state if you want.
    # All changes to blocks will be erased when you exit the context manager.
    @contextlib.contextmanager
    def temporary_state(self) -> Generator[None, None, None]:
        old_landed = self.landed_squares
        self.landed_squares = {copy.copy(s) for s in self.landed_squares}
        old_need_render = self.need_render_event.is_set()

        old_moving = []
        for block in self._get_moving_blocks().values():
            old_moving.append((block, block.squares))
            block.squares = {copy.copy(s) for s in block.squares}

        try:
            yield
        finally:
            self.landed_squares = old_landed
            for block, squares in old_moving:
                block.squares = squares
            if old_need_render:
                self.need_render_event.set()
            else:
                self.need_render_event.clear()

    def _apply_change_if_possible(self, callback: Callable[[], None]) -> bool:
        assert self.is_valid()
        with self.temporary_state():
            callback()
            stayed_valid = self.is_valid()
        if stayed_valid:
            callback()
            return True
        return False

    def game_is_over(self) -> bool:
        return not any(
            isinstance(p.moving_block_or_wait_counter, MovingBlock)
            for p in self.players
        )

    def new_block(self, player: Player, *, from_hold: bool = False) -> None:
        assert player in self.players
        assert self.is_valid()

        if from_hold:
            assert player.held_squares is not None
            squares = player.held_squares
        else:
            squares = player.next_moving_squares
            player.next_moving_squares = create_moving_squares(self.score)

        for square in squares:
            square.switch_to_world_coordinates(player)

        player.moving_block_or_wait_counter = MovingBlock(
            squares, came_from_hold=from_hold
        )
        if not self.is_valid():
            # New block overlaps with someone else's moving block
            self.start_please_wait_countdown(player)
        assert self.is_valid()
        self.need_render_event.set()

    def hold_block(self, player: Player) -> None:
        if (
            not isinstance(player.moving_block_or_wait_counter, MovingBlock)
            or player.moving_block_or_wait_counter.came_from_hold
        ):
            return

        to_hold = player.moving_block_or_wait_counter.squares
        self.new_block(player, from_hold=(player.held_squares is not None))
        for square in to_hold:
            square.restore_original_coordinates()
        player.held_squares = to_hold

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
            for block in self._get_moving_blocks().values()
            for square in block.squares
        }
        for square in self.landed_squares.copy():
            if (square.x, square.y) in bad_coords:
                self.landed_squares.remove(square)
            else:
                bad_coords.add((square.x, square.y))  # delete duplicates

        assert self.is_valid()

    def _move(
        self, player: Player, dx: int, dy: int, in_player_coords: bool, can_drill: bool
    ) -> None:
        if not isinstance(player.moving_block_or_wait_counter, MovingBlock):
            return

        if in_player_coords:
            dx, dy = player.player_to_world(dx, dy)

        assert isinstance(player.moving_block_or_wait_counter, MovingBlock)
        for square in player.moving_block_or_wait_counter.squares:
            square.x += dx
            square.y += dy
            self.fix_moving_square(player, square)

            if can_drill and isinstance(square, DrillSquare):
                square_sets = [self.landed_squares]
                for block in self._get_moving_blocks().values():
                    square_sets.append(block.squares)

                for square_set in square_sets:
                    for other_square in square_set.copy():
                        if (
                            other_square.x == square.x
                            and other_square.y == square.y
                            and not isinstance(other_square, DrillSquare)
                        ):
                            square_set.remove(other_square)

        self.need_render_event.set()

    def move_if_possible(
        self,
        player: Player,
        dx: int,
        dy: int,
        in_player_coords: bool,
        *,
        can_drill: bool = False,
    ) -> bool:
        return self._apply_change_if_possible(
            lambda: self._move(player, dx, dy, in_player_coords, can_drill)
        )

    # RingGame overrides this to get blocks to wrap back to top
    def fix_moving_square(self, player: Player, square: Square) -> None:
        pass

    def _rotate(self, player: Player, counter_clockwise: bool) -> None:
        if isinstance(player.moving_block_or_wait_counter, MovingBlock):
            for square in player.moving_block_or_wait_counter.squares:
                square.rotate(counter_clockwise)
                self.fix_moving_square(player, square)
            self.need_render_event.set()

    def rotate_if_possible(self, player: Player, counter_clockwise: bool) -> bool:
        return self._apply_change_if_possible(
            lambda: self._rotate(player, counter_clockwise)
        )

    @abstractmethod
    def add_player(self, name: str, color: int) -> Player:
        pass

    @abstractmethod
    def remove_player(self, player: Player) -> None:
        pass

    def wipe_vertical_slice(self, first_column: int, width: int) -> None:
        square_sets = [self.landed_squares]
        for block in self._get_moving_blocks().values():
            square_sets.append(block.squares)

        for square_set in square_sets:
            for square in square_set.copy():
                if first_column <= square.x < first_column + width:
                    square_set.remove(square)
                elif square.x >= first_column + width:
                    square.x -= width

    def _predict_landing_places(self, player: Player) -> set[tuple[int, int]]:
        if not isinstance(player.moving_block_or_wait_counter, MovingBlock):
            return set()

        with self.temporary_state():
            for i in range(40):  # enough even in ring mode
                coords = {
                    (s.x, s.y) for s in player.moving_block_or_wait_counter.squares
                }
                # _move() is faster than move_if_possible()
                self._move(player, dx=0, dy=1, in_player_coords=True, can_drill=True)
                if not self.is_valid():
                    # Can't move down anymore. This is where it will land
                    return coords
            # Block won't land if you press down arrow. Happens a lot in ring mode.
            return set()

    def get_square_texts(self, player: Player) -> dict[tuple[int, int], bytes]:
        assert self.is_valid()

        result = {}
        for square in self.landed_squares:
            result[square.x, square.y] = square.get_text(player, landed=True)
        for point in self._predict_landing_places(player):
            # "::" can go on top of landed blocks, useful for drills
            if point in result:
                result[point] = result[point].replace(b"  ", b"::")
            else:
                result[point] = b"::"
        for block in self._get_moving_blocks().values():
            for square in block.squares:
                result[square.x, square.y] = square.get_text(player, landed=False)
        for point, color in self.flashing_squares.items():
            result[point] = (COLOR % color) + b"  " + (COLOR % 0)

        return {
            point: text
            for point, text in result.items()
            if point in self.valid_landed_coordinates
        }

    @abstractmethod
    def get_lines_to_render(self, rendering_for_this_player: Player) -> list[bytes]:
        pass

    async def _explode_bombs(self, bombs: list[BombSquare]) -> list[BombSquare]:
        exploding_points = {
            (x, y)
            for x, y in self.valid_landed_coordinates
            for bomb in bombs
            if (x - bomb.x) ** 2 + (y - bomb.y) ** 2 < 3.5**2
        }
        explode_next = [
            square
            for square in self._get_all_squares()
            if isinstance(square, BombSquare)
            and (square.x, square.y) in exploding_points
            and square not in bombs
        ]

        if exploding_points:
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

        return explode_next

    async def _bomb_task(self) -> None:
        while True:
            await self.pause_aware_sleep(1)

            for square in self._get_all_squares():
                if isinstance(square, BombSquare):
                    square.timer -= 1

            async with self.flashing_lock:
                exploding_bombs = [
                    square
                    for square in self._get_all_squares()
                    if isinstance(square, BombSquare) and square.timer <= 0
                ]
                while exploding_bombs:
                    exploding_bombs = await self._explode_bombs(exploding_bombs)

            self.need_render_event.set()

    async def _drilling_task(self) -> None:
        while True:
            await self.pause_aware_sleep(0.1)
            squares = set()
            for block in self._get_moving_blocks().values():
                squares |= block.squares
            for player in self.players:
                squares |= player.next_moving_squares
                if player.held_squares is not None:
                    squares |= player.held_squares

            for square in squares:
                if isinstance(square, DrillSquare):
                    square.picture_counter += 1
                    self.need_render_event.set()

    async def _please_wait_countdown(self, player: Player) -> None:
        assert isinstance(player.moving_block_or_wait_counter, int)

        while player.moving_block_or_wait_counter > 0 and player in self.players:
            await self.pause_aware_sleep(1)
            assert isinstance(player.moving_block_or_wait_counter, int)
            player.moving_block_or_wait_counter -= 1
            self.need_render_event.set()

        if player not in self.players:
            # player quit
            return

        for square in self.landed_squares.copy():
            if self.square_belongs_to_player(player, square.x, square.y):
                self.landed_squares.remove(square)
        self.new_block(player)

    def start_please_wait_countdown(self, player: Player) -> None:
        # Get rid of moving block immediately to prevent invalid state after
        # adding a moving block that overlaps someone else's moving block.
        player.moving_block_or_wait_counter = 30
        self.need_render_event.set()
        self.tasks.append(asyncio.create_task(self._please_wait_countdown(player)))

    # Make sure to hold flashing_lock.
    # If you want to erase landed blocks, do that too while holding the lock.
    async def flash(self, points: set[tuple[int, int]], color: int) -> None:
        for display_color in [color, 0, color, 0]:
            for point in points:
                self.flashing_squares[point] = display_color
            self.need_render_event.set()
            await self.pause_aware_sleep(0.1)

        for point in points:
            try:
                del self.flashing_squares[point]
            except KeyError:
                # can happen with simultaneous overlapping flashes
                pass
        self.need_render_event.set()

    async def _move_blocks_down_once(self, fast: bool) -> None:
        # All moving squares can be drilled or bombed away
        for player, moving_block in self._get_moving_blocks().items():
            if not moving_block.squares:
                self.new_block(player)

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
            # Move drills last, makes them consistently drill other moving blocks
            for player in sorted(todo, key=_player_has_a_drill):
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

            if full_squares:
                self.need_render_event.set()
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
                await self.pause_aware_sleep(0.025)
            else:
                # I tried blocks_per_second = ax+b, where x is duration.
                # Games ended slowly, blocks coming fast and not much happening.
                blocks_per_second = 2 * 1.07 ** (self.get_duration_sec() / 60)
                await self.pause_aware_sleep(1 / blocks_per_second)
            await self._move_blocks_down_once(fast)
