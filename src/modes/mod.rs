mod traditional;
use crate::ansi::KeyPress;
use crate::lobby::MAX_CLIENTS_PER_LOBBY;
use crate::logic_base::Game;
use crate::logic_base::Player;
use crate::logic_base::SquareContent;
use crate::modes::traditional::TraditionalGame;
use crate::render;
use std::collections::HashMap;

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum GameMode {
    Traditional,
    Bottle,
    Ring,
}

impl GameMode {
    pub const ALL_MODES: &'static [GameMode] =
        &[GameMode::Traditional, GameMode::Bottle, GameMode::Ring];

    pub fn name(self) -> &'static str {
        match self {
            GameMode::Traditional => "Traditional game",
            GameMode::Bottle => "Bottle game",
            GameMode::Ring => "Ring game",
        }
    }

    pub fn max_players(self) -> usize {
        match self {
            GameMode::Traditional | GameMode::Bottle => MAX_CLIENTS_PER_LOBBY,
            GameMode::Ring => 4,
        }
    }
}

pub enum AnyGame {
    Traditional(TraditionalGame),
}

impl AnyGame {
    pub fn new(mode: GameMode, player: Player) -> AnyGame {
        match mode {
            GameMode::Traditional => AnyGame::Traditional(TraditionalGame::new(player)),
            _ => panic!("not implemented"),
        }
    }

    pub fn mode(&self) -> GameMode {
        match self {
            AnyGame::Traditional(_) => GameMode::Traditional,
        }
    }
}

impl Game for AnyGame {
    fn add_player(&mut self, player: Player) {
        match self {
            AnyGame::Traditional(g) => g.add_player(player),
        }
    }
    fn remove_player_if_exists(&mut self, client_id: u64) {
        match self {
            AnyGame::Traditional(g) => g.remove_player_if_exists(client_id),
        }
    }
    fn player_count(&self) -> usize {
        match self {
            AnyGame::Traditional(g) => g.player_count(),
        }
    }
    fn get_square_contents(&self) -> HashMap<(i8, i8), SquareContent> {
        match self {
            AnyGame::Traditional(g) => g.get_square_contents(),
        }
    }
    fn render_to_buf(&self, buffer: &mut render::Buffer) {
        match self {
            AnyGame::Traditional(g) => g.render_to_buf(buffer),
        }
    }
    fn handle_key_press(&mut self, client_id: u64, key: KeyPress) -> bool {
        match self {
            AnyGame::Traditional(g) => g.handle_key_press(client_id, key),
        }
    }
    fn move_blocks_down(&mut self) {
        match self {
            AnyGame::Traditional(g) => g.move_blocks_down(),
        }
    }
}
