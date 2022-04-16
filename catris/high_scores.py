from __future__ import annotations

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


def _add_high_score_sync(file_name: str, hs: HighScore) -> list[HighScore]:
    high_scores = []
    try:
        with open(file_name, "r", encoding="utf-8") as file:
            for line in file:
                score, duration, *players = line.strip("\n").split("\t")
                high_scores.append(
                    HighScore(
                        score=int(score), duration_sec=float(duration), players=players
                    )
                )
    except FileNotFoundError:
        print("Creating", file_name)
    except (ValueError, OSError) as e:
        print(f"Reading {file_name} failed:", e)
    else:
        print("Found high scores file:", file_name)

    try:
        with open(file_name, "a", encoding="utf-8") as file:
            print(hs.score, hs.duration_sec, *hs.players, file=file, sep="\t")
    except OSError as e:
        print(f"Writing to {file_name} failed:", e)

    high_scores.append(hs)
    return high_scores


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

    high_scores = await to_thread(
        _add_high_score_sync, game.HIGH_SCORES_FILE, new_high_score
    )
    high_scores.sort(key=(lambda hs: hs.score), reverse=True)
    best5 = high_scores[:5]

    for client in clients:
        if isinstance(client.view, GameOverView) and client.view.game == game:
            client.view.set_high_scores(best5)
            client.render()
