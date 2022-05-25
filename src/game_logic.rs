use std::collections::HashMap;

use crate::ansi;
use crate::render::RenderBuffer;

struct SquareContent {
    text: [char; 2],
    colors: ansi::Colors,
}

pub struct MovingBlock {
    // Relatively big ints. In ring mode (not implemented yet) these just grow as the blocks wrap around.
    pub center_x: i32,
    pub center_y: i32,
    pub relative_coords: Vec<(i8, i8)>,
}

impl MovingBlock {
    fn get_square_contents(&self) -> SquareContent {
        SquareContent {
            text: [' ', ' '],
            colors: ansi::Colors { fg: 0, bg: 43 },
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
    pub fn move_blocks_down(&mut self) {
        for player in &mut self.players {
            for pair in &mut player.block.relative_coords {
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

    pub fn render_to_buf(&self, buffer: &mut RenderBuffer) {
        buffer.resize(2 * WIDTH + 2, HEIGHT);
        let square_contents = self.get_square_contents();

        for y in 0..HEIGHT {
            buffer.set_text(0, y, &mut "|".chars(), ansi::Colors { fg: 0, bg: 0 });
            buffer.set_text(
                2*WIDTH + 1,
                y,
                &mut "|".chars(),
                ansi::Colors { fg: 0, bg: 0 },
            );

            for x in 0..WIDTH {
                let upoint = (x as i8, y as i8);
                if let Some(content) = square_contents.get(&upoint) {
                    buffer.set_text(2 * x + 1, y, &mut content.text.into_iter(), content.colors);
                }
            }
        }
    }
}
