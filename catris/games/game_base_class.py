from __future__ import annotations

import asyncio
import copy
import time
from abc import abstractmethod
from typing import Any, Callable, ClassVar, Iterator

from catris.ansi import COLOR
from catris.player import MovingBlock, Player
from catris.squares import (
    BombSquare,
    DrillSquare,
    NormalSquare,
    Square,
    create_moving_squares,
)


class Game:
    NAME: ClassVar[str]
    ID: ClassVar[str]
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

        # This is assigned elsewhere after instantiating the game.
        # TODO: refactor?
        self.player_has_a_connected_client: Callable[[Player], bool]

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

    def get_duration_ns(self) -> int:
        assert not self.is_paused
        return time.monotonic_ns() - self._start_time - self._time_spent_in_pause

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

    def game_is_over(self) -> bool:
        return bool(self.players) and not any(
            isinstance(p.moving_block_or_wait_counter, MovingBlock)
            for p in self.players
        )

    def new_block(self, player: Player) -> None:
        assert player in self.players
        assert self.is_valid()
        squares = player.next_moving_squares
        player.next_moving_squares = create_moving_squares(self.score)

        # Convert to world coordinates. Rotations are needed to give the user
        # exactly what the next block display promises and to avoid subtle bugs.
        #
        # It's good to do this as late as possible, so that next_moving_squares
        # don't have to be updated if e.g. the player's view or spawning
        # coordinates change.
        for square in squares:
            square.offset_x, square.offset_y = player.player_to_world(
                square.offset_x, square.offset_y
            )
            square.x, square.y = player.player_to_world(square.x, square.y)
            square.x += player.moving_block_start_x
            square.y += player.moving_block_start_y

        player.moving_block_or_wait_counter = MovingBlock(squares)
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
            for block in self._get_moving_blocks().values()
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
    def get_existing_player_or_add_new_player(
        self, name: str, color: int
    ) -> Player | None:
        if not self.player_can_join(name):
            return None

        game_over = self.game_is_over()

        for player in self.players:
            if player.name.lower() == name.lower():
                # Let's say your caps lock was on accidentally and you type
                # "aKULI" as name when you intended to type "Akuli".
                # If that happens, you can leave the game and join back.
                player.name = name
                player.color = color
                break
        else:
            player = self.add_player(name, color)

        if not game_over and not isinstance(player.moving_block_or_wait_counter, int):
            self.new_block(player)
            self.need_render_event.set()
        return player

    def player_can_join(self, name: str) -> bool:
        return len(self.players) < self.MAX_PLAYERS or name.lower() in (
            p.name.lower() for p in self.players
        )

    # How many steps will the block move if user presses down arrow key?
    def _predict_how_far_block_lands(self, player: Player, block: MovingBlock) -> int:
        for offset in range(100):
            # Feels a bit hacky, but it's simple and it works
            old_squares = block.squares
            block.squares = {copy.copy(square) for square in block.squares}
            for square in block.squares:
                square.x -= player.up_x * offset
                square.y -= player.up_y * offset
                self.fix_moving_square(player, square)
            offset_works = self.is_valid()
            block.squares = old_squares

            if not offset_works:
                # Return the last offset that worked
                return offset - 1

        # Block won't land if you press down arrow. Happens in ring mode.
        return 0

    def get_square_texts(self) -> dict[tuple[int, int], bytes]:
        assert self.is_valid()

        result = {}

        for player, block in self._get_moving_blocks().items():
            offset = self._predict_how_far_block_lands(player, block)
            for square in block.squares:
                prediction = (
                    square.x - offset * player.up_x,
                    square.y - offset * player.up_y,
                )
                if prediction in self.valid_landed_coordinates:
                    result[prediction] = b"::"

        for square in self.landed_squares:
            result[square.x, square.y] = square.get_text(landed=True)
        for block in self._get_moving_blocks().values():
            for square in block.squares:
                result[square.x, square.y] = square.get_text(landed=False)

        for point, color in self.flashing_squares.items():
            if point in self.valid_landed_coordinates:
                result[point] = (COLOR % color) + b"  " + (COLOR % 0)

        return result

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
                    if not block.squares:
                        self.new_block(player)

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
                    if isinstance(square, BombSquare) and square.timer == 0
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

            for square in squares:
                if isinstance(square, DrillSquare):
                    square.picture_counter += 1
                    self.need_render_event.set()

    async def _please_wait_countdown(self, player: Player) -> None:
        assert isinstance(player.moving_block_or_wait_counter, int)

        while player.moving_block_or_wait_counter > 0:
            await self.pause_aware_sleep(1)
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
            await self.pause_aware_sleep(0.1)

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
                await self.pause_aware_sleep(0.025)
            else:
                await self.pause_aware_sleep(0.5 / (1 + self.score / 1000))
            await self._move_blocks_down_once(fast)
