from .bottle import BottleGame
from .ring import RingGame
from .game_base_class import Game
from .traditional import TraditionalGame
GAME_CLASSES: list[type[Game]] = [TraditionalGame, BottleGame, RingGame]
