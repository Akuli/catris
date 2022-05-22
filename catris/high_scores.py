from __future__ import annotations

import asyncio
import dataclasses
import io
import logging
import re
import sys
from typing import IO, TYPE_CHECKING, Iterator

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


CURRENT_VERSION = 3


def _upgrade_high_scores_file(file: IO[str], old_version: int) -> None:
    logging.info(f"Updating high scores file from v{old_version} to v{CURRENT_VERSION}")
    # multiple digits will be more difficult, if ever needed
    assert 1 <= old_version < CURRENT_VERSION <= 9

    file.seek(len(b"catris high scores file v"))
    file.write(str(CURRENT_VERSION))

    file.seek(0, io.SEEK_END)
    file.write(f"# --- upgraded from v{old_version} to v{CURRENT_VERSION} ---\n")


def _read_header_line(file: IO[str]) -> int:
    file.seek(0)
    first_line = file.readline(100)
    match = re.fullmatch(r"catris high scores file v([1-9])\n", first_line)
    if match is None:
        raise ValueError(f"unrecognized first line: {repr(first_line)}")

    version = int(match.group(1))
    if version > CURRENT_VERSION:
        raise ValueError(f"unsupported high scores file version: {version}")
    return version


def _read_high_scores(
    file: IO[str], game_class: type[Game], is_multiplayer: bool
) -> Iterator[HighScore]:
    for line in file:
        if line.startswith("#"):
            continue

        parts = line.strip("\n").split("\t")
        game_class_id, lobby_id, score, duration, *players = parts

        if game_class_id == game_class.ID and (len(players) >= 2) == is_multiplayer:
            yield HighScore(
                score=int(score), duration_sec=float(duration), players=players
            )


def _add_high_score_sync(
    game_class: type[Game], hs: HighScore, hs_lobby_id: str | None
) -> list[HighScore]:
    high_scores: list[HighScore] = []
    try:
        with open("catris_high_scores.txt", "r+", encoding="utf-8") as file:
            version = _read_header_line(file)
            if version < CURRENT_VERSION:
                _upgrade_high_scores_file(file, old_version=version)
                version = _read_header_line(file)
            assert version == CURRENT_VERSION
            high_scores.extend(
                _read_high_scores(
                    file, game_class, is_multiplayer=(len(hs.players) >= 2)
                )
            )
    except FileNotFoundError:
        _logger.info("Creating catris_high_scores.txt")
        with open("catris_high_scores.txt", "x", encoding="utf-8") as file:
            file.write(f"catris high scores file v{CURRENT_VERSION}\n")
    except Exception:
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
    if not game.players:
        _logger.info("Not adding high score because everyone left the game")
        return

    new_high_score = HighScore(
        score=game.score,
        duration_sec=game.get_duration_sec(),
        players=[p.name for p in game.players],
    )

    for client in lobby.clients:
        if isinstance(client.view, PlayingView) and client.view.game == game:
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
