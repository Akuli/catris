from __future__ import annotations

# "from foo import bar as bar" is a way of telling mypy it is exposed for other files
from .bottle import BottleGame as BottleGame
from .game_base_class import Game as Game
from .ring import RingGame as RingGame
from .traditional import TraditionalGame as TraditionalGame

GAME_CLASSES: list[type[Game]] = [TraditionalGame, BottleGame, RingGame]
