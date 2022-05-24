use std::collections::HashMap;

struct SquareContent {
    ansi_colors: String,
    text: String,
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
            ansi_colors: "\x1b[1;43m".to_string(),
            text: "  ".to_string(),
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

const WIDTH: i8 = 10;
const HEIGHT: i8 = 20;

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
                    i32::from(*x) + player.block.center_x,
                    i32::from(*y) + player.block.center_y,
                );
                result.insert(
                    self.player_to_world(player_point),
                    player.block.get_square_contents(),
                );
            }
        }

        result
    }

    pub fn get_lines_to_render(&self) -> Vec<String> {
        let square_contents = self.get_square_contents();
        let mut result: Vec<String> = vec![];

        for y in 0..HEIGHT {
            let mut row = "|".to_string();
            let mut current_colors = "".to_string();

            for x in 0..WIDTH {
                let mut content = SquareContent {
                    ansi_colors: "".to_string(),
                    text: "  ".to_string(),
                };
                match square_contents.get(&(x, y)) {
                    Some(found_content) => {
                        content.ansi_colors = found_content.ansi_colors.clone();
                        content.text = found_content.text.clone();
                    }
                    _ => {}
                }
                if current_colors != content.ansi_colors {
                    row.push_str("\x1b[0m");
                    row.push_str(&content.ansi_colors);
                    current_colors = content.ansi_colors;
                }
                row.push_str(&content.text);
            }

            if current_colors != "" {
                row.push_str("\x1b[0m");
            }
            row.push_str("|");
            result.push(row);
        }
        result
    }
}
