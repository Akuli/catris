use std::cell::RefCell;
use std::collections::HashMap;

use crate::lobby::ClientInfo;
use crate::logic_base::Player;
use crate::logic_base::PlayerPoint;
use crate::logic_base::SquareContent;
use crate::logic_base::WorldPoint;
use crate::render;

const HEIGHT: usize = 20;

pub struct TraditionalGame {
    pub players: Vec<RefCell<Player>>,
    landed_rows: [Vec<Option<SquareContent>>; HEIGHT],
    pub flashing_points: HashMap<WorldPoint, u8>,
}

impl TraditionalGame {
    pub fn new() -> TraditionalGame {
        const BLANK: Vec<Option<SquareContent>> = vec![];
        TraditionalGame {
            players: vec![],
            landed_rows: [BLANK; HEIGHT],
            flashing_points: HashMap::new(),
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

        let w = self.get_width();
        for row in self.landed_rows.iter_mut() {
            row.resize(w, None);
        }
    }

    pub fn remove_player_if_exists(&mut self, client_id: u64) {
        if let Some(i) = self
            .players
            .iter()
            .position(|info| info.borrow().client_id == client_id)
        {
            self.players.remove(i);
            // TODO: wipe a slice of landed squares properly, instead of trim at end
            let w = self.get_width();
            for row in self.landed_rows.iter_mut() {
                row.resize(w, None);
            }
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

    pub fn render_world_edges_to_buf(&self, buffer: &mut render::Buffer) -> (i8, i8) {
        for y in 0..HEIGHT {
            buffer.set_char(0, y, '|');
            buffer.set_char(2 * self.get_width() + 1, y, '|');
        }
        return (1, 0);
    }

    pub fn get_landed_square(&self, point: WorldPoint) -> Option<SquareContent> {
        let (x, y) = point;
        self.landed_rows[y as usize][x as usize]
    }

    pub fn set_landed_square(&mut self, point: WorldPoint, value: Option<SquareContent>) {
        let (x, y) = point;
        self.landed_rows[y as usize][x as usize] = value;
    }

    pub fn find_full_rows(&self) -> Vec<WorldPoint> {
        let mut full_points = vec![];
        for (y, row) in self.landed_rows.iter().enumerate() {
            if !row.iter().any(|cell| cell.is_none()) {
                for (x, _) in row.iter().enumerate() {
                    full_points.push((x as i8, y as i8));
                }
            }
        }
        full_points
    }

    pub fn remove_full_rows(&mut self, full_points: &[WorldPoint]) {
        let mut should_wipe = [false; HEIGHT];
        for (_, y) in full_points {
            should_wipe[*y as usize] = true;
        }

        for y in 0..HEIGHT {
            if should_wipe[y] {
                self.landed_rows[y].clear();
                self.landed_rows[y].resize(self.get_width(), None);
                self.landed_rows[..y + 1].rotate_right(1);
            }
        }
    }
}
