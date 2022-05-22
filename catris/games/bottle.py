from __future__ import annotations

from typing import Iterator

from catris.ansi import COLOR
from catris.player import Player
from catris.squares import BottleSeparatorSquare

from .game_base_class import Game
from .traditional import calculate_score


class BottleGame(Game):
    NAME = "Bottle game"
    ID = "bottle"

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

    def get_terminal_size(self) -> tuple[int, int]:
        return (self._get_width() * 2 + 2, 24)

    # Boundaries between bottles belong to neither neighbor player
    def square_belongs_to_player(self, player: Player, x: int, y: int) -> bool:
        i = self.players.index(player)
        left = self.BOTTLE_OUTER_WIDTH * i
        right = left + self.BOTTLE_INNER_WIDTH
        return x in range(left, right)

    def is_valid_moving_block_coords(self, player: Player, x: int, y: int) -> bool:
        return (x, max(0, y)) in self.valid_landed_coordinates

    def find_and_then_wipe_full_lines(self) -> Iterator[set[tuple[int, int]]]:
        if not self.players:
            # TODO: can this happen?
            yield set()
            return

        full_areas = []
        for y, row in enumerate(self.BOTTLE):
            if row.startswith(b"|") and row.endswith(b"|"):
                # Whole line
                points = {p for p in self.landed_squares.keys() if p[1] == y}
                if len(points) == self._get_width():
                    full_areas.append(points)
            else:
                # Player-specific parts
                for player in self.players:
                    points = {
                        (x, y)
                        for x in range(self._get_width())
                        if (x, y) in self.valid_landed_coordinates
                        and self.square_belongs_to_player(player, x, y)
                    }
                    if points.issubset(self.landed_squares.keys()):
                        full_areas.append(points)

        yield {point for point_set in full_areas for point in point_set}
        self.score += calculate_score(self, len(full_areas))

        # This loop must be in the correct order, top to bottom.
        for removed_points in full_areas:
            removed_y = list(removed_points)[0][1]
            removed_xs = {x for x, y in removed_points}
            self.landed_squares = {
                (x, (y + 1 if y < removed_y and x in removed_xs else y)): square
                for (x, y), square in self.landed_squares.items()
                if (x, y) not in removed_points
            }

        self.finish_wiping_full_lines()

    def _update_spawn_places_and_landed_coords(self) -> None:
        for i, player in enumerate(self.players):
            player.spawn_x = (i * self.BOTTLE_OUTER_WIDTH) + (
                self.BOTTLE_INNER_WIDTH // 2
            )

        self.valid_landed_coordinates = set()

        # Insides of bottles
        for i in range(len(self.players)):
            x_offset = self.BOTTLE_OUTER_WIDTH * i
            for y, row in enumerate(self.BOTTLE):
                for x in range(self.BOTTLE_INNER_WIDTH):
                    if row[2 * x + 1 : 2 * x + 3] == b"xx":
                        assert (x + x_offset, y) not in self.valid_landed_coordinates
                        self.valid_landed_coordinates.add((x + x_offset, y))

        # Walls between bottles
        for i in range(1, len(self.players)):
            x = (self.BOTTLE_OUTER_WIDTH * i) - 1
            for y, row in enumerate(self.BOTTLE):
                if row.startswith(b"|") and row.endswith(b"|"):
                    self.valid_landed_coordinates.add((x, y))

    def add_player(self, name: str, color: int) -> Player:
        if self.players:
            # Not the first player. Add squares to boundary.
            x = self.BOTTLE_OUTER_WIDTH * len(self.players) - 1
            for y, row in enumerate(self.BOTTLE):
                if row.startswith(b"|") and row.endswith(b"|"):
                    self.landed_squares[x, y] = BottleSeparatorSquare(
                        self.players[-1].color, color
                    )

        # spawn_x will be changed soon
        player = Player(name, color, up_x=0, up_y=-1, spawn_x=123, spawn_y=-1)
        self.players.append(player)
        self._update_spawn_places_and_landed_coords()
        self.new_block(player)
        return player

    def remove_player(self, player: Player) -> None:
        assert self.is_valid()

        left_wall_x = self.players.index(player) * self.BOTTLE_OUTER_WIDTH - 1
        right_wall_x = left_wall_x + self.BOTTLE_OUTER_WIDTH

        if player == self.players[0]:
            # Wipe wall on right side
            self.wipe_vertical_slice(0, self.BOTTLE_OUTER_WIDTH)
        elif player == self.players[-1]:
            # Wipe wall on left side
            self.wipe_vertical_slice(left_wall_x, self.BOTTLE_OUTER_WIDTH)
        else:
            # There's a wall on both sides of player. Combine the walls.
            left_neighbor = self.players[self.players.index(player) - 1]
            right_neighbor = self.players[self.players.index(player) + 1]

            for (x, y), square in list(self.landed_squares.items()):
                if x == left_wall_x:
                    self.landed_squares[right_wall_x, y] = square
                if x in (left_wall_x, right_wall_x) and isinstance(
                    square, BottleSeparatorSquare
                ):
                    square.left_color = left_neighbor.color
                    square.right_color = right_neighbor.color

            self.wipe_vertical_slice(left_wall_x, self.BOTTLE_OUTER_WIDTH)

        self.players.remove(player)
        self._update_spawn_places_and_landed_coords()
        assert self.is_valid()

    def get_lines_to_render(self, rendering_for_this_player: Player) -> list[bytes]:
        square_texts = self.get_square_texts(rendering_for_this_player)

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
