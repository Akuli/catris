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

#[derive(Debug)]
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

    pub fn get_player_coords(&self) -> Vec<PlayerPoint> {
        let (cx, cy) = self.center;
        self.relative_coords
            .iter()
            .map(|(dx, dy)| (cx + dx, cy + dy))
            .collect()
    }
}

#[derive(Debug)]
pub struct Player {
    pub client_id: u64,
    pub name: String,
    pub color: u8,
    pub spawn_point: PlayerPoint,
    pub block: MovingBlock,
    pub fast_down: bool,
}
impl Player {
    pub fn new(spawn_point: PlayerPoint, client_info: &ClientInfo) -> Player {
        Player {
            client_id: client_info.client_id,
            name: client_info.name.to_string(),
            color: client_info.color,
            spawn_point: spawn_point,
            block: MovingBlock::new(spawn_point),
            fast_down: false,
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

    pub fn new_block(&mut self) {
        println!("new block");
        self.block = MovingBlock::new(self.spawn_point);
        // TODO: start please wait countdown if there are overlaps
    }
}
