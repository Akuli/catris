use std::collections::HashMap;

use crate::ansi;
use crate::ansi::Color;
use crate::ansi::KeyPress;
use crate::lobby::ClientInfo;
use crate::render;

pub struct SquareContent {
    pub text: [char; 2],
    pub colors: Color,
}

// Relatively big ints in player coords because in ring mode they just grow as blocks wrap around.
pub type PlayerPoint = (i32, i32);
pub type WorldPoint = (i8, i8);

pub struct MovingBlock {
    pub center: PlayerPoint,
    pub relative_coords: Vec<PlayerPoint>,
}
impl MovingBlock {
    pub fn new(spawn_location: PlayerPoint) -> MovingBlock {
        MovingBlock {
            center: spawn_location,
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
    color: u8,
    pub block: MovingBlock,
}
impl Player {
    pub fn new(spawn_point: PlayerPoint, client_info: &ClientInfo) -> Player {
        Player {
            client_id: client_info.client_id,
            name: client_info.name.to_string(),
            color: client_info.color,
            block: MovingBlock::new(spawn_point),
        }
    }
}

pub trait Game {
    fn get_players(&mut self) -> &mut [Player];
    fn get_landed_squares(&mut self) -> &mut HashMap<WorldPoint, SquareContent>;
    fn add_player(&mut self, client_info: &ClientInfo);
    fn remove_player_if_exists(&mut self, client_id: u64);
    fn get_square_contents(&self) -> HashMap<(i8, i8), SquareContent>;
    fn world_to_player(&self, player_idx: usize, point: WorldPoint) -> (i32, i32);
    fn player_to_world(&self, player_idx: usize, point: PlayerPoint) -> (i8, i8);
    fn is_valid_moving_block_coords(&self, player_idx: usize, point: PlayerPoint) -> bool;
    fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool;
    fn square_belongs_to_player(&self, player_idx: usize, point: WorldPoint) -> bool;
    fn render_to_buf(&self, buffer: &mut render::Buffer);
}

fn new_block(game: &mut impl Game) {
    panic!("not impl");
}

pub fn wipe_vertical_slice(game: &mut impl Game) {
    panic!("not impl");
}

pub fn delete_points(game: &mut impl Game, points_to_delete: impl Iterator<Item = (i8, i8)>) {
    panic!("not impl");
}

pub fn move_blocks_down(game: &mut impl Game) {
    // TODO: rewrite the whole func
    for player in game.get_players() {
        for pair in &mut player.block.relative_coords {
            if pair.1 > 25 {
                pair.1 = -5;
            }
            pair.1 += 1;
        }
    }
}

pub fn handle_key_press(game: &mut impl Game, client_id: u64, key: KeyPress) -> bool {
    println!("Key Press!! {:?}", key);
    false
}
