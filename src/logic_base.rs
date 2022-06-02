use std::collections::HashMap;

use crate::ansi;
use crate::ansi::Color;
use crate::ansi::KeyPress;
use crate::lobby::ClientInfo;
use crate::render;

#[derive(Copy, Clone)]
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

    fn get_player_coords(&self) -> Vec<PlayerPoint> {
        let (cx, cy) = self.center;
        self.relative_coords
            .iter()
            .map(|(dx, dy)| (cx + dx, cy + dy))
            .collect()
    }
}

pub struct Player {
    pub client_id: u64,
    pub name: String,
    color: u8,
    spawn_point: PlayerPoint,
    pub block: MovingBlock,
}
impl Player {
    pub fn new(spawn_point: PlayerPoint, client_info: &ClientInfo) -> Player {
        Player {
            client_id: client_info.client_id,
            name: client_info.name.to_string(),
            color: client_info.color,
            spawn_point: spawn_point,
            block: MovingBlock::new(spawn_point),
        }
    }
}

pub trait Game {
    fn get_players(&mut self) -> Vec<&mut Player>;
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

fn new_block(game: &mut impl Game, player_idx: usize) {
    println!("new block");
    let player = &mut game.get_players()[player_idx];
    player.block = MovingBlock::new(player.spawn_point);
    // TODO: start please wait countdown if there are overlaps
}

pub fn wipe_vertical_slice(_: &mut impl Game) {
    panic!("not impl");
}

pub fn delete_points(_: &mut impl Game, _: impl Iterator<Item = (i8, i8)>) {
    panic!("not impl");
}

pub fn move_blocks_down(game: &mut impl Game) {
    // TODO: possible to write without indexing?
    for player_idx in 0..game.get_players().len() {
        let new_relative_coords: Vec<PlayerPoint> = game.get_players()[player_idx]
            .block
            .relative_coords
            .iter()
            .map(|(x, y)| (*x, y + 1))
            .collect();
        let (spawn_x, spawn_y) = game.get_players()[player_idx].spawn_point;
        let new_player_coords: Vec<PlayerPoint> = new_relative_coords
            .iter()
            .map(|(dx, dy)| (spawn_x + dx, spawn_y + dy))
            .collect();
        let new_world_coords: Vec<WorldPoint> = new_player_coords
            .iter()
            .map(|p| game.player_to_world(player_idx, *p))
            .collect();

        if new_player_coords
            .iter()
            .all(|p| game.is_valid_moving_block_coords(player_idx, *p))
            && !new_world_coords
                .iter()
                .any(|p| game.get_landed_squares().contains_key(p))
        {
            game.get_players()[player_idx].block.relative_coords = new_relative_coords;
        } else {
            let player_coords = game.get_players()[player_idx].block.get_player_coords();
            for player_point in player_coords {
                let world_point = game.player_to_world(player_idx, player_point);
                let square_contents = game.get_players()[player_idx].block.get_square_contents();
                game.get_landed_squares()
                    .insert(world_point, square_contents);
            }
            new_block(game, player_idx);
        }
    }
}

pub fn handle_key_press(_: &mut impl Game, _client_id: u64, key: KeyPress) -> bool {
    println!("Key Press!! {:?}", key);
    false
}
