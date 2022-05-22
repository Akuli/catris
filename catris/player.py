from __future__ import annotations

import dataclasses

from catris.squares import Square, create_moving_squares


@dataclasses.dataclass(eq=False)
class MovingBlock:
    squares_in_player_coords: dict[tuple[int, int], Square]
    fast_down: bool = False
    came_from_hold: bool = False


@dataclasses.dataclass(eq=False)
class Player:
    name: str
    color: int
    # What direction is up in the player's view? The up vector always has length 1.
    up_x: int
    up_y: int
    # These should be barely above the top of the game, in player coordinates.
    # For example, this is -1 in traditional game and bottle game.
    spawn_x: int
    spawn_y: int

    moving_block_or_wait_counter: MovingBlock | int = 0
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

    def set_fast_down(self, value: bool) -> None:
        if isinstance(self.moving_block_or_wait_counter, MovingBlock):
            self.moving_block_or_wait_counter.fast_down = value
