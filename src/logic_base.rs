use std::cell::RefCell;
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

    pub fn world_to_player(&self, point: WorldPoint) -> PlayerPoint {
        let (x, y) = point;
        (x as i32, y as i32)
    }

    pub fn player_to_world(&self, point: PlayerPoint) -> WorldPoint {
        let (x, y) = point;
        (x as i8, y as i8)
    }

    fn new_block(&mut self) {
        println!("new block");
        self.block = MovingBlock::new(self.spawn_point);
        // TODO: start please wait countdown if there are overlaps
    }
}

pub trait Game {
    // players wrapped in RefCell to allow mutating players and accessing game simultaneously
    fn get_players(&self) -> &[RefCell<Player>];
    fn get_landed_squares(&mut self) -> &mut HashMap<WorldPoint, SquareContent>;
    fn add_player(&mut self, client_info: &ClientInfo);
    fn remove_player_if_exists(&mut self, client_id: u64);
    fn get_square_contents(&self, exclude: Option<&Player>) -> HashMap<(i8, i8), SquareContent>;
    fn is_valid_moving_block_coords(&self, point: PlayerPoint) -> bool;
    fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool;
    fn square_belongs_to_player(&self, player_idx: usize, point: WorldPoint) -> bool;
    fn render_to_buf(&self, buffer: &mut render::Buffer);
}

pub fn wipe_vertical_slice(_: &mut impl Game) {
    panic!("not impl");
}

pub fn delete_points(_: &mut impl Game, _: impl Iterator<Item = (i8, i8)>) {
    panic!("not impl");
}

pub fn move_blocks_down(game: &mut impl Game) {
    let square_contents = game.get_square_contents(None); // FIXME
    let mut landing = vec![];

    for player in game.get_players() {
        let new_relative_coords: Vec<PlayerPoint> = player
            .borrow()
            .block
            .relative_coords
            .iter()
            .map(|(x, y)| (*x, y + 1))
            .collect();
        let (spawn_x, spawn_y) = player.borrow().spawn_point;
        let new_player_coords: Vec<PlayerPoint> = new_relative_coords
            .iter()
            .map(|(dx, dy)| (spawn_x + dx, spawn_y + dy))
            .collect();

        let can_move = new_player_coords.iter().all(|p| {
            let stays_in_bounds = game.is_valid_moving_block_coords(*p);
            let goes_on_top_of_something =
                square_contents.contains_key(&player.borrow().player_to_world(*p));
            if !stays_in_bounds {
                println!("out uf bounds");
            }
            if goes_on_top_of_something {
                println!("goes on top of something");
            }
            stays_in_bounds && !goes_on_top_of_something
        });

        if can_move {
            player.borrow_mut().block.relative_coords = new_relative_coords;
        } else {
            let player_coords = player.borrow().block.get_player_coords();
            for player_point in player_coords {
                let world_point = player.borrow().player_to_world(player_point);
                let square_contents = player.borrow().block.get_square_contents();
                landing.push((world_point, square_contents));
            }
            player.borrow_mut().new_block();
        }
    }

    game.get_landed_squares().extend(landing);
}

pub fn handle_key_press(_: &mut impl Game, _client_id: u64, key: KeyPress) -> bool {
    println!("Key Press!! {:?}", key);
    false
}
