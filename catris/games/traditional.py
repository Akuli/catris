from __future__ import annotations

from typing import Iterator

from catris.ansi import COLOR
from catris.player import Player
from catris.squares import Square

from .game_base_class import Game


def calculate_score(game: Game, full_row_count: int) -> int:
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
    ID = "traditional"
    TERMINAL_HEIGHT_NEEDED = 24

    # Width varies as people join/leave. If you adjust these, please make sure
    # the game fits in 80 columns.
    HEIGHT = 20
    MAX_PLAYERS = 4

    def square_belongs_to_player(self, player: Player, x: int, y: int) -> bool:
        index = self.players.index(player)
        x_min = self._get_width_per_player() * index
        x_max = x_min + self._get_width_per_player()
        return x in range(x_min, x_max)

    def _get_width_per_player(self) -> int:
        if len(self.players) >= 2:
            # Each player has relatively narrow amount of room so we can fit
            # on 80 columns terminal. On windows with no putty installed, all
            # you have is 80 columns...
            return 7
        else:
            return 10

    def _get_width(self) -> int:
        return self._get_width_per_player() * len(self.players)

    def is_valid(self) -> bool:
        if self.players:
            assert self.valid_landed_coordinates == {
                (x, y) for x in range(self._get_width()) for y in range(self.HEIGHT)
            }

        return super().is_valid() and all(
            square.x in range(self._get_width()) and square.y < self.HEIGHT
            for block in self._get_moving_blocks().values()
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
        self.score += calculate_score(self, len(full_rows))

        for full_y, squares in sorted(full_rows.items()):
            self.landed_squares -= squares
            for square in self.landed_squares:
                if square.y < full_y:
                    square.y += 1

        self.finish_wiping_full_lines()

    def add_player(self, name: str, color: int) -> Player:
        self.players.append(
            Player(
                name,
                color,
                up_x=0,
                up_y=-1,
                moving_block_start_x=123,  # changed below
                moving_block_start_y=-1,
            )
        )

        # width per player might have changed, adjust spawning locations
        w = self._get_width_per_player()
        for i, player in enumerate(self.players):
            player.moving_block_start_x = (i * w) + (w // 2)

        for y in range(self.HEIGHT):
            for x in range(self._get_width()):
                self.valid_landed_coordinates.add((x, y))

        return player

    def get_lines_to_render(self, rendering_for_this_player: Player) -> list[bytes]:
        header_line = b"o"
        name_line = b" "
        name_length = 2 * self._get_width_per_player()

        for player in self.players:
            name_text = player.get_name_string(max_length=name_length)

            color_bytes = COLOR % player.color
            header_line += color_bytes
            name_line += color_bytes

            if player == rendering_for_this_player:
                header_line += b"=" * name_length
            else:
                header_line += b"-" * name_length
            name_line += name_text.center(name_length).encode("utf-8")

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
