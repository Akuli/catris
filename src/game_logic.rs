// TODO: rename to something like game_modes/traditional.rs
use std::collections::HashMap;

use crate::ansi;
use crate::game_logic_base;
use crate::game_logic_base::Game;
use crate::game_logic_base::GameMode;
use crate::game_logic_base::Player;
use crate::lobby;
use crate::render;

const WIDTH: usize = 10;
const HEIGHT: usize = 20;

pub struct TraditionalGame {
    players: Vec<game_logic_base::Player>,
}

impl TraditionalGame {
    pub fn new(first_player: Player) -> TraditionalGame {
        TraditionalGame {
            players: vec![first_player],
        }
    }

    fn get_width_per_player(&self) -> usize {
        // TODO: 10 would be wide enough for two
        if self.players.len() >= 2 {
            7
        } else {
            10
        }
    }
    fn get_width(&self) -> usize {
        self.get_width_per_player() * self.players.len()
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
        (x as i8, y as i8)
    }
}

impl game_logic_base::Game for TraditionalGame {
    fn add_player(&mut self, player: Player) {
        self.players.push(player);
    }

    fn remove_player_if_exists(&mut self, client_id: u64) {
        // TODO: wipe a slice of landed squares
        if let Some(i) = self
            .players
            .iter()
            .position(|info| info.client_id == client_id)
        {
            self.players.remove(i);
        }
    }

    fn player_count(&self) -> usize {
        self.players.len()
    }

    fn get_square_contents(&self) -> HashMap<(i8, i8), game_logic_base::SquareContent> {
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

    fn render_to_buf(&self, buffer: &mut render::Buffer) {
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

    fn handle_key_press(&mut self, client_id: u64, key: ansi::KeyPress) -> bool {
        println!("client {} pressed {:?}", client_id, key);
        false
    }
}
