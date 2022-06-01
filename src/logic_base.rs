use std::collections::HashMap;

use crate::ansi;
use crate::ansi::Color;
use crate::ansi::KeyPress;
use crate::lobby;
use crate::render;

pub struct SquareContent {
    pub text: [char; 2],
    pub colors: Color,
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

pub trait Game {
    fn add_player(&mut self, player: Player);
    fn remove_player_if_exists(&mut self, client_id: u64);
    fn player_count(&self) -> usize;
    fn get_square_contents(&self) -> HashMap<(i8, i8), SquareContent>;
    fn render_to_buf(&self, buffer: &mut render::Buffer);
    fn move_blocks_down(&mut self);
    fn handle_key_press(&mut self, client_id: u64, key: KeyPress) -> bool;
}
