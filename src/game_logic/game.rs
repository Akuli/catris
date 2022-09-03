use crate::ansi::Color;
use crate::ansi::KeyPress;
use crate::game_logic::blocks::BlockType;
use crate::game_logic::blocks::FallingBlock;
use crate::game_logic::blocks::SquareContent;
use crate::game_logic::player::BlockOrTimer;
use crate::game_logic::player::Player;
use crate::game_logic::BlockRelativeCoords;
use crate::game_logic::PlayerPoint;
use crate::game_logic::WorldPoint;
use crate::lobby::ClientInfo;
use crate::lobby::MAX_CLIENTS_PER_LOBBY;
use rand::Rng;
use std::cell::RefCell;
use std::cmp::max;
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum Mode {
    Traditional,
    Bottle,
    Ring,
    Adventure,
}

impl Mode {
    pub const ALL_MODES: &'static [Mode] =
        &[Mode::Traditional, Mode::Bottle, Mode::Ring, Mode::Adventure];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Traditional => "Traditional game",
            Mode::Bottle => "Bottle game",
            Mode::Ring => "Ring game",
            Mode::Adventure => "Adventure game",
        }
    }

    pub fn max_players(self) -> usize {
        match self {
            Mode::Traditional | Mode::Bottle | Mode::Adventure => MAX_CLIENTS_PER_LOBBY,
            Mode::Ring => 4,
        }
    }
}

fn circle(center: WorldPoint, radius: f32) -> Vec<WorldPoint> {
    let (cx, cy) = center;
    let mut result = vec![];
    for dx in (-radius.ceil() as i16)..=(radius.ceil() as i16) {
        for dy in (-radius.ceil() as i16)..=(radius.ceil() as i16) {
            if ((dx * dx + dy * dy) as f32) < radius * radius {
                result.push((cx + dx, cy + dy));
            }
        }
    }
    result
}

pub const BOTTLE_MAP: &[&str] = &[
    r"    |xxxxxxxxxx|    ",
    r"    |xxxxxxxxxx|    ",
    r"    |xxxxxxxxxx|    ",
    r"    |xxxxxxxxxx|    ",
    r"    /xxxxxxxxxx\    ",
    r"   /.xxxxxxxxxx.\   ",
    r"  /xxxxxxxxxxxxxx\  ",
    r" /.xxxxxxxxxxxxxx.\ ",
    r"/xxxxxxxxxxxxxxxxxx\",
    r"|xxxxxxxxxxxxxxxxxx|",
    r"|xxxxxxxxxxxxxxxxxx|",
    r"|xxxxxxxxxxxxxxxxxx|",
    r"|xxxxxxxxxxxxxxxxxx|",
    r"|xxxxxxxxxxxxxxxxxx|",
    r"|xxxxxxxxxxxxxxxxxx|",
    r"|xxxxxxxxxxxxxxxxxx|",
    r"|xxxxxxxxxxxxxxxxxx|",
    r"|xxxxxxxxxxxxxxxxxx|",
    r"|xxxxxxxxxxxxxxxxxx|",
    r"|xxxxxxxxxxxxxxxxxx|",
    r"|xxxxxxxxxxxxxxxxxx|",
];
const BOTTLE_INNER_WIDTH: usize = 9;
const BOTTLE_OUTER_WIDTH: usize = 10;
const BOTTLE_PERSONAL_SPACE_HEIGHT: usize = 9; // rows above the wide "|" area

pub const RING_MAP: &[&str] = &[
    "               .o------------------------------------------o.               ",
    "             .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'.             ",
    "           .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'.           ",
    "         .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'.         ",
    "       .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'.       ",
    "     .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'.     ",
    "   .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'.   ",
    " .'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'. ",
    "oxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxo",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxo============oxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|wwwwwwwwwwww|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|aaaaaadddddd|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|aaaaaadddddd|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|aaaaaadddddd|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|ssssssssssss|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxo------------oxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx|",
    "oxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxo",
    " '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.' ",
    "   '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.'   ",
    "     '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.'     ",
    "       '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.'       ",
    "         '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.'         ",
    "           '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.'           ",
    "             '.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.'             ",
    "               'o------------------------------------------o'               ",
];
pub const RING_OUTER_RADIUS: usize = 18;
pub const RING_INNER_RADIUS: usize = 3;

pub fn wrap_around(mode: Mode, y: &mut i32) {
    if mode == Mode::Ring && *y > 0 {
        *y += RING_OUTER_RADIUS as i32;
        *y %= (2 * RING_OUTER_RADIUS + 1) as i32;
        *y -= RING_OUTER_RADIUS as i32;
    }
}

/*
In adventure mode, the map always extends below the top of the view.
This way the landing places prediction will work even for blocks that aren't visible yet.

With multiple players, it's also possible to move blocks more than this amount below the view.
In that case, the map will be generated enough for the lowest square of that block.
*/
const ADVENTURE_EXTRA_ROWS_BOTTOM: usize = 5;

// In adventure mode, there are this many rows at top of game that never have blocks in them.
// This gives the player time to guide the block through a hole in previous landed squares.
// There are never landed squares above the top of the view, I tried that and it was very confusing.
const ADVENTURE_BLANK_ROWS_TOP: usize = 4;

struct AdventureModeData {
    scroll_count: usize,
    rows_since_prefill: u8, // Pre-filled row is a row that already contains squares when it appears
}

pub struct Game {
    pub players: Vec<RefCell<Player>>,
    pub flashing_points: HashMap<WorldPoint, u8>,
    pub mode: Mode,
    landed_rows: Vec<Vec<Option<SquareContent>>>,
    adventure_data: Option<AdventureModeData>,
    score: usize,
    bomb_id_counter: u64,
    block_factory: fn(usize) -> FallingBlock,
}
impl Game {
    pub fn new(mode: Mode) -> Self {
        let landed_rows = match mode {
            Mode::Traditional => vec![vec![]; 20],
            Mode::Bottle => vec![vec![]; 21],
            Mode::Adventure => vec![],
            Mode::Ring => {
                let size = 2 * RING_OUTER_RADIUS + 1;
                let mut rows = vec![];
                for _ in 0..size {
                    let mut row = vec![];
                    row.resize(size, None);
                    rows.push(row);
                }
                rows
            }
        };
        let adventure_data = if mode == Mode::Adventure {
            Some(AdventureModeData {
                scroll_count: 0,
                rows_since_prefill: 0,
            })
        } else {
            None
        };

        Self {
            players: vec![],
            flashing_points: HashMap::new(),
            mode,
            landed_rows,
            adventure_data,
            score: 0,
            bomb_id_counter: 0,
            block_factory: |score| FallingBlock::new(BlockType::from_score(score)),
        }
    }

    #[cfg(test)]
    pub fn truncate_height(&mut self, new_height: usize) {
        self.landed_rows.truncate(new_height);
    }

    #[cfg(test)]
    pub fn set_block_factory(&mut self, factory: fn(usize) -> FallingBlock) {
        self.block_factory = factory;
    }

    pub fn get_score(&self) -> usize {
        self.score
    }

    pub fn get_width_per_player(&self) -> Option<usize> {
        match self.mode {
            Mode::Traditional | Mode::Adventure if self.players.len() >= 2 => Some(7),
            Mode::Traditional | Mode::Adventure => Some(10),
            Mode::Bottle | Mode::Ring => None,
        }
    }

    pub fn get_width(&self) -> usize {
        // can't always return self.landed_rows[0].len(), because this is called during resizing
        // TODO: clean this up
        match self.mode {
            Mode::Traditional | Mode::Adventure => {
                self.get_width_per_player().unwrap() * self.players.len()
            }
            Mode::Bottle => BOTTLE_OUTER_WIDTH * self.players.len() - 1,
            Mode::Ring => self.landed_rows[0].len(),
        }
    }

    pub fn get_height(&self) -> usize {
        match self.mode {
            // Adventure mode has extra rows above and below the visible area
            Mode::Adventure => 21,
            _ => self.landed_rows.len(),
        }
    }

    // Where is world coordinate (0,0) in the landed_rows array?
    fn get_center_offset(&self) -> (i16, i16) {
        match self.mode {
            Mode::Traditional | Mode::Bottle | Mode::Adventure => (0, 0),
            Mode::Ring => (RING_OUTER_RADIUS as i16, RING_OUTER_RADIUS as i16),
        }
    }

    // for the ui, returns (x_min, x_max+1, y_min, y_max+1)
    pub fn get_bounds_in_player_coords(&self) -> (i32, i32, i32, i32) {
        match self.mode {
            Mode::Traditional | Mode::Bottle | Mode::Adventure => {
                (0, self.get_width() as i32, 0, self.get_height() as i32)
            }
            Mode::Ring => {
                let r = RING_OUTER_RADIUS as i32;
                (-r, r + 1, -r, r + 1)
            }
        }
    }

    fn update_spawn_points(&self) {
        match self.mode {
            Mode::Traditional | Mode::Adventure => {
                let w = self.get_width_per_player().unwrap() as i32;
                for (player_idx, player) in self.players.iter().enumerate() {
                    let i = player_idx as i32;
                    // This works in adventure mode because spawn points are in player coordinates
                    player.borrow_mut().spawn_point = ((i * w) + (w / 2), 0);
                }
            }
            Mode::Bottle => {
                for (player_idx, player) in self.players.iter().enumerate() {
                    let x = (player_idx * BOTTLE_OUTER_WIDTH) + (BOTTLE_INNER_WIDTH / 2);
                    player.borrow_mut().spawn_point = (x as i32, 0);
                }
            }
            Mode::Ring => {}
        }
    }

    fn wipe_vertical_slice(&mut self, left: usize, width: usize) {
        // In these modes, player x coordinates and world x coordinates are the same.
        // So it doesn't matter whether "left" is in world or player points.
        assert!(
            self.mode == Mode::Traditional
                || self.mode == Mode::Bottle
                || self.mode == Mode::Adventure
        );

        let right = left + width;
        for row in &mut self.landed_rows {
            row.splice(left..right, vec![]);
        }

        let left = left as i32;
        let width = width as i32;
        let right = right as i32;

        for player in &self.players {
            if let BlockOrTimer::Block(block) = &mut player.borrow_mut().block_or_timer {
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
        }
    }

    fn generate_rows(&mut self) {
        if self.mode != Mode::Adventure {
            return;
        }

        let biggest_y_used = self
            .players
            .iter()
            .map(|p| p.borrow().block_or_timer.get_coords())
            .flatten()
            .map(|(_, y)| y)
            .chain([self.get_height() as i32].into_iter())
            .max()
            .unwrap();
        let min_count = biggest_y_used + (ADVENTURE_EXTRA_ROWS_BOTTOM as i32);

        while (self.landed_rows.len() as i32) < min_count {
            let y = self.adventure_data.as_ref().unwrap().scroll_count + self.landed_rows.len();
            let prefill = if y < self.get_height()-1 {
                false // Start with most of the playing area empty
            } else if y == self.get_height() - 1 {
                true // But with one pre-filled row at the bottom
            } else if self.adventure_data.as_ref().unwrap().rows_since_prefill == 0 {
                /*
                Previous row was prefilled. Add another prefilled row with probability p.
                This way, expected value of number of rows added is

                     inf
                    .---    k-1         1
                     )   k p     = -----------
                    '---            (1 - p)^2
                     k=1

                where kp^(k-1) is the probability of getting k consecutive pre-filled rows.

                We first choose the expected value of rows so that the game reasonably
                becomes more challenging as you play, and then compute the corresponding
                probability p.

                The expected value is >1, meaning p>0. Otherwise users could wait for a
                block that fits through a single hole and keep moving it to the matching
                hole when new rows arrive.
                */
                let s = self.score as f64;
                let expected_value_of_prefilled_count = 1.5 + s / 5000.0;
                let p = 1.0 - (1.0 / expected_value_of_prefilled_count).sqrt();
                rand::thread_rng().gen_bool(p)
            } else if self.adventure_data.as_ref().unwrap().rows_since_prefill == 4 {
                // Got enough normal rows, time for prefilled
                true
            } else {
                false
            };

            if prefill {
                self.adventure_data.as_mut().unwrap().rows_since_prefill = 0;
            } else {
                self.adventure_data.as_mut().unwrap().rows_since_prefill += 1;
            }

            if prefill {
                let mut row = vec![Some(SquareContent::new_undrillable()); self.get_width()];

                let n = row.len(); // borrow checker is lol
                row[rand::thread_rng().gen_range(0..n)] = None;
                if rand::thread_rng().gen_bool(0.1) {
                    row[rand::thread_rng().gen_range(0..n)] = None;
                }

                self.landed_rows.push(row);
            } else {
                self.landed_rows.push(vec![None; self.get_width()]);
            }
        }

        // TODO: this is not an ideal place for this as it erases landed squares
        // Should probably run when scrolling?
        for row in &mut self.landed_rows[..ADVENTURE_BLANK_ROWS_TOP] {
            *row = vec![None; row.len()];
        }
    }

    pub fn add_player(&mut self, client_info: &ClientInfo) -> bool {
        if self.players.len() == self.mode.max_players() {
            return false;
        }

        let player_idx = self.players.len();
        let down_direction = match self.mode {
            Mode::Traditional | Mode::Bottle | Mode::Adventure => (0, 1),
            Mode::Ring => {
                /*
                prefer opposite directions of existing players
                never choose a direction that is already in use
                choose consistently, not randomly or depending on hashing
                */
                let used: Vec<WorldPoint> = self
                    .players
                    .iter()
                    .map(|p| p.borrow().down_direction)
                    .collect();
                let opposites: Vec<WorldPoint> = used.iter().map(|(x, y)| (-x, -y)).collect();
                let all: &[WorldPoint] = &[(0, 1), (0, -1), (1, 0), (-1, 0)];

                *opposites
                    .iter()
                    .chain(all.iter())
                    .find(|dir| !used.contains(dir))
                    .unwrap()
            }
        };
        let spawn_point = match self.mode {
            Mode::Ring => (0, -(RING_OUTER_RADIUS as i32)),
            _ => (0, 0), // dummy value to be changed soon
        };
        self.players.push(RefCell::new(Player::new(
            spawn_point,
            client_info,
            down_direction,
            self.mode,
            || (self.block_factory)(self.score),
        )));
        self.update_spawn_points();

        let w = self.get_width();
        match self.mode {
            Mode::Traditional => {
                for y in 0..self.landed_rows.len() {
                    self.landed_rows[y].resize(w, None);
                }
            }
            Mode::Bottle => {
                for (y, row) in self.landed_rows.iter_mut().enumerate() {
                    row.resize(w, None);
                    if player_idx >= 1 && (BOTTLE_PERSONAL_SPACE_HEIGHT..).contains(&y) {
                        let left_color = Color {
                            fg: self.players[player_idx - 1].borrow().color,
                            bg: 0,
                        };
                        let right_color = Color {
                            fg: client_info.color,
                            bg: 0,
                        };
                        row[player_idx * BOTTLE_OUTER_WIDTH - 1] = Some(SquareContent::Normal([
                            ('|', left_color),
                            ('|', right_color),
                        ]));
                    }
                }
            }
            Mode::Ring => self.clear_playing_area(player_idx),
            Mode::Adventure => {
                if player_idx == 0 {
                    // First player
                    self.generate_rows();
                } else {
                    todo!();
                }
            }
        }

        self.new_block(player_idx);
        true
    }

    pub fn remove_player_if_exists(&mut self, client_id: u64) {
        let i = self
            .players
            .iter()
            .position(|info| info.borrow().client_id == client_id);
        if i.is_none() {
            return;
        }
        let i = i.unwrap();

        match self.mode {
            Mode::Traditional | Mode::Adventure => {
                let slice_x = self.get_width_per_player().unwrap() * i;
                let old_width = self.get_width();
                self.players.remove(i);
                let new_width = self.get_width();

                let slice_width = old_width - new_width;
                self.wipe_vertical_slice(slice_x, slice_width);
            }
            Mode::Bottle => {
                let (slice_x, slice_width) = if self.players.len() == 1 {
                    (0, BOTTLE_INNER_WIDTH)
                } else if i == 0 {
                    (0, BOTTLE_OUTER_WIDTH)
                } else if i == self.players.len() - 1 {
                    (i * BOTTLE_OUTER_WIDTH, BOTTLE_INNER_WIDTH)
                } else {
                    (i * BOTTLE_OUTER_WIDTH, BOTTLE_OUTER_WIDTH)
                };

                self.players.remove(i);
                self.wipe_vertical_slice(slice_x, slice_width);
            }
            Mode::Ring => {
                self.players.remove(i);
            }
        }

        self.update_spawn_points();
    }

    fn add_score(&mut self, mut add: usize, multi_player_compensate: bool) {
        if multi_player_compensate {
            /*
            It seems to be exponentially harder to get more points when there are a
            lot of players, basically P(all n players full) = P(1 player full)^n,
            although that wrongly assumes players are independent of each other.

            Currently this seems to give points more easily when there's a lot of
            players, but maybe that's a feature, because it should encourage people
            to play together :)

            The scores also feel quite different for single player and multiplayer.
            That's why they are shown separately in the high scores view.
            */
            add *= 2usize.pow((self.players.len() as u32) - 1);
        }
        self.score += add;
    }

    pub fn find_full_rows_and_increment_score(&mut self) -> Vec<WorldPoint> {
        let mut full_points = vec![];
        let mut full_count_everyone = 0;
        let mut full_count_single_player = 0;

        match self.mode {
            Mode::Traditional => {
                for (y, row) in self.landed_rows.iter().enumerate() {
                    if !row.iter().any(|cell| cell.is_none()) {
                        full_count_everyone += 1;
                        for (x, _) in row.iter().enumerate() {
                            full_points.push((x as i16, y as i16));
                        }
                    }
                }
            }
            Mode::Adventure => {
                // Same as traditional, but do not add score here
                for (y, row) in self.landed_rows.iter().enumerate() {
                    if !row.iter().any(|cell| cell.is_none()) {
                        for (x, _) in row.iter().enumerate() {
                            full_points.push((x as i16, y as i16));
                        }
                    }
                }
            }
            Mode::Bottle => {
                for (y, row) in self.landed_rows.iter().enumerate() {
                    if (0..BOTTLE_PERSONAL_SPACE_HEIGHT).contains(&y) {
                        for i in 0..self.players.len() {
                            let left = BOTTLE_OUTER_WIDTH * i
                                + BOTTLE_MAP[y].chars().position(|c| c == 'x').unwrap() / 2;
                            let right = left + BOTTLE_MAP[y].matches("xx").count();
                            if !row[left..right].iter().any(|cell| cell.is_none()) {
                                full_count_single_player += 1;
                                for x in left..right {
                                    full_points.push((x as i16, y as i16));
                                }
                            }
                        }
                    } else if !row.iter().any(|cell| cell.is_none()) {
                        full_count_everyone += 1;
                        for (x, _) in row.iter().enumerate() {
                            full_points.push((x as i16, y as i16));
                        }
                    }
                }
            }
            Mode::Ring => {
                for r in (RING_INNER_RADIUS as i16 + 1)..=(RING_OUTER_RADIUS as i16) {
                    let mut ring = vec![(-r, -r), (-r, r), (r, -r), (r, r)];
                    for i in (-r + 1)..r {
                        ring.push((-r, i));
                        ring.push((r, i));
                        ring.push((i, -r));
                        ring.push((i, r));
                    }

                    if ring.iter().all(|p| self.get_landed_square(*p).is_some()) {
                        full_count_everyone += 1;
                        full_points.extend(ring);
                    }
                }
            }
        }

        /*
        With 1 player:
            no full rows: +0
            1 full row:   +10
            2 full rows:  +30
            3 full rows:  +60
            etc
        */
        self.add_score(
            5 * full_count_single_player * (full_count_single_player + 1),
            false,
        );
        self.add_score(5 * full_count_everyone * (full_count_everyone + 1), true);
        full_points
    }

    pub fn remove_full_rows(&mut self, full: &[WorldPoint]) {
        match self.mode {
            Mode::Traditional | Mode::Adventure => {
                for y in 0..self.landed_rows.len() {
                    if full.contains(&(0, y as i16)) {
                        self.landed_rows[..(y + 1)].rotate_right(1);
                        for cell in &mut self.landed_rows[0] {
                            *cell = None;
                        }
                    }
                }
            }
            Mode::Bottle => {
                for (i, _) in self.players.iter().enumerate() {
                    for y in 0..BOTTLE_PERSONAL_SPACE_HEIGHT {
                        let x_left = i * BOTTLE_OUTER_WIDTH;
                        let x_right = x_left + BOTTLE_INNER_WIDTH;
                        if full.contains(&(((x_left + x_right) / 2) as i16, y as i16)) {
                            // Blocks fall down only on this player's personal area
                            for source_y in (0..y).rev() {
                                let source_row =
                                    self.landed_rows[source_y][x_left..x_right].to_vec();
                                self.landed_rows[source_y + 1].splice(x_left..x_right, source_row);
                            }
                            for cell in &mut self.landed_rows[0][x_left..x_right] {
                                *cell = None;
                            }
                        }
                    }
                }

                for y in BOTTLE_PERSONAL_SPACE_HEIGHT..self.landed_rows.len() {
                    if full.contains(&(0, y as i16)) {
                        self.landed_rows[..(y + 1)].rotate_right(1);
                        for cell in &mut self.landed_rows[0] {
                            *cell = None;
                        }
                    }
                }
            }
            Mode::Ring => {
                let mut counts = vec![0; RING_OUTER_RADIUS + 1];
                for (x, y) in full {
                    self.set_landed_square((*x, *y), None);
                    counts[max(x.abs(), y.abs()) as usize] += 1;
                }

                // removing a ring shifts outer radiuses, so go inwards
                for (r, count) in counts.iter().enumerate().rev() {
                    if r == 0 || *count != 8 * r {
                        continue;
                    }
                    let r = r as i16;

                    // clear destination radius where outer blocks will go
                    // moving the squares doesn't overwrite, if source (outer) square is None
                    for i in (-r)..=r {
                        self.set_landed_square((-r, i), None);
                        self.set_landed_square((r, i), None);
                        self.set_landed_square((i, -r), None);
                        self.set_landed_square((i, r), None);
                    }

                    for dest_r in r..(RING_OUTER_RADIUS as i16) {
                        let source_r = dest_r + 1;
                        for i in (-source_r + 1)..source_r {
                            self.move_landed_square((-source_r, i), (-dest_r, i));
                            self.move_landed_square((source_r, i), (dest_r, i));
                            self.move_landed_square((i, -source_r), (i, -dest_r));
                            self.move_landed_square((i, source_r), (i, dest_r));
                        }
                        self.move_landed_square((-source_r, -source_r), (-dest_r, -dest_r));
                        self.move_landed_square((-source_r, source_r), (-dest_r, dest_r));
                        self.move_landed_square((source_r, -source_r), (dest_r, -dest_r));
                        self.move_landed_square((source_r, source_r), (dest_r, dest_r));
                    }
                }
            }
        }

        // Moving landed squares can cause them to overlap falling squares
        let mut potential_overlaps: Vec<WorldPoint> = vec![];

        for player in &self.players {
            let player = player.borrow();
            for player_point in player.block_or_timer.get_coords() {
                potential_overlaps.push(player.player_to_world(player_point));
            }
        }

        for point in potential_overlaps {
            if self.is_valid_landed_block_coords(point) {
                self.set_landed_square(point, None);
            }
        }
    }

    fn is_valid_falling_block_coords(&self, point: PlayerPoint) -> bool {
        let (x, mut y) = point;
        let top_y = match self.mode {
            Mode::Traditional | Mode::Bottle | Mode::Adventure => 0,
            Mode::Ring => -(RING_OUTER_RADIUS as i32),
        };
        if y < top_y {
            y = top_y;
        }
        wrap_around(self.mode, &mut y);
        self.is_valid_landed_block_coords((x as i16, y as i16))
    }

    pub fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool {
        let (x, y) = point;
        match self.mode {
            Mode::Traditional | Mode::Adventure => {
                let w = self.get_width() as i16;
                let h = self.landed_rows.len() as i16; // can be below view in adventure mode
                (0..w).contains(&x) && (0..h).contains(&y)
            }
            Mode::Bottle => {
                let w = self.get_width() as i16;
                let h = self.get_height() as i16;
                if !(0..w).contains(&x) || !(0..h).contains(&y) {
                    false
                } else if (x as usize) % BOTTLE_OUTER_WIDTH == BOTTLE_INNER_WIDTH {
                    // on wall between two players, not allowed near top
                    (BOTTLE_PERSONAL_SPACE_HEIGHT..).contains(&(y as usize))
                } else {
                    let line = BOTTLE_MAP[y as usize].as_bytes();
                    line[2 * ((x as usize) % BOTTLE_OUTER_WIDTH) + 1] == b'x'
                }
            }
            Mode::Ring => {
                if max(x.abs(), y.abs()) > (RING_OUTER_RADIUS as i16) {
                    return false;
                }
                let map_x = 2 * (x + (RING_OUTER_RADIUS as i16)) as usize + 1;
                let map_y = (y + (RING_OUTER_RADIUS as i16)) as usize + 1;
                let line = RING_MAP[map_y as usize].as_bytes();
                line[map_x as usize] == b'x'
            }
        }
    }

    pub fn get_falling_square(
        &self,
        point: WorldPoint,
    ) -> Option<(SquareContent, BlockRelativeCoords, usize)> {
        for (player_idx, player) in self.players.iter().enumerate() {
            if let BlockOrTimer::Block(block) = &player.borrow().block_or_timer {
                for (player_coords, relative_coords) in block
                    .get_coords()
                    .iter()
                    .zip(block.get_relative_coords().iter())
                {
                    if player.borrow().player_to_world(*player_coords) == point {
                        return Some((block.square_content, *relative_coords, player_idx));
                    }
                }
            }
        }
        None
    }

    pub fn get_landed_square(&self, point: WorldPoint) -> Option<SquareContent> {
        let (x, y) = point;
        let (offset_x, offset_y) = self.get_center_offset();
        self.landed_rows[(y + offset_y) as usize][(x + offset_x) as usize]
    }

    pub fn set_landed_square(&mut self, point: WorldPoint, value: Option<SquareContent>) {
        let (x, y) = point;
        if self.mode == Mode::Adventure && (0..(ADVENTURE_BLANK_ROWS_TOP as i16)).contains(&y) {
            return;
        }
        let (offset_x, offset_y) = self.get_center_offset();
        self.landed_rows[(y + offset_y) as usize][(x + offset_x) as usize] = value;
    }

    fn move_landed_square(&mut self, from: WorldPoint, to: WorldPoint) {
        let value_to_move = self.get_landed_square(from);
        self.set_landed_square(from, None);
        if value_to_move.is_some() {
            self.set_landed_square(to, value_to_move);
        }
    }

    pub fn get_any_square(
        &self,
        point: WorldPoint,
        exclude_player_idx: Option<usize>,
    ) -> Option<SquareContent> {
        if self.is_valid_landed_block_coords(point) {
            if let Some(content) = self.get_landed_square(point) {
                return Some(content);
            }
        }

        if let Some((content, _, player_idx)) = self.get_falling_square(point) {
            if exclude_player_idx != Some(player_idx) {
                return Some(content);
            }
        }

        None
    }

    fn rotate_if_possible(&self, player_idx: usize, prefer_counter_clockwise: bool) -> bool {
        let player = &self.players[player_idx];
        let coords = match &player.borrow().block_or_timer {
            BlockOrTimer::Block(block) => block.get_rotated_coords(prefer_counter_clockwise),
            _ => return false,
        };

        let can_rotate = coords.iter().all(|p| {
            let stays_in_bounds = self.is_valid_falling_block_coords(*p);
            let goes_on_top_of_something = self
                .get_any_square(player.borrow().player_to_world(*p), Some(player_idx))
                .is_some();
            stays_in_bounds && !goes_on_top_of_something
        });
        if can_rotate {
            let mut player = player.borrow_mut();
            match &mut player.block_or_timer {
                BlockOrTimer::Block(block) => block.rotate(prefer_counter_clockwise),
                _ => panic!(),
            }
        }
        can_rotate
    }

    fn move_if_possible(
        &mut self,
        player_idx: usize,
        dx: i8,
        dy: i8,
        enable_drilling: bool,
    ) -> bool {
        let player = &self.players[player_idx];
        let mut gonna_drill: HashSet<WorldPoint> = HashSet::new();
        let can_move = {
            let (content, coords) = match &player.borrow().block_or_timer {
                BlockOrTimer::Block(block) => {
                    (block.square_content, block.get_moved_coords(dx, dy))
                }
                _ => return false,
            };

            coords.iter().all(|p| {
                let stays_in_bounds = self.is_valid_falling_block_coords(*p);
                stays_in_bounds && {
                    let p = player.borrow().player_to_world(*p);
                    if let Some(goes_on_top_of) = self.get_any_square(p, Some(player_idx)) {
                        if enable_drilling && content.can_drill(&goes_on_top_of) {
                            gonna_drill.insert(p);
                            true
                        } else {
                            false
                        }
                    } else {
                        true
                    }
                }
            })
        };

        if can_move {
            match &mut player.borrow_mut().block_or_timer {
                BlockOrTimer::Block(block) => block.m0v3(dx, dy),
                _ => panic!(),
            }
            self.filter_and_mutate_all_squares_in_place(|point, _, i| {
                i == Some(player_idx) || !gonna_drill.contains(&point)
            });
        }
        can_move
    }

    pub fn predict_landing_place(&self, player_idx: usize) -> Vec<WorldPoint> {
        let player = &self.players[player_idx];
        let (content, mut working_coords) = match &player.borrow().block_or_timer {
            BlockOrTimer::Block(block) => (block.square_content, block.get_coords()),
            _ => return vec![],
        };

        // 40 is enough even in ring mode
        for _ in 0..40 {
            let can_move = working_coords.iter().all(|p| {
                let (x, mut y) = *p;
                y += 1;

                let stays_in_bounds = self.is_valid_falling_block_coords((x, y));
                stays_in_bounds && {
                    let world_point = player.borrow().player_to_world((x, y));
                    if let Some(goes_on_top_of) = self.get_any_square(world_point, Some(player_idx))
                    {
                        content.can_drill(&goes_on_top_of)
                    } else {
                        true
                    }
                }
            });
            if can_move {
                for point in working_coords.iter_mut() {
                    point.1 += 1;
                }
            } else {
                return working_coords
                    .iter()
                    .map(|p| player.borrow().player_to_world(*p))
                    .collect();
            }
        }

        // Block won't land if it moves down. Happens a lot in ring mode.
        return vec![];
    }

    pub fn move_blocks_down(&mut self, fast: bool) -> bool {
        let mut drill_indexes = vec![];
        let mut other_indexes = vec![];
        for (player_idx, player) in self.players.iter().enumerate() {
            if player.borrow().fast_down == fast {
                if let BlockOrTimer::Block(b) = &player.borrow().block_or_timer {
                    if b.square_content.is_drill() {
                        drill_indexes.push(player_idx);
                    } else {
                        other_indexes.push(player_idx);
                    }
                }
            }
        }

        let mut need_render = false;
        loop {
            let old_total_len = drill_indexes.len() + other_indexes.len();
            // Move drills last, gives other blocks a chance to go in front of a drill and get drilled
            // Need to loop so other blocks can go to where a drill came from
            other_indexes.retain(|i| !self.move_if_possible(*i, 0, 1, true));
            drill_indexes.retain(|i| !self.move_if_possible(*i, 0, 1, true));
            if drill_indexes.len() + other_indexes.len() == old_total_len {
                break;
            }
            need_render = true;
        }

        // Remaining players need a new block
        for player_idx in drill_indexes.iter().chain(other_indexes.iter()) {
            let player = &self.players[*player_idx];
            if fast {
                player.borrow_mut().fast_down = false;
            } else {
                let (player_coords, relative_coords, square_content) =
                    if let BlockOrTimer::Block(b) = &player.borrow().block_or_timer {
                        (
                            b.get_coords(),
                            b.get_relative_coords().to_vec(),
                            b.square_content,
                        )
                    } else {
                        panic!()
                    };

                let world_coords: Vec<WorldPoint> = player_coords
                    .iter()
                    .map(|p| player.borrow().player_to_world(*p))
                    .collect();
                if world_coords
                    .iter()
                    .all(|p| self.is_valid_landed_block_coords(*p))
                {
                    // land the block
                    let (down_x, down_y) = player.borrow().down_direction;
                    for (w, r) in world_coords.iter().zip(relative_coords.iter()) {
                        let landed_content =
                            square_content.get_landed_content(*r, (down_x as i8, down_y as i8));
                        self.set_landed_square(*w, Some(landed_content));
                    }
                    self.new_block(*player_idx);
                } else {
                    // no room to land
                    player.borrow_mut().block_or_timer = BlockOrTimer::TimerPending;
                }
                need_render = true;
            }
        }

        if self.mode == Mode::Adventure {
            // Keep highest moving block visible
            let smallest_center_y = self
                .players
                .iter()
                .filter_map(|p| match &p.borrow().block_or_timer {
                    BlockOrTimer::Block(b) => Some(b.center.1),
                    _ => None,
                })
                .min()
                .unwrap_or(-1);

            if smallest_center_y > 14 {
                self.scroll_down();
                need_render = true;
            }
        }

        need_render
    }

    fn scroll_down(&mut self) {
        assert!(self.mode == Mode::Adventure);
        self.adventure_data.as_mut().unwrap().scroll_count += 1;
        for player in &mut self.players {
            if let BlockOrTimer::Block(b) = &mut player.borrow_mut().block_or_timer {
                b.center.1 -= 1;
            }
        }

        self.landed_rows.remove(0);
        self.generate_rows();
        self.add_score(1, true);
    }

    fn flip_view(&mut self) -> bool {
        if self.mode != Mode::Ring || self.players.len() != 1 {
            return false;
        }

        let mut player = self.players[0].borrow_mut();
        let coords = match &player.block_or_timer {
            BlockOrTimer::Block(block) => block.get_coords(),
            _ => return false,
        };

        for (x, y) in coords {
            let flipped_point = player.player_to_world((-x, -y));
            if self.is_valid_landed_block_coords(flipped_point)
                && self.get_landed_square(flipped_point).is_some()
            {
                return false;
            }
        }

        player.down_direction.0 *= -1;
        player.down_direction.1 *= -1;
        true
    }

    pub fn animate_drills(&mut self) -> bool {
        let mut something_changed = false;
        let mut handle_block = |block: &mut FallingBlock| {
            if block.square_content.animate() {
                something_changed = true;
            }
        };

        for player_ref in &self.players {
            let mut player = player_ref.borrow_mut();
            if let BlockOrTimer::Block(b) = &mut player.block_or_timer {
                handle_block(b);
            }
            handle_block(&mut player.next_block);
            if let Some(b) = &mut player.block_in_hold {
                handle_block(b);
            }
        }
        something_changed
    }

    pub fn handle_key_press(
        &mut self,
        client_id: u64,
        client_prefers_rotating_counter_clockwise: bool,
        key: KeyPress,
    ) -> bool {
        let player_idx = self
            .players
            .iter()
            .position(|cell| cell.borrow().client_id == client_id)
            .unwrap();

        let need_render = match key {
            KeyPress::Down | KeyPress::Character('S') | KeyPress::Character('s') => {
                let mut player = self.players[player_idx].borrow_mut();
                player.fast_down = true;
                return false;
            }
            KeyPress::Left | KeyPress::Character('A') | KeyPress::Character('a') => {
                self.move_if_possible(player_idx, -1, 0, false)
            }
            KeyPress::Right | KeyPress::Character('D') | KeyPress::Character('d') => {
                self.move_if_possible(player_idx, 1, 0, false)
            }
            KeyPress::Up | KeyPress::Character('W') | KeyPress::Character('w') => {
                self.rotate_if_possible(player_idx, client_prefers_rotating_counter_clockwise)
            }
            KeyPress::Character('F') | KeyPress::Character('f') => self.flip_view(),
            KeyPress::Character('H') | KeyPress::Character('h') => self.hold_block(player_idx),
            _ => false,
        };

        self.players[player_idx].borrow_mut().fast_down = false;
        need_render
    }

    fn can_add_block(&self, player_idx: usize, block: &FallingBlock) -> bool {
        let overlaps = block.get_coords().iter().any(|p| {
            self.get_any_square(
                self.players[player_idx].borrow().player_to_world(*p),
                Some(player_idx),
            )
            .is_some()
        });
        !overlaps
    }

    fn new_block_possibly_from_hold(&self, player_idx: usize, from_hold_if_possible: bool) {
        use std::mem::replace;

        let block = {
            let mut player = self.players[player_idx].borrow_mut();
            let mut block = if from_hold_if_possible && player.block_in_hold.is_some() {
                replace(&mut player.block_in_hold, None).unwrap()
            } else {
                replace(&mut player.next_block, (self.block_factory)(self.score))
            };
            block.spawn_at(player.spawn_point);
            block
        };

        let can_add = self.can_add_block(player_idx, &block);
        let mut player = self.players[player_idx].borrow_mut();
        if can_add {
            player.block_or_timer = BlockOrTimer::Block(block)
        } else {
            player.block_or_timer = BlockOrTimer::TimerPending
        }
        player.fast_down = false;
    }

    fn new_block(&self, player_idx: usize) {
        self.new_block_possibly_from_hold(player_idx, false);
    }

    fn hold_block(&self, player_idx: usize) -> bool {
        use std::mem::replace;

        let mut to_hold = match &mut self.players[player_idx].borrow_mut().block_or_timer {
            BlockOrTimer::Block(b) if !b.has_been_in_hold => {
                // Replace the block with a dummy value.
                // It will be overwritten soon anyway.
                replace(b, (self.block_factory)(self.score))
            }
            _ => return false,
        };
        self.new_block_possibly_from_hold(player_idx, true);
        to_hold.has_been_in_hold = true;
        self.players[player_idx].borrow_mut().block_in_hold = Some(to_hold);
        true
    }

    pub fn get_points_to_flash(&self, bomb_centers: &Vec<WorldPoint>) -> Vec<WorldPoint> {
        let mut result: HashSet<WorldPoint> = HashSet::new();
        for center in bomb_centers {
            for point in circle(*center, 3.5) {
                if self.is_valid_landed_block_coords(point) {
                    result.insert(point);
                }
            }
        }
        Vec::from_iter(result)
    }

    // for<'a> copied from stackoverflow answer with 0 upvotes
    // https://stackoverflow.com/a/71254643
    fn filter_and_mutate_all_squares_in_place<F>(&mut self, mut f: F)
    where
        F: for<'a> FnMut(WorldPoint, &'a mut SquareContent, Option<usize>) -> bool,
    {
        let mut need_new_block = vec![];

        for (player_idx, player_ref) in self.players.iter().enumerate() {
            let mut player = player_ref.borrow_mut();

            // Conversion to world coords can't be done while block_or_timer is borrowed mutable.
            // I don't really understand why, but it doesn't compile if I use &mut block_or_timer for everything.
            let mut player_coords: Vec<PlayerPoint> = vec![];
            let mut world_coords: Vec<WorldPoint> = vec![];

            if let BlockOrTimer::Block(falling_block) = &player.block_or_timer {
                player_coords = falling_block.get_coords();
                world_coords = player_coords
                    .iter()
                    .map(|p| player.player_to_world(*p))
                    .collect();
            }

            if let BlockOrTimer::Block(falling_block) = &mut player.block_or_timer {
                let old_len = player_coords.len();
                assert!(old_len != 0);

                // see example in retain docs
                let mut world_coord_iter = world_coords.iter();
                player_coords.retain(|_| {
                    f(
                        *world_coord_iter.next().unwrap(),
                        &mut falling_block.square_content,
                        Some(player_idx),
                    )
                });

                if player_coords.is_empty() {
                    // can't call new_block() here, because player is already borrowed
                    need_new_block.push(player_idx);
                    continue;
                }

                // this if statement is a pseudo-optimization
                if player_coords.len() != old_len {
                    falling_block.set_player_coords(&player_coords, falling_block.center);
                }
            }
        }

        for player_idx in need_new_block {
            self.new_block(player_idx);
        }

        let (offset_x, offset_y) = self.get_center_offset();
        for (y, row) in self.landed_rows.iter_mut().enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                let point = (x as i16 - offset_x, y as i16 - offset_y);
                if let Some(content) = cell {
                    if !f(point, content, None) {
                        *cell = None;
                    }
                }
            }
        }
    }

    // returns bomb locations that were affected
    pub fn finish_explosion(
        &mut self,
        old_bomb_points: &[WorldPoint],
        old_flashing_points: &[WorldPoint],
    ) -> Vec<WorldPoint> {
        let mut bomb_locations = vec![];

        self.filter_and_mutate_all_squares_in_place(|point, content, _| {
            if content.is_bomb()
                && old_flashing_points.contains(&point)
                && !old_bomb_points.contains(&point)
            {
                bomb_locations.push(point);
            }
            !old_flashing_points.contains(&point)
        });

        bomb_locations
    }

    pub fn start_ticking_new_bombs(&mut self) -> Vec<u64> {
        let mut bomb_ids = vec![];
        for player in &self.players {
            if let BlockOrTimer::Block(block) = &mut player.borrow_mut().block_or_timer {
                if let SquareContent::Bomb { id, .. } = &mut block.square_content {
                    if id.is_none() {
                        *id = Some(self.bomb_id_counter);
                        bomb_ids.push(self.bomb_id_counter);
                        self.bomb_id_counter += 1;
                    }
                }
            }
        }
        bomb_ids
    }

    // Returns list of locations of exploding bombs, or None if bombs with given id no longer exist
    pub fn tick_bombs_by_id(&mut self, bomb_id: u64) -> Option<Vec<WorldPoint>> {
        let mut found_bombs = false;
        let mut result: Vec<WorldPoint> = vec![];

        // Each player typically has 4 bomb squares associated with the same square content.
        // We want to decrement the counter in the square content only once.
        let mut falling_block_timer_decremented: Vec<bool> = vec![false; self.players.len()];

        self.filter_and_mutate_all_squares_in_place(|point, square_content, player_idx| {
            match square_content {
                SquareContent::Bomb { id, timer } if *id == Some(bomb_id) => {
                    found_bombs = true;
                    // timer can already be zero, if other bombs are exploding (holds async lock)
                    if *timer > 0
                        && (player_idx.is_none()
                            || !falling_block_timer_decremented[player_idx.unwrap()])
                    {
                        *timer -= 1;
                    }
                    if *timer == 0 {
                        result.push(point);
                    }
                    if let Some(i) = player_idx {
                        falling_block_timer_decremented[i] = true;
                    }
                }
                _ => {}
            }
            true
        });

        if found_bombs {
            Some(result)
        } else {
            None
        }
    }

    // returns None if everyone end up waiting, i.e. if game is over
    pub fn start_pending_please_wait_counters(&mut self) -> Option<Vec<u64>> {
        let mut client_ids = vec![];
        for player in &self.players {
            let mut player = player.borrow_mut();
            if matches!(player.block_or_timer, BlockOrTimer::TimerPending) {
                player.block_or_timer = BlockOrTimer::Timer(30);
                client_ids.push(player.client_id);
            }
        }

        if self
            .players
            .iter()
            .all(|p| matches!(p.borrow().block_or_timer, BlockOrTimer::Timer(_)))
        {
            None
        } else {
            Some(client_ids)
        }
    }

    // returns whether this should be called again in 1 second
    pub fn tick_please_wait_counter(&mut self, client_id: u64) -> bool {
        if let Some(i) = self
            .players
            .iter()
            .position(|p| p.borrow().client_id == client_id)
        {
            let need_reset = {
                let mut player = self.players[i].borrow_mut();
                match player.block_or_timer {
                    BlockOrTimer::Timer(0) => panic!(),
                    BlockOrTimer::Timer(1) => true, // need reset
                    BlockOrTimer::Timer(n) => {
                        player.block_or_timer = BlockOrTimer::Timer(n - 1);
                        return true; // call again in 1sec
                    }
                    _ => false,
                }
            };
            if need_reset {
                self.clear_playing_area(i);
                self.new_block(i);
            }
        }
        false
    }

    fn clear_playing_area(&mut self, player_idx: usize) {
        match self.mode {
            Mode::Traditional | Mode::Adventure => {
                let w = self.get_width_per_player().unwrap();
                let h = self.get_height();
                let left = w * player_idx;
                let right = w * (player_idx + 1);
                for row in self.landed_rows[0..h].iter_mut() {
                    for square_ref in row[left..right].iter_mut() {
                        *square_ref = None;
                    }
                }
            }
            Mode::Bottle => {
                let left = BOTTLE_OUTER_WIDTH * player_idx;
                let right = left + BOTTLE_INNER_WIDTH;
                for row in self.landed_rows.iter_mut() {
                    for square_ref in row[left..right].iter_mut() {
                        *square_ref = None;
                    }
                }
            }
            Mode::Ring => {
                for y_abs in 0..=(RING_OUTER_RADIUS as i32) {
                    for x in (-y_abs)..=y_abs {
                        let point = self.players[player_idx]
                            .borrow()
                            .player_to_world((x, -y_abs));
                        if self.is_valid_landed_block_coords(point) {
                            self.set_landed_square(point, None);
                        }
                    }
                }
            }
        }
    }
}
