from __future__ import annotations

import copy
import random
from abc import abstractmethod

from catris.ansi import COLOR


class Square:
    def __init__(self) -> None:
        self.x = 0
        self.y = 0
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
    def __init__(self) -> None:
        super().__init__()
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

    # Do not rotate
    def rotate(self, counter_clockwise: bool) -> None:
        pass


def create_moving_squares(score: int) -> set[Square]:
    bomb_probability_as_percents = score / 800 + 1
    drill_probability_as_percents = score / 2000

    if random.uniform(0, 100) < bomb_probability_as_percents:
        center_square: Square = BombSquare()
        relative_coords = [(-1, 0), (0, 0), (0, -1), (-1, -1)]
    elif random.uniform(0, 100) < drill_probability_as_percents:
        center_square = DrillSquare()
        relative_coords = [(x, y) for x in (-1, 0) for y in range(1 - DRILL_HEIGHT, 1)]
    else:
        shape_letter = random.choice(list(BLOCK_SHAPES.keys()))
        center_square = NormalSquare(shape_letter)
        relative_coords = BLOCK_SHAPES[shape_letter]

    result = set()

    for x, y in relative_coords:
        square = copy.copy(center_square)
        square.x = x
        square.y = y
        square.offset_x = -x
        square.offset_y = -y
        if isinstance(square, DrillSquare):
            square.picture_x = 1 + x
            square.picture_y = DRILL_HEIGHT - 1 + y
        result.add(square)

    return result
