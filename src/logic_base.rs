use rand::seq::SliceRandom;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::ansi::Color;
use crate::ansi::KeyPress;
use crate::lobby::ClientInfo;
use crate::render;

#[derive(Copy, Clone)]
pub struct SquareContent {
    pub text: [char; 2],
    pub color: Color,
}

// Relatively big ints in player coords because in ring mode they just grow as blocks wrap around.
pub type PlayerPoint = (i32, i32);
pub type WorldPoint = (i8, i8);

#[rustfmt::skip]
const STANDARD_BLOCKS: &[(Color, &[PlayerPoint])] = &[
    // Colors from here: https://tetris.fandom.com/wiki/Tetris_Guideline
    // The white block should be orange, but that would mean using colors
    // that don't work on windows cmd (I hope nobody actually uses this on cmd though)
    (Color::WHITE_BACKGROUND, &[(-1, 0), (0, 0), (1, 0), (1, -1)]),
    (Color::CYAN_BACKGROUND, &[(-2, 0), (-1, 0), (0, 0), (1, 0)]),
    (Color::BLUE_BACKGROUND, &[(-1, -1), (-1, 0), (0, 0), (1, 0)]),
    (Color::YELLOW_BACKGROUND, &[(-1, 0), (0, 0), (0, -1), (-1, -1)]),
    (Color::PURPLE_BACKGROUND, &[(-1, 0), (0, 0), (1, 0), (0, -1)]),
    (Color::RED_BACKGROUND, &[(-1, -1), (0, -1), (0, 0), (1, 0)]),
    (Color::GREEN_BACKGROUND, &[(1, -1), (0, -1), (0, 0), (-1, 0)]),
];

#[derive(Debug)]
pub struct MovingBlock {
    pub center: PlayerPoint,
    pub relative_coords: Vec<PlayerPoint>,
    color: Color,
}
impl MovingBlock {
    pub fn new(spawn_location: PlayerPoint) -> MovingBlock {
        let (color, coords) = STANDARD_BLOCKS.choose(&mut rand::thread_rng()).unwrap();
        MovingBlock {
            center: spawn_location,
            color: *color,
            relative_coords: coords.to_vec(),
        }
    }

    pub fn get_square_contents(&self) -> SquareContent {
        SquareContent {
            text: [' ', ' '],
            color: self.color,
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
