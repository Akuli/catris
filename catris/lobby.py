from __future__ import annotations

import asyncio
import random
import string
from typing import TYPE_CHECKING, Container

from catris.games import Game
from catris.high_scores import save_and_display_high_scores
from catris.player import MovingBlock, Player
from catris.views import ChooseGameView, PlayingView

if TYPE_CHECKING:
    from catris.server_and_client import Client


def generate_lobby_id(ids_in_use: Container[str]) -> str:
    system_random = random.SystemRandom()
    while True:
        lobby_id = "".join(
            system_random.choice(string.ascii_uppercase + string.digits)
            for i in range(6)
        )
        if lobby_id not in ids_in_use:
            return lobby_id


_CLIENT_COLORS = {31, 32, 33, 34, 35, 36}
MAX_CLIENTS_PER_LOBBY = len(_CLIENT_COLORS)


class Lobby:

    # None is used when there's only one lobby that everyone joins by default
    def __init__(self, lobby_id: str | None) -> None:
        self.lobby_id = lobby_id
        self.games: dict[type[Game], Game] = {}
        self.clients: list[Client] = []

    # ChooseGameViews display a list of all players and how many are playing each game.
    # Call this method when any of that info changes.
    def _update_choose_game_views(self) -> None:
        for client in self.clients:
            if isinstance(client.view, ChooseGameView):
                client.render()

    @property
    def is_full(self) -> bool:
        return len(self.clients) == MAX_CLIENTS_PER_LOBBY

    def add_client(self, client: Client) -> None:
        client.log(f"Joining lobby: {self.lobby_id}")
        assert not self.is_full
        assert client not in self.clients
        assert client.name is not None
        assert client.lobby is None
        assert client.color is None
        client.color = min(_CLIENT_COLORS - {c.color for c in self.clients})
        self.clients.append(client)
        client.lobby = self
        self._update_choose_game_views()

    def remove_client(self, client: Client) -> None:
        client.log(f"Leaving lobby: {self.lobby_id}")
        assert client.lobby is self
        self.clients.remove(client)
        client.lobby = None

        if isinstance(client.view, PlayingView) and isinstance(
            client.view.player.moving_block_or_wait_counter, MovingBlock
        ):
            client.view.player.moving_block_or_wait_counter = None
            client.view.game.need_render_event.set()
        self._update_choose_game_views()

    def _player_has_a_connected_client(self, player: Player) -> bool:
        return any(
            isinstance(client.view, PlayingView) and client.view.player == player
            for client in self.clients
        )

    def start_game(self, client: Client, game_class: type[Game]) -> None:
        assert client in self.clients

        game = self.games.get(game_class)
        if game is None:
            game = game_class()
            game.player_has_a_connected_client = self._player_has_a_connected_client
            game.tasks.append(asyncio.create_task(self._render_task(game)))
            self.games[game_class] = game

        assert client.name is not None
        assert client.color is not None
        client.log(f"Joining a game with {len(game.players)} existing players: {game}")
        player = game.get_existing_player_or_add_new_player(client.name, client.color)
        if player is None:
            client.view = ChooseGameView(client, game_class)
        else:
            client.view = PlayingView(client, game, player)
        self._update_choose_game_views()

    async def _render_task(self, game: Game) -> None:
        while True:
            await game.need_render_event.wait()
            game.need_render_event.clear()
            self.render_game(game)

    def render_game(self, game: Game) -> None:
        assert game.is_valid()
        assert self.games[type(game)] == game
        game.tasks = [t for t in game.tasks if not t.done()]

        if game.game_is_over():
            del self.games[type(game)]
            for task in game.tasks:
                task.cancel()
            asyncio.create_task(save_and_display_high_scores(self, game))
        else:
            for client in self.clients:
                if isinstance(client.view, PlayingView) and client.view.game == game:
                    client.render()
        self._update_choose_game_views()