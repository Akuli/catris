mod traditional;
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
use impl_enum;
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

#[impl_enum::with_methods {
    pub fn get_players(&self) -> &Vec<Player> {}
    pub fn add_player(&mut self, client_info: &ClientInfo) {}
    pub fn remove_player_if_exists(&mut self, client_id: u64) {}
    pub fn get_square_contents(&self) -> HashMap<(i8, i8), SquareContent> {}
    pub fn world_to_player(&self, player_idx: usize, point: WorldPoint) -> (i32, i32) {}
    pub fn player_to_world(&self, player_idx: usize, point: PlayerPoint) -> (i8, i8) {}
    pub fn is_valid_moving_block_coords(&self, player_idx: usize, point: PlayerPoint) -> bool {}
    pub fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool {}
    pub fn square_belongs_to_player(&self, player_idx: usize, point: WorldPoint) -> bool {}
    pub fn render_to_buf(&self, buffer: &mut render::Buffer) {}
    pub fn move_blocks_down(&mut self) {}
    pub fn handle_key_press(&mut self, client_id: u64, key: KeyPress) -> bool {}
}]
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
