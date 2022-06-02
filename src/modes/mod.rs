use std::cell::RefCell;
use std::collections::HashMap;

use crate::ansi::KeyPress;
use crate::lobby::ClientInfo;
use crate::lobby::MAX_CLIENTS_PER_LOBBY;
use crate::logic_base::Game;
use crate::logic_base::Player;
use crate::logic_base::PlayerPoint;
use crate::logic_base::SquareContent;
use crate::logic_base::WorldPoint;
use crate::modes::traditional::TraditionalGame;
use crate::render;

mod traditional;

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
    pub fn new(mode: GameMode) -> AnyGame {
        match mode {
            GameMode::Traditional => AnyGame::Traditional(TraditionalGame::new()),
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
    fn get_players(&self) -> &[RefCell<Player>] {
        match self {
            AnyGame::Traditional(game) => game.get_players(),
        }
    }
    fn add_player(&mut self, client_info: &ClientInfo) {
        match self {
            AnyGame::Traditional(game) => game.add_player(client_info),
        }
    }
    fn remove_player_if_exists(&mut self, client_id: u64) {
        match self {
            AnyGame::Traditional(game) => game.remove_player_if_exists(client_id),
        }
    }
    fn get_square_contents(&self, exclude: Option<&Player>) -> HashMap<(i8, i8), SquareContent> {
        match self {
            AnyGame::Traditional(game) => game.get_square_contents(exclude),
        }
    }
    fn is_valid_moving_block_coords(&self, point: PlayerPoint) -> bool {
        match self {
            AnyGame::Traditional(game) => game.is_valid_moving_block_coords(point),
        }
    }
    fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool {
        match self {
            AnyGame::Traditional(game) => game.is_valid_landed_block_coords(point),
        }
    }
    fn square_belongs_to_player(&self, player_idx: usize, point: WorldPoint) -> bool {
        match self {
            AnyGame::Traditional(game) => game.square_belongs_to_player(player_idx, point),
        }
    }
    fn render_to_buf(&self, buffer: &mut render::Buffer) {
        match self {
            AnyGame::Traditional(game) => game.render_to_buf(buffer),
        }
    }
    fn get_landed_squares(&mut self) -> &mut HashMap<WorldPoint, SquareContent> {
        match self {
            AnyGame::Traditional(game) => game.get_landed_squares(),
        }
    }
}
