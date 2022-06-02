use std::cell::RefCell;
use std::collections::HashMap;

use crate::ansi;
use crate::lobby::ClientInfo;
use crate::logic_base::Player;
use crate::logic_base::PlayerPoint;
use crate::logic_base::SquareContent;
use crate::logic_base::WorldPoint;
use crate::render;

const HEIGHT: usize = 20;

pub struct TraditionalGame {
    pub players: Vec<RefCell<Player>>,
    pub landed_squares: HashMap<WorldPoint, SquareContent>,
}

impl TraditionalGame {
    pub fn new() -> TraditionalGame {
        TraditionalGame {
            players: vec![],
            landed_squares: HashMap::new(),
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

    pub fn add_player(&mut self, client_info: &ClientInfo) {
        let new_width_per_player = if self.players.len() == 0 { 10 } else { 7 };
        let spawn_x = self.players.len() * new_width_per_player + new_width_per_player / 2;
        self.players
            .push(RefCell::new(Player::new((spawn_x as i32, -1), client_info)));
        assert!(self.get_width_per_player() == new_width_per_player);
    }

    pub fn remove_player_if_exists(&mut self, client_id: u64) {
        // TODO: wipe a slice of landed squares
        if let Some(i) = self
            .players
            .iter()
            .position(|info| info.borrow().client_id == client_id)
        {
            self.players.remove(i);
        }
    }

    pub fn is_valid_moving_block_coords(&self, point: PlayerPoint) -> bool {
        let (x, y) = point;
        0 <= x && x < (self.get_width() as i32) && y < (HEIGHT as i32)
    }

    pub fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool {
        let (x, y) = point;
        0 <= x && x < self.get_width() as i8 && 0 <= y && y < HEIGHT as i8
    }

    pub fn square_belongs_to_player(&self, player_idx: usize, point: WorldPoint) -> bool {
        let (x, _) = point;
        (player_idx * self.get_width_per_player()) as i8 <= x
            && x < ((player_idx + 1) * self.get_width_per_player()) as i8
    }

    pub fn get_square_contents(
        &self,
        exclude_player_idx: Option<usize>,
    ) -> HashMap<(i8, i8), SquareContent> {
        let mut result: HashMap<(i8, i8), SquareContent> = HashMap::new();
        result.extend(&self.landed_squares);
        for (i, player) in self.players.iter().enumerate() {
            if Some(i) == exclude_player_idx {
                continue;
            }

            let (center_x, center_y) = player.borrow().block.center;
            for (x, y) in &player.borrow().block.relative_coords {
                let player_point: PlayerPoint = (*x + center_x, *y + center_y);
                result.insert(
                    player.borrow().player_to_world(player_point),
                    player.borrow().block.get_square_contents(),
                );
            }
        }

        result
    }

    pub fn render_to_buf(&self, buffer: &mut render::Buffer) {
        let square_contents = self.get_square_contents(None);

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
}
