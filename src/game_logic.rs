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

struct SquareContent {
    text: [char; 2],
    colors: ansi::Color,
}

pub struct MovingBlock {
    // Relatively big ints. In ring mode (not implemented yet) these just grow as the blocks wrap around.
    pub center_x: i32,
    pub center_y: i32,
    pub relative_coords: Vec<(i8, i8)>,
}

impl MovingBlock {
    fn new(player_index: usize) -> MovingBlock {
        MovingBlock {
            center_x: (10 * player_index + 5) as i32,
            center_y: -1,
            relative_coords: vec![(0, 0), (0, -1), (-1, 0), (-1, -1)],
        }
    }

    fn get_square_contents(&self) -> SquareContent {
        SquareContent {
            text: [' ', ' '],
            colors: ansi::YELLOW_BACKGROUND,
        }
    }
}

pub struct Player {
    pub name: String,
    pub block: MovingBlock,
}

pub struct Game {
    pub players: Vec<Player>,
}

const WIDTH: usize = 10;
const HEIGHT: usize = 20;

impl Game {
    pub fn new(player_name: &str) -> Game {
        let player = Player {
            name: player_name.to_string(),
            block: MovingBlock::new(0),
        };
        Game {
            players: vec![player],
        }
    }

    pub fn move_blocks_down(&mut self) {
        for player in &mut self.players {
            for pair in &mut player.block.relative_coords {
                // TODO: remove weird wrapping
                if pair.1 > 25 {
                    pair.1 = -5;
                }
                pair.1 += 1;
            }
        }
    }

    fn player_to_world(&self, player_point: (i32, i32)) -> (i8, i8) {
        let (x, y) = player_point;
        (i8::try_from(x).unwrap(), i8::try_from(y).unwrap())
    }

    fn get_square_contents(&self) -> HashMap<(i8, i8), SquareContent> {
        let mut result = HashMap::new();
        for player in &self.players {
            for (x, y) in &player.block.relative_coords {
                let player_point: (i32, i32) = (
                    (*x as i32) + player.block.center_x,
                    (*y as i32) + player.block.center_y,
                );
                result.insert(
                    self.player_to_world(player_point),
                    player.block.get_square_contents(),
                );
            }
        }

        result
    }

    pub fn render_to_buf(&self, buffer: &mut render::Buffer) {
        buffer.resize(2 * WIDTH + 2, HEIGHT);
        let square_contents = self.get_square_contents();

        for y in 0..HEIGHT {
            buffer.set_char(0, y, '|');
            buffer.set_char(2 * WIDTH + 1, y, '|');

            for x in 0..WIDTH {
                let upoint = (x as i8, y as i8);
                if let Some(content) = square_contents.get(&upoint) {
                    buffer.set_char_with_color(2 * x + 1, y, content.text[0], content.colors);
                    buffer.set_char_with_color(2 * x + 2, y, content.text[1], content.colors);
                }
            }
        }
    }
}
