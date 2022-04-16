from __future__ import annotations

import asyncio
import dataclasses
import sys
import time
from typing import TYPE_CHECKING

from catris.games import Game
from catris.views import GameOverView, PlayingView

if sys.version_info >= (3, 9):
    from asyncio import to_thread
else:
    import asyncio
    from typing import Any

    # copied from source code with slight modifications
    async def to_thread(func: Any, *args: Any, **kwargs: Any) -> Any:
        import contextvars
        import functools

        loop = asyncio.get_running_loop()
        ctx = contextvars.copy_context()
        func_call = functools.partial(ctx.run, func, *args, **kwargs)
        return await loop.run_in_executor(None, func_call)


if TYPE_CHECKING:
    from catris.server_and_client import Client


@dataclasses.dataclass
class HighScore:
    score: int
    duration_sec: float
    players: list[str]

    def get_duration_string(self) -> str:
        seconds = int(self.duration_sec)
        minutes = seconds // 60
        hours = minutes // 60

        if hours:
            return f"{hours}h"
        if minutes:
            return f"{minutes}min"
        return f"{seconds}sec"


def _add_high_score_sync(game_class: type[Game], hs: HighScore) -> list[HighScore]:
    high_scores = []
    try:
        with open("catris_high_scores.tsv", "r", encoding="utf-8") as file:
            first_line = file.readline()
            if first_line != "VERSION\t1\n":
                raise ValueError(f"unrecognized first line: {repr(first_line)}")

            for line in file:
                game_class_id, score, duration, *players = line.strip("\n").split("\t")
                if game_class_id == game_class.ID:
                    high_scores.append(
                        HighScore(
                            score=int(score),
                            duration_sec=float(duration),
                            players=players,
                        )
                    )
    except FileNotFoundError:
        print("Creating catris_high_scores.tsv")
        with open("catris_high_scores.tsv", "x", encoding="utf-8") as file:
            file.write("VERSION\t1\n")
    except (ValueError, OSError) as e:
        print("Reading catris_high_scores.tsv failed:", e)
        return [hs]  # do not write to file
    else:
        print("Adding score to catris_high_scores.tsv")

    try:
        with open("catris_high_scores.tsv", "a", encoding="utf-8") as file:
            print(
                game_class.ID,
                hs.score,
                hs.duration_sec,
                *hs.players,
                file=file,
                sep="\t",
            )
    except OSError as e:
        print("Writing to catris_high_scores.tsv failed:", e)

    high_scores.append(hs)
    high_scores.sort(key=(lambda hs: hs.score), reverse=True)
    return high_scores


_high_scores_lock = asyncio.Lock()


async def save_and_display_high_scores(game: Game, clients: list[Client]) -> None:
    duration_ns = time.monotonic_ns() - game.start_time
    new_high_score = HighScore(
        score=game.score,
        duration_sec=duration_ns / (1000 * 1000 * 1000),
        players=[p.name for p in game.players],
    )

    playing_clients = [
        c for c in clients if isinstance(c.view, PlayingView) and c.view.game == game
    ]
    if not playing_clients:
        print("Not adding high score because everyone disconnected")
        return

    for client in playing_clients:
        client.view = GameOverView(client, game, new_high_score)
        client.render()

    async with _high_scores_lock:
        high_scores = await to_thread(_add_high_score_sync, type(game), new_high_score)

    best5 = high_scores[:5]
    for client in clients:
        if isinstance(client.view, GameOverView) and client.view.game == game:
            client.view.set_high_scores(best5)
            client.render()
