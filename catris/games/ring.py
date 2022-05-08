from __future__ import annotations

import textwrap
from typing import Iterator

from catris.ansi import COLOR
from catris.player import Player
from catris.squares import Square

from .game_base_class import Game

MAP = b"""\
           .o--------------------------------------------------o.
         .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'.
       .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'.
     .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'.
   .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'.
 .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'.
oxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxo
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxo============oxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|wwwwwwwwwwww|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|aaaaaadddddd|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|aaaaaadddddd|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|aaaaaadddddd|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|ssssssssssss|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxo------------oxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|
oxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxo
 '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.'
   '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.'
     '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.'
       '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.'
         '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.'
           'o--------------------------------------------------o'
""".splitlines()

MIDDLE_AREA_RADIUS = 3
GAME_RADIUS = (len(MAP) - 2) // 2

# Playing area is actually 2*GAME_RADIUS + 1 in each direction.
assert max(line.count(b"xx") for line in MAP) == 2 * GAME_RADIUS + 1
assert len([line for line in MAP if b"xx" in line]) == 2 * GAME_RADIUS + 1


def wrap_names(players_by_letter: dict[str, Player]) -> dict[str, list[str]]:
    wrapped_names = {}

    for letter in "wasd":
        widths = [line.count(ord(letter)) for line in MAP if ord(letter) in line]
        if letter in players_by_letter:
            text = players_by_letter[letter].get_name_string(max_length=sum(widths))
        else:
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

    return wrapped_names


class RingGame(Game):
    NAME = "Ring game"
    ID = "ring"

    TERMINAL_WIDTH_NEEDED = max(len(row) for row in MAP) + 22
    TERMINAL_HEIGHT_NEEDED = len(MAP) + 1

    MAX_PLAYERS = 4

    def __init__(self) -> None:
        super().__init__()
        self.valid_landed_coordinates = {
            (x, y)
            for x in range(-GAME_RADIUS, GAME_RADIUS + 1)
            for y in range(-GAME_RADIUS, GAME_RADIUS + 1)
            if MAP[y + GAME_RADIUS + 1][2 * (x + GAME_RADIUS) + 1 :].startswith(b"xx")
        }

    def is_valid(self) -> bool:
        if not super().is_valid():
            return False

        for player, block in self._get_moving_blocks().items():
            for square in block.squares:
                player_x, player_y = player.world_to_player(square.x, square.y)
                if player_y < -GAME_RADIUS:
                    # Square above game. Treat it like the first row of map
                    player_y = -GAME_RADIUS
                if (player_x, player_y) not in self.valid_landed_coordinates:
                    return False
        return True

    # In ring mode, full lines are actually full squares, represented by radiuses.
    def find_and_then_wipe_full_lines(self) -> Iterator[set[Square]]:
        landed_locations = {(s.x, s.y) for s in self.landed_squares}
        full_radiuses = set(range(MIDDLE_AREA_RADIUS + 1, GAME_RADIUS + 1)) - {
            max(abs(x), abs(y))
            for x, y in (self.valid_landed_coordinates - landed_locations)
        }

        yield {
            square
            for square in self.landed_squares
            if max(abs(square.x), abs(square.y)) in full_radiuses
        }

        self.score += 100 * len(full_radiuses)

        for r in sorted(full_radiuses, reverse=True):
            self._delete_ring(r)

        self.finish_wiping_full_lines()

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
            y += GAME_RADIUS
            y %= 2 * GAME_RADIUS + 1
            y -= GAME_RADIUS
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
            moving_block_start_x=(GAME_RADIUS + 1) * up_x,
            moving_block_start_y=(GAME_RADIUS + 1) * up_y,
        )
        self.players.append(player)
        return player

    def get_lines_to_render(self, rendering_for_this_player: Player) -> list[bytes]:
        players_by_letter = {}
        colors_by_letter = {"w": 0, "a": 0, "s": 0, "d": 0}

        for player in self.players:
            relative_direction = rendering_for_this_player.world_to_player(
                player.up_x, player.up_y
            )
            letter = {(0, -1): "w", (-1, 0): "a", (0, 1): "s", (1, 0): "d"}[
                relative_direction
            ]
            players_by_letter[letter] = player
            colors_by_letter[letter] = player.color

        square_texts = self.get_square_texts(rendering_for_this_player)
        wrapped_names = wrap_names(players_by_letter)

        lines = []

        for y, map_row in enumerate(MAP, start=-GAME_RADIUS - 1):
            map_row = map_row.ljust(max(map(len, MAP)))

            result_line = b""
            map_x = 0
            while map_x < len(map_row):
                x = map_x // 2 - GAME_RADIUS
                if map_row.startswith(b"xx", map_x):
                    result_line += square_texts.get(
                        rendering_for_this_player.player_to_world(x, y), b"  "
                    )
                    map_x += 2
                elif map_row.startswith(b"=", map_x):
                    n = map_row.count(b"=")
                    result_line += (
                        (COLOR % colors_by_letter["w"]) + b"=" * n + (COLOR % 0)
                    )
                    map_x += n
                elif map_row.startswith(b"-", map_x) and 0 < y < 10:
                    n = map_row.count(b"-")
                    result_line += (
                        (COLOR % colors_by_letter["s"]) + b"-" * n + (COLOR % 0)
                    )
                    map_x += n
                elif abs(x) < 10 and map_row.startswith(b"|", map_x):
                    if x < 0:
                        result_line += (
                            (COLOR % colors_by_letter["a"]) + b"|" + (COLOR % 0)
                        )
                    else:
                        result_line += (
                            (COLOR % colors_by_letter["d"]) + b"|" + (COLOR % 0)
                        )
                    map_x += 1
                elif map_row.startswith((b"w", b"a", b"s", b"d"), map_x):
                    letter = chr(map_row[map_x])
                    result_line += COLOR % colors_by_letter[letter]
                    result_line += wrapped_names[letter].pop(0).encode("utf-8")
                    result_line += COLOR % 0
                    map_x += map_row.count(letter.encode("ascii"))
                else:
                    result_line += map_row[map_x : map_x + 1]
                    map_x += 1

            lines.append(result_line)

        return lines
