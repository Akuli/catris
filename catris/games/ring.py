from __future__ import annotations

import textwrap
from typing import Iterator

from catris.ansi import COLOR
from catris.player import Player
from catris.squares import Square

from .game_base_class import Game


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
