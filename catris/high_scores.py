from __future__ import annotations

import asyncio
import dataclasses
import logging
import sys
from typing import TYPE_CHECKING

from catris.games import Game
from catris.views import GameOverView, PlayingView

if TYPE_CHECKING:
    from catris.lobby import Lobby

if sys.version_info >= (3, 9):
    from asyncio import to_thread
else:
    from typing import Any

    # copied from source code with slight modifications
    async def to_thread(func: Any, *args: Any, **kwargs: Any) -> Any:
        import contextvars
        import functools

        loop = asyncio.get_running_loop()
        ctx = contextvars.copy_context()
        func_call = functools.partial(ctx.run, func, *args, **kwargs)
        return await loop.run_in_executor(None, func_call)


_logger = logging.getLogger(__name__)


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
            return f"{hours}h {minutes - 60*hours}min"
        if minutes:
            return f"{minutes}min"
        return f"{seconds}sec"


def _add_high_score_sync(
    game_class: type[Game], hs: HighScore, hs_lobby_id: str | None
) -> list[HighScore]:
    high_scores = []
    try:
        with open("catris_high_scores.txt", "r", encoding="utf-8") as file:
            first_line = file.readline(100)
            if first_line != "catris high scores file v1\n":
                raise ValueError(f"unrecognized first line: {repr(first_line)}")

            for line in file:
                parts = line.strip("\n").split("\t")
                game_class_id, lobby_id, score, duration, *players = parts
                old_high_score_is_multiplayer = len(players) >= 2
                new_high_score_is_multiplayer = len(hs.players) >= 2

                # If new high score is from a multiplayer game, return multiplayer high scores.
                # If not, return single player high scores.
                if (
                    game_class_id == game_class.ID
                    and old_high_score_is_multiplayer == new_high_score_is_multiplayer
                ):
                    high_scores.append(
                        HighScore(
                            score=int(score),
                            duration_sec=float(duration),
                            players=players,
                        )
                    )
    except FileNotFoundError:
        _logger.info("Creating catris_high_scores.txt")
        with open("catris_high_scores.txt", "x", encoding="utf-8") as file:
            file.write("catris high scores file v1\n")
    except (ValueError, OSError):
        _logger.exception("Reading catris_high_scores.txt failed")
        return [hs]  # do not write to file
    else:
        _logger.info(f"Adding score to catris_high_scores.txt: {hs}")

    try:
        with open("catris_high_scores.txt", "a", encoding="utf-8") as file:
            # Currently lobby_id is not used for anything.
            # But I don't want to change the format if I ever need it for something...
            print(
                game_class.ID,
                hs_lobby_id or "-",
                hs.score,
                hs.duration_sec,
                *hs.players,
                file=file,
                sep="\t",
            )
    except OSError:
        _logger.exception("Writing to catris_high_scores.txt failed")

    high_scores.append(hs)
    high_scores.sort(key=(lambda hs: hs.score), reverse=True)
    return high_scores


_high_scores_lock = asyncio.Lock()


async def save_and_display_high_scores(lobby: Lobby, game: Game) -> None:
    playing_clients = [
        client
        for client in lobby.clients
        if isinstance(client.view, PlayingView) and client.view.game == game
    ]
    if not playing_clients:
        _logger.info("Not adding high score because everyone disconnected")
        return

    new_high_score = HighScore(
        score=game.score,
        duration_sec=game.get_duration_sec(),
        players=[p.name for p in game.players],
    )

    for client in playing_clients:
        client.view = GameOverView(client, game, new_high_score)
        client.render()

    async with _high_scores_lock:
        high_scores = await to_thread(
            _add_high_score_sync, type(game), new_high_score, lobby.lobby_id
        )

    best5 = high_scores[:5]
    for client in lobby.clients:
        if isinstance(client.view, GameOverView) and client.view.game == game:
            client.view.set_high_scores(best5)
            client.render()
