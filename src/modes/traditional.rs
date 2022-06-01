use std::collections::HashMap;

use crate::ansi;
use crate::lobby::ClientInfo;
use crate::logic_base::Game;
use crate::logic_base::Player;
use crate::logic_base::PlayerPoint;
use crate::logic_base::SquareContent;
use crate::logic_base::WorldPoint;
use crate::render;

const HEIGHT: usize = 20;

pub struct TraditionalGame {
    players: Vec<Player>,
}

impl TraditionalGame {
    pub fn new() -> TraditionalGame {
        TraditionalGame {
            players: vec![],
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

    fn player_to_world(&self, player_point: (i32, i32)) -> (i8, i8) {
        let (x, y) = player_point;
        (x as i8, y as i8)
    }
}

impl Game for TraditionalGame {
    fn add_player(&mut self, client_info: &ClientInfo) {
        let new_width_per_player = if self.players.len() == 0 { 10 } else { 7 };
        let spawn_x = self.players.len() * new_width_per_player + new_width_per_player / 2;
        self.players
            .push(Player::new((spawn_x as i32, -1), client_info));
        assert!(self.get_width_per_player() == new_width_per_player);
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

    fn get_players(&self) -> &Vec<Player> {
        &self.players
    }

    fn world_to_player(&self, _player_idx: usize, point: WorldPoint) -> PlayerPoint {
        let (x, y) = point;
        (x as i32, y as i32)
    }

    fn player_to_world(&self, _player_idx: usize, point: PlayerPoint) -> WorldPoint {
        let (x, y) = point;
        (x as i8, y as i8)
    }

    fn is_valid_moving_block_coords(&self, _player_idx: usize, point: PlayerPoint) -> bool {
        let (x, y) = point;
        0 <= x && x < (self.get_width() as i32) && y < (HEIGHT as i32)
    }

    fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool {
        let (x, y) = point;
        0 <= x && x < self.get_width() as i8 && 0 <= y && y < HEIGHT as i8
    }

    fn square_belongs_to_player(&self, player_idx: usize, point: WorldPoint) -> bool {
        let (x, _) = point;
        (player_idx * self.get_width_per_player()) as i8 <= x
            && x < ((player_idx + 1) * self.get_width_per_player()) as i8
    }

    fn get_square_contents(&self) -> HashMap<(i8, i8), SquareContent> {
        let mut result = HashMap::new();
        for player in &self.players {
            let (center_x, center_y) = player.block.center;
            for (x, y) in &player.block.relative_coords {
                let player_point: PlayerPoint = (*x + center_x, *y + center_y);
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
            buffer.set_char(2 * self.get_width() + 1, y, '|');

            for x in 0..self.get_width() {
                if let Some(content) = square_contents.get(&(x as i8, y as i8)) {
                    buffer.set_char_with_color(2 * x + 1, y, content.text[0], content.colors);
                    buffer.set_char_with_color(2 * x + 2, y, content.text[1], content.colors);
                }
            }
        }
    }

    fn move_blocks_down(&mut self) {
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

    fn handle_key_press(&mut self, client_id: u64, key: ansi::KeyPress) -> bool {
        println!("client {} pressed {:?}", client_id, key);
        false
    }
}
