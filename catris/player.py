from __future__ import annotations

import dataclasses

from catris.squares import Square, create_moving_squares


@dataclasses.dataclass(eq=False)
class MovingBlock:
    squares: set[Square]
    fast_down: bool = False
    came_from_hold: bool = False


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
    held_squares: set[Square] | None = None

    def __post_init__(self) -> None:
        # score=0 is wrong when a new player joins an existing game.
        # But it's good enough and accessing the score from here is hard.
        self.next_moving_squares = create_moving_squares(score=0)

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
        return ((-self.up_y * x + self.up_x * y), (-self.up_x * x - self.up_y * y))

    def player_to_world(self, x: int, y: int) -> tuple[int, int]:
        return ((-self.up_y * x - self.up_x * y), (self.up_x * x - self.up_y * y))

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
            for square in self.moving_block_or_wait_counter.squares:
                square.x *= -1
                square.y *= -1
                square.offset_x *= -1
                square.offset_y *= -1
