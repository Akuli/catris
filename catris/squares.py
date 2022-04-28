from __future__ import annotations

import copy
import random
from abc import abstractmethod
from enum import Enum

from catris.ansi import COLOR


class _RotateMode(Enum):
    NO_ROTATING = 1
    ROTATE_90DEG_AND_BACK = 2
    FULL_ROTATING = 3


class Square:
    def __init__(self) -> None:
        self.x = 0
        self.y = 0
        # The offset is a vector from current position (x, y) to center of rotation
        self.offset_x = 0
        self.offset_y = 0
        # These don't change as the square moves down and lands.
        # Used in the hold feature, where an already moving block has to be respawned
        self.original_x = 0
        self.original_y = 0
        self.original_offset_x = 0
        self.original_offset_y = 0
        self.wrap_around_end = False  # for ring mode
        self.rotate_mode = _RotateMode.NO_ROTATING
        self._next_rotate_goes_backwards = False

    def _raw_rotate(self, counter_clockwise: bool) -> None:
        self.x += self.offset_x
        self.y += self.offset_y
        if counter_clockwise:
            self.offset_x, self.offset_y = self.offset_y, -self.offset_x
        else:
            self.offset_x, self.offset_y = -self.offset_y, self.offset_x
        self.x -= self.offset_x
        self.y -= self.offset_y

    def rotate(self, counter_clockwise: bool) -> None:
        if self.rotate_mode == _RotateMode.NO_ROTATING:
            pass
        elif self.rotate_mode == _RotateMode.ROTATE_90DEG_AND_BACK:
            self._next_rotate_goes_backwards = not self._next_rotate_goes_backwards
            self._raw_rotate(counter_clockwise=self._next_rotate_goes_backwards)
        elif self.rotate_mode == _RotateMode.FULL_ROTATING:
            self._raw_rotate(counter_clockwise)
        else:
            raise NotImplementedError(self.rotate_mode)

    @abstractmethod
    def get_text(self, landed: bool) -> bytes:
        raise NotImplementedError


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


class NormalSquare(Square):
    def __init__(self, shape_letter: str) -> None:
        super().__init__()
        self.shape_letter = shape_letter

    def get_text(self, landed: bool) -> bytes:
        return (COLOR % BLOCK_COLORS[self.shape_letter]) + b"  " + (COLOR % 0)


class BombSquare(Square):
    def __init__(self) -> None:
        super().__init__()
        self.timer = 15

    def get_text(self, landed: bool) -> bytes:
        # red middle text when bomb about to explode
        color = 31 if self.timer <= 3 else 33
        text = str(self.timer).center(2).encode("ascii")
        return (COLOR % color) + text + (COLOR % 0)


class BottleSeparatorSquare(Square):
    def __init__(self, left_color: int, right_color: int) -> None:
        super().__init__()
        self._left_color = left_color
        self._right_color = right_color

    def get_text(self, landed: bool) -> bytes:
        return (
            (COLOR % self._left_color)
            + b"|"
            + (COLOR % self._right_color)
            + b"|"
            + (COLOR % 0)
        )


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
    def __init__(self) -> None:
        super().__init__()
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


def _shapes_match_but_maybe_not_locations(a: set[tuple[int, int]], b: set[tuple[int, int]]):
    offset_x = min(x for x, y in b) - min(x for x, y in a)
    offset_y = min(y for x, y in b) - min(y for x, y in a)
    return {(x + offset_x, y + offset_y) for x, y in a} == b


# Not based on shape letter, because blocks can contain extra squares for the lolz
def _choose_rotate_mode(not_rotated: set[tuple[int, int]]) -> None:
    rotated_once = {(y, -x) for x, y in not_rotated}
    if _shapes_match_but_maybe_not_locations(not_rotated, rotated_once):
        return _RotateMode.NO_ROTATING
    rotated_twice = {(-x, -y) for x, y in not_rotated}
    if _shapes_match_but_maybe_not_locations(not_rotated, rotated_twice):
        return _RotateMode.ROTATE_90DEG_AND_BACK
    return _RotateMode.FULL_ROTATING


def _add_extra_square(relative_coords: list[tuple[int, int]]) -> None:
    while True:
        x, y = random.choice(relative_coords)
        offset_x, offset_y = random.choice([(-1, 0), (1, 0), (0, -1), (0, 1)])
        x += offset_x
        y += offset_y
        if (x, y) not in relative_coords:
            relative_coords.append((x, y))
            return


# Once extra square has been added, blocks can rotate wildly.
# This function adjusts the center of rotation to be in the center of mass.
def _fix_rotation_center(relative_coords: list[tuple[int, int]]) -> list[tuple[int, int]]:
    com_x = round(sum(x for x, y in relative_coords) / len(relative_coords))
    com_y = round(sum(y for x, y in relative_coords) / len(relative_coords))
    return [(x - com_x, y - com_y) for x, y in relative_coords]


def create_moving_squares(score: int) -> set[Square]:
    bomb_probability_as_percents = score / 800 + 1
    drill_probability_as_percents = score / 2000
    # Extra squares appear only with score>1000
    extra_square_probability_as_percents = (score - 1000) / 1500

    extra_square_probability_as_percents = 50

    rotate_mode = _RotateMode.NO_ROTATING
    if random.uniform(0, 100) < bomb_probability_as_percents:
        center_square: Square = BombSquare()
        relative_coords = [(-1, 0), (0, 0), (0, -1), (-1, -1)]
        rotate_mode = _RotateMode.NO_ROTATING
    elif random.uniform(0, 100) < drill_probability_as_percents:
        center_square = DrillSquare()
        relative_coords = [(x, y) for x in (-1, 0) for y in range(1 - DRILL_HEIGHT, 1)]
        rotate_mode = _RotateMode.NO_ROTATING
    else:
        shape_letter = random.choice(list(BLOCK_SHAPES.keys()))
        center_square = NormalSquare(shape_letter)
        relative_coords = BLOCK_SHAPES[shape_letter].copy()
        if random.uniform(0, 100) < extra_square_probability_as_percents:
            _add_extra_square(relative_coords)
            relative_coords = _fix_rotation_center(relative_coords)
        rotate_mode = _choose_rotate_mode(set(relative_coords))

    result = set()

    for x, y in relative_coords:
        square = copy.copy(center_square)
        square.x = x
        square.y = y
        square.original_x = x
        square.original_y = y
        square.offset_x = -x
        square.offset_y = -y
        square.original_offset_x = -x
        square.original_offset_y = -y
        square.rotate_mode = rotate_mode
        if isinstance(square, DrillSquare):
            square.picture_x = 1 + x
            square.picture_y = DRILL_HEIGHT - 1 + y
        result.add(square)

    return result
