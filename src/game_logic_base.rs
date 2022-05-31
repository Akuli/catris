// TODO: rename this file
// TODO: put all game logics to folder
use std::collections::HashMap;

use crate::ansi;
use crate::lobby;
use crate::render;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum GameMode {
    Traditional,
    Bottle,
    Ring,
}
pub const ALL_GAME_MODES: &[GameMode] = &[GameMode::Traditional, GameMode::Bottle, GameMode::Ring];

impl GameMode {
    pub fn name(self) -> &'static str {
        match self {
            GameMode::Traditional => "Traditional game",
            GameMode::Bottle => "Bottle game",
            GameMode::Ring => "Ring game",
        }
    }

    pub fn max_players(self) -> usize {
        match self {
            GameMode::Traditional | GameMode::Bottle => lobby::MAX_CLIENTS_PER_LOBBY,
            GameMode::Ring => 4,
        }
    }
}

pub struct SquareContent {
    pub text: [char; 2],
    pub colors: ansi::Color,
}

pub struct MovingBlock {
    // Relatively big ints. In ring mode (not implemented yet) these just grow as the blocks wrap around.
    pub center_x: i32,
    pub center_y: i32,
    pub relative_coords: Vec<(i8, i8)>,
}

impl MovingBlock {
    pub fn new(player_index: usize) -> MovingBlock {
        MovingBlock {
            center_x: (10 * player_index + 5) as i32,
            center_y: -1,
            relative_coords: vec![(0, 0), (0, -1), (-1, 0), (-1, -1)],
        }
    }

    pub fn get_square_contents(&self) -> SquareContent {
        SquareContent {
            text: [' ', ' '],
            colors: ansi::YELLOW_BACKGROUND,
        }
    }
}

pub struct Player {
    pub client_id: u64,
    pub name: String,
    pub block: MovingBlock,
}
impl Player {
    pub fn new(client_id: u64, name: &str) -> Player {
        Player {
            client_id,
            name: name.to_string(),
            block: MovingBlock::new(0),
        }
    }
}

/*
pub fn remove_if_exists(&mut self, client_id: u64) -> bool {
    if let Some(i) = self
        .players
        .iter()
        .position(|info| info.client_id == client_id)
    {
        self.players.remove(i);
        true
    } else {
        false
    }
}*/

pub trait Game {
    fn add_player(&mut self, player: Player);
    fn remove_player_if_exists(&mut self, client_id: u64);
    fn player_count(&self) -> usize;
    fn get_square_contents(&self) -> HashMap<(i8, i8), SquareContent>;
    fn render_to_buf(&self, buffer: &mut render::Buffer);
    fn handle_key_press(&mut self, client_id: u64, key: ansi::KeyPress) -> bool;
}
