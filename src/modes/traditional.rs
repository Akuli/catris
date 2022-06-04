use std::cell::RefCell;
use std::collections::HashMap;

use crate::lobby::ClientInfo;
use crate::logic_base::BlockOrTimer;
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

    fn update_spawn_places(&self) {
        let w = self.get_width_per_player() as i32;
        for (player_idx, player) in self.players.iter().enumerate() {
            let i = player_idx as i32;
            player.borrow_mut().spawn_point.0 = (i * w) + (w / 2);
        }
    }

    pub fn add_player(&mut self, client_info: &ClientInfo) {
        self.players
            .push(RefCell::new(Player::new((0, -1), client_info)));
        self.update_spawn_places();

        let w = self.get_width();
        for row in self.landed_rows.iter_mut() {
            row.resize(w, None);
        }
    }

    fn wipe_vertical_slice(&mut self, left: usize, width: usize) {
        let right = left + width;
        for row in self.landed_rows.iter_mut() {
            row.splice(left..right, vec![]);
        }

        let left = left as i32;
        let width = width as i32;
        let right = right as i32;

        for player in &self.players {
            match &mut player.borrow_mut().block_or_timer {
                BlockOrTimer::Block(block) => {
                    // In traditional mode, player points and world points are the same.
                    // So it doesn't matter whether "left" is in world or player points.
                    let old_points = block.get_coords();
                    let mut new_points = vec![];
                    for (x, y) in old_points {
                        // Remove points in (left..right), move points on right side
                        if (..left).contains(&x) {
                            new_points.push((x, y));
                        } else if (right..).contains(&x) {
                            new_points.push((x - width, y));
                        }
                    }

                    // Move center just like other points, except that it can't be removed
                    let (mut center_x, center_y) = block.center;
                    if (right..).contains(&center_x) {
                        center_x -= width;
                    } else if (left..right).contains(&center_x) {
                        center_x = left;
                    }

                    block.set_player_coords(&new_points, (center_x, center_y));
                }
                BlockOrTimer::Timer(_) => {}
            }
        }
    }

    pub fn remove_player_if_exists(&mut self, client_id: u64) {
        if let Some(i) = self
            .players
            .iter()
            .position(|info| info.borrow().client_id == client_id)
        {
            let slice_x = self.get_width_per_player() * i;
            let old_width = self.get_width();
            self.players.remove(i);
            let new_width = self.get_width();

            let slice_width = old_width - new_width;
            self.wipe_vertical_slice(slice_x, slice_width);

            self.update_spawn_places();
        }
    }

    pub fn is_valid_moving_block_coords(&self, point: PlayerPoint) -> bool {
        let (x, y) = point;
        let w = self.get_width() as i32;
        let h = HEIGHT as i32;
        (0..w).contains(&x) && (..h).contains(&y)
    }

    pub fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool {
        let (x, y) = point;
        let w = self.get_width() as i8;
        let h = HEIGHT as i8;
        (0..w).contains(&x) && (0..h).contains(&y)
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

    pub fn remove_full_rows_raw(&mut self, full_points: &[WorldPoint]) {
        let mut should_wipe = [false; HEIGHT];
        for (_, y) in full_points {
            should_wipe[*y as usize] = true;
        }

        for y in 0..HEIGHT {
            if should_wipe[y] {
                self.landed_rows[y].clear();
                self.landed_rows[y].resize(self.get_width(), None);
                self.landed_rows[..(y + 1)].rotate_right(1);
            }
        }
    }
}
