use crate::ansi::KeyPress;
use crate::blocks::MovingBlock;
use crate::blocks::SquareContent;
use crate::lobby::ClientInfo;
use crate::lobby::MAX_CLIENTS_PER_LOBBY;
use crate::player::BlockOrTimer;
use crate::player::Player;
use crate::player::PlayerPoint;
use crate::player::WorldPoint;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum Mode {
    Traditional,
    Bottle,
    Ring,
}

impl Mode {
    pub const ALL_MODES: &'static [Mode] = &[Mode::Traditional, Mode::Bottle, Mode::Ring];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Traditional => "Traditional game",
            Mode::Bottle => "Bottle game",
            Mode::Ring => "Ring game",
        }
    }

    pub fn max_players(self) -> usize {
        match self {
            Mode::Traditional | Mode::Bottle => MAX_CLIENTS_PER_LOBBY,
            Mode::Ring => 4,
        }
    }
}

enum ModeSpecificData {
    Traditional {
        landed_rows: [Vec<Option<SquareContent>>; 20],
    },
}

pub struct Game {
    pub players: Vec<RefCell<Player>>,
    pub flashing_points: HashMap<WorldPoint, u8>,
    mode_specific_data: ModeSpecificData,
    score: usize,
}

impl Game {
    pub fn new(mode: Mode) -> Self {
        let mode_specific_data = match mode {
            Mode::Traditional => {
                const BLANK: Vec<Option<SquareContent>> = vec![];
                ModeSpecificData::Traditional {
                    landed_rows: [BLANK; 20],
                }
            }
            Mode::Bottle | Mode::Ring => unimplemented!(),
        };
        Self {
            players: vec![],
            flashing_points: HashMap::new(),
            mode_specific_data,
            score: 0,
        }
    }

    pub fn mode(&self) -> Mode {
        match &self.mode_specific_data {
            ModeSpecificData::Traditional { .. } => Mode::Traditional,
        }
    }

    pub fn get_score(&self) -> usize {
        self.score
    }

    pub fn get_width_per_player(&self) -> usize {
        match self.mode() {
            Mode::Traditional => {
                // TODO: 10 would be wide enough for two
                if self.players.len() >= 2 {
                    7
                } else {
                    10
                }
            }
            _ => unimplemented!(),
        }
    }

    pub fn get_width(&self) -> usize {
        match self.mode() {
            Mode::Traditional => self.get_width_per_player() * self.players.len(),
            _ => unimplemented!(),
        }
    }

    pub fn get_height(&self) -> usize {
        match &self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => landed_rows.len(),
        }
    }

    fn update_spawn_places(&self) {
        match self.mode() {
            Mode::Traditional => {
                let w = self.get_width_per_player() as i32;
                for (player_idx, player) in self.players.iter().enumerate() {
                    let i = player_idx as i32;
                    player.borrow_mut().spawn_point.0 = (i * w) + (w / 2);
                }
            }
            _ => unimplemented!(),
        }
    }

    fn wipe_vertical_slice(&mut self, left: usize, width: usize) {
        let right = left + width;

        match &mut self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                for row in landed_rows.iter_mut() {
                    row.splice(left..right, vec![]);
                }
            }
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
                _ => {}
            }
        }
    }

    pub fn add_player(&mut self, client_info: &ClientInfo) {
        let player_idx = self.players.len();
        self.players
            .push(RefCell::new(Player::new((0, -1), client_info)));
        self.update_spawn_places();

        let w = self.get_width();
        match &mut self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                for row in landed_rows.iter_mut() {
                    row.resize(w, None);
                }
            }
        }

        self.new_block(player_idx);
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

        match self.mode_specific_data {
            ModeSpecificData::Traditional { .. } => {
                let slice_x = self.get_width_per_player() * i;
                let old_width = self.get_width();
                self.players.remove(i);
                let new_width = self.get_width();

                let slice_width = old_width - new_width;
                self.wipe_vertical_slice(slice_x, slice_width);
            }
        }

        self.update_spawn_places();
    }

    fn add_score(&mut self, single_player_score: usize) {
        /*
        It seems to be exponentially harder to get more points when there are a
        lot of players, basically P(all n players full) = P(1 player full)^n,
        although that wrongly assumes players are independent of each other.

        Currently this seems to give points more easily when there's a lot of
        players, but maybe that's a feature, because it should encourage people to
        play together :)

        The scores also feel quite different for single player and multiplayer.
        That's why they are shown separately in the high scores view.
        */
        self.score += single_player_score * 2usize.pow((self.players.len() - 1) as u32);
    }

    pub fn find_full_rows_and_increment_score(&mut self) -> Vec<WorldPoint> {
        match &self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                let mut full_points = vec![];
                let mut full_count = 0;

                for (y, row) in landed_rows.iter().enumerate() {
                    if !row.iter().any(|cell| cell.is_none()) {
                        full_count += 1;
                        for (x, _) in row.iter().enumerate() {
                            full_points.push((x as i8, y as i8));
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
                self.add_score(5 * full_count * (full_count + 1));

                full_points
            }
        }
    }

    fn is_valid_moving_block_coords(&self, point: PlayerPoint) -> bool {
        match self.mode() {
            Mode::Traditional => {
                let (x, y) = point;
                let w = self.get_width() as i32;
                let h = self.get_height() as i32;
                (0..w).contains(&x) && (..h).contains(&y)
            }
            _ => panic!(),
        }
    }

    pub fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool {
        match self.mode() {
            Mode::Traditional => {
                let (x, y) = point;
                let w = self.get_width() as i8;
                let h = self.get_height() as i8;
                (0..w).contains(&x) && (0..h).contains(&y)
            }
            _ => panic!(),
        }
    }

    pub fn get_moving_square(&self, point: WorldPoint) -> Option<SquareContent> {
        for player in &self.players {
            match &player.borrow().block_or_timer {
                BlockOrTimer::Block(block) => {
                    if block
                        .get_coords()
                        .iter()
                        .any(|p| player.borrow().player_to_world(*p) == point)
                    {
                        return Some(block.get_square_content());
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub fn get_landed_square(&self, point: WorldPoint) -> Option<SquareContent> {
        match &self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                let (x, y) = point;
                landed_rows[y as usize][x as usize]
            }
        }
    }

    fn set_landed_square(&mut self, point: WorldPoint, value: Option<SquareContent>) {
        match &mut self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                let (x, y) = point;
                landed_rows[y as usize][x as usize] = value;
            }
        }
    }

    fn square_is_occupied(&self, point: WorldPoint, exclude_player_idx: Option<usize>) -> bool {
        (self.is_valid_landed_block_coords(point) && self.get_landed_square(point).is_some())
            || self.players.iter().enumerate().any(|(i, player)| {
                exclude_player_idx != Some(i)
                    && player
                        .borrow()
                        .block_or_timer
                        .get_coords()
                        .iter()
                        .any(|p| player.borrow().player_to_world(*p) == point)
            })
    }

    fn rotate_if_possible(&self, player_idx: usize) -> bool {
        let player = &self.players[player_idx];
        let coords = player.borrow().block_or_timer.get_rotated_coords();

        let can_rotate = !coords.is_empty()
            && coords.iter().all(|p| {
                let stays_in_bounds = self.is_valid_moving_block_coords(*p);
                let goes_on_top_of_something =
                    self.square_is_occupied(player.borrow().player_to_world(*p), Some(player_idx));
                stays_in_bounds && !goes_on_top_of_something
            });
        if can_rotate {
            match &mut player.borrow_mut().block_or_timer {
                BlockOrTimer::Block(block) => block.rotate(),
                _ => panic!(),
            }
        }
        can_rotate
    }

    fn move_if_possible(&self, player_idx: usize, dx: i8, dy: i8) -> bool {
        let player = &self.players[player_idx];
        let coords = player.borrow().block_or_timer.get_moved_coords(dx, dy);

        let can_move = !coords.is_empty()
            && coords.iter().all(|p| {
                let stays_in_bounds = self.is_valid_moving_block_coords(*p);
                let goes_on_top_of_something =
                    self.square_is_occupied(player.borrow().player_to_world(*p), Some(player_idx));
                stays_in_bounds && !goes_on_top_of_something
            });
        if can_move {
            match &mut player.borrow_mut().block_or_timer {
                BlockOrTimer::Block(block) => block.m0v3(dx, dy),
                _ => panic!(),
            }
        }
        can_move
    }

    pub fn predict_landing_place(&self, player_idx: usize) -> Vec<WorldPoint> {
        let player = &self.players[player_idx];
        let mut working_coords: Vec<WorldPoint> = vec![];

        // 40 is senough even in ring mode
        for offset in 0..40 {
            let coords = player.borrow().block_or_timer.get_moved_coords(0, offset);

            let can_move = !coords.is_empty()
                && coords.iter().all(|p| {
                    let stays_in_bounds = self.is_valid_moving_block_coords(*p);
                    let goes_on_top_of_something = self
                        .square_is_occupied(player.borrow().player_to_world(*p), Some(player_idx));
                    stays_in_bounds && !goes_on_top_of_something
                });
            if can_move {
                working_coords = coords
                    .iter()
                    .map(|p| player.borrow().player_to_world(*p))
                    .collect();
            } else {
                // offset 0 always works, so it shouldn't be None
                return working_coords;
            }
        }

        // Block won't land if it moves down. Happens a lot in ring mode.
        return vec![];
    }

    pub fn move_blocks_down(&mut self, fast: bool) -> bool {
        let mut landing = vec![];
        let mut need_render = false;

        for (player_idx, player) in self.players.iter().enumerate() {
            if player.borrow().fast_down != fast {
                continue;
            }

            if self.move_if_possible(player_idx, 0, 1) {
                need_render = true;
                continue;
            }

            let (player_coords, square_content) = match &player.borrow().block_or_timer {
                BlockOrTimer::Block(b) => (b.get_coords(), b.get_square_content()),
                _ => continue,
            };

            let world_points: Vec<WorldPoint> = player_coords
                .iter()
                .map(|p| player.borrow().player_to_world(*p))
                .collect();
            if world_points
                .iter()
                .all(|p| self.is_valid_landed_block_coords(*p))
            {
                // land the block
                for p in world_points {
                    landing.push((p, square_content));
                }
                self.new_block(player_idx);
            } else {
                // no room to land
                let mut player = player.borrow_mut();
                player.block_or_timer = BlockOrTimer::TimerPending;
            }
            need_render = true;
        }

        for (point, content) in landing {
            self.set_landed_square(point, Some(content));
        }

        need_render
    }

    pub fn handle_key_press(&mut self, client_id: u64, key: KeyPress) -> bool {
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
                self.move_if_possible(player_idx, -1, 0)
            }
            KeyPress::Right | KeyPress::Character('D') | KeyPress::Character('d') => {
                self.move_if_possible(player_idx, 1, 0)
            }
            KeyPress::Up | KeyPress::Character('W') | KeyPress::Character('w') => {
                self.rotate_if_possible(player_idx)
            }
            KeyPress::Character('H') | KeyPress::Character('h') => self.hold_block(player_idx),
            _ => {
                println!("Unhandled Key Press!! {:?}", key);
                false
            }
        };

        self.players[player_idx].borrow_mut().fast_down = false;
        need_render
    }

    pub fn remove_full_rows(&mut self, full: &[WorldPoint]) {
        let w = self.get_width();
        let h = self.get_height();

        match &mut self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                let mut should_wipe = vec![];
                should_wipe.resize(h, false);
                for (_, y) in full {
                    should_wipe[*y as usize] = true;
                }

                for (y, wipe) in should_wipe.iter().enumerate() {
                    if *wipe {
                        landed_rows[..(y + 1)].rotate_right(1);
                        landed_rows[0].clear();
                        landed_rows[0].resize(w, None);
                    }
                }
            }
        }

        // Moving landed squares can cause them to overlap moving squares
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

    fn can_add_block(&self, player_idx: usize, block: &MovingBlock) -> bool {
        let overlaps = block.get_coords().iter().any(|p| {
            self.square_is_occupied(
                self.players[player_idx].borrow().player_to_world(*p),
                Some(player_idx),
            )
        });
        !overlaps
    }

    fn new_block_possibly_from_hold(&self, player_idx: usize, from_hold_if_possible: bool) {
        use std::mem::replace;

        let block = {
            let mut player = self.players[player_idx].borrow_mut();
            let mut block;
            if from_hold_if_possible && player.block_in_hold.is_some() {
                block = replace(&mut player.block_in_hold, None).unwrap();
            } else {
                block = replace(&mut player.next_block, MovingBlock::new());
            }
            block.center = player.spawn_point;
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
                replace(b, MovingBlock::new())
            }
            _ => return false,
        };
        self.new_block_possibly_from_hold(player_idx, true);
        to_hold.has_been_in_hold = true;
        self.players[player_idx].borrow_mut().block_in_hold = Some(to_hold);
        true
    }

    pub fn start_pending_please_wait_counters(&mut self) -> Vec<u64> {
        let mut client_ids = vec![];
        for player in &self.players {
            let mut player = player.borrow_mut();
            if matches!(player.block_or_timer, BlockOrTimer::TimerPending) {
                player.block_or_timer = BlockOrTimer::Timer(30);
                client_ids.push(player.client_id);
            }
        }

        client_ids
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
        let left = self.get_width_per_player() * player_idx;
        let right = self.get_width_per_player() * (player_idx + 1);

        match &mut self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                for row in landed_rows.iter_mut() {
                    for square_ref in row[left..right].iter_mut() {
                        *square_ref = None;
                    }
                }
            }
        }
    }
}
