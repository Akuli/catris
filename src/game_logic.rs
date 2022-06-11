use crate::ansi::KeyPress;
use crate::blocks::MovingBlock;
use crate::blocks::SquareContent;
use crate::lobby::ClientInfo;
use crate::lobby::MAX_CLIENTS_PER_LOBBY;
use crate::player::BlockOrTimer;
use crate::player::Player;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;

pub type PlayerPoint = (i32, i32); // must be big, these don't wrap around in ring mode
pub type WorldPoint = (i16, i16);
pub type BlockRelativeCoords = (i8, i8);

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

pub struct Game {
    pub players: Vec<RefCell<Player>>,
    pub flashing_points: HashMap<WorldPoint, u8>,
    mode_specific_data: ModeSpecificData,
    score: usize,
    bomb_id_counter: u64,
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
            bomb_id_counter: 0,
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

    fn update_spawn_points(&self) {
        match self.mode() {
            Mode::Traditional => {
                let w = self.get_width_per_player() as i32;
                for (player_idx, player) in self.players.iter().enumerate() {
                    let i = player_idx as i32;
                    player.borrow_mut().spawn_point = ((i * w) + (w / 2), 0);
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
            .push(RefCell::new(Player::new((0, 0), client_info, self.score)));
        self.update_spawn_points();

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

        self.update_spawn_points();
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
                            full_points.push((x as i16, y as i16));
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
                let w = self.get_width() as i16;
                let h = self.get_height() as i16;
                (0..w).contains(&x) && (0..h).contains(&y)
            }
            _ => panic!(),
        }
    }

    pub fn get_moving_square(
        &self,
        point: WorldPoint,
        exclude_player_idx: Option<usize>,
    ) -> Option<(SquareContent, BlockRelativeCoords)> {
        for (player_idx, player) in self.players.iter().enumerate() {
            if exclude_player_idx != Some(player_idx) {
                match &player.borrow().block_or_timer {
                    BlockOrTimer::Block(block) => {
                        for (player_coords, relative_coords) in block
                            .get_coords()
                            .iter()
                            .zip(block.get_relative_coords().iter())
                        {
                            if player.borrow().player_to_world(*player_coords) == point {
                                return Some((block.square_content, *relative_coords));
                            }
                        }
                    }
                    _ => {}
                }
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

    pub fn get_any_square(
        &self,
        point: WorldPoint,
        exclude_player_idx: Option<usize>,
    ) -> Option<SquareContent> {
        let landed = if self.is_valid_landed_block_coords(point) {
            self.get_landed_square(point)
        } else {
            None
        };
        landed.or_else(|| {
            self.get_moving_square(point, exclude_player_idx)
                .map(|(content, _)| content)
        })
    }

    // TODO: delete
    fn square_is_occupied(&self, point: WorldPoint, exclude_player_idx: Option<usize>) -> bool {
        self.get_any_square(point, exclude_player_idx).is_some()
    }

    fn rotate_if_possible(&self, player_idx: usize, prefer_counter_clockwise: bool) -> bool {
        let player = &self.players[player_idx];
        let coords = match &player.borrow().block_or_timer {
            BlockOrTimer::Block(block) => block.get_rotated_coords(prefer_counter_clockwise),
            _ => return false,
        };

        let can_rotate = coords.iter().all(|p| {
            let stays_in_bounds = self.is_valid_moving_block_coords(*p);
            let goes_on_top_of_something =
                self.square_is_occupied(player.borrow().player_to_world(*p), Some(player_idx));
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
                let stays_in_bounds = self.is_valid_moving_block_coords(*p);
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

        // 40 is senough even in ring mode
        for _ in 0..40 {
            let can_move = working_coords.iter().all(|p| {
                let (x, mut y) = *p;
                y += 1;

                let stays_in_bounds = self.is_valid_moving_block_coords((x, y));
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

        let mut landing = vec![];
        for player_idx in drill_indexes.iter().chain(other_indexes.iter()) {
            let player = &self.players[*player_idx];
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
                for (w, r) in world_coords.iter().zip(relative_coords.iter()) {
                    landing.push((*w, square_content.to_landed_content(*r)));
                }
                self.new_block(*player_idx);
            } else {
                // no room to land
                player.borrow_mut().block_or_timer = BlockOrTimer::TimerPending;
            }
            need_render = true;
        }

        for (point, content) in landing {
            self.set_landed_square(point, Some(content));
        }

        need_render
    }

    pub fn animate_drills(&mut self) -> bool {
        let mut something_changed = false;
        let mut handle_block = |block: &mut MovingBlock| {
            if block.square_content.animate() {
                something_changed = true;
            }
        };

        for player_ref in &self.players {
            let mut player = player_ref.borrow_mut();
            match &mut player.block_or_timer {
                BlockOrTimer::Block(b) => handle_block(b),
                _ => {}
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
                block = replace(&mut player.next_block, MovingBlock::new(self.score));
            }
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
                replace(b, MovingBlock::new(self.score))
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

            if let BlockOrTimer::Block(moving_block) = &player.block_or_timer {
                player_coords = moving_block.get_coords();
                world_coords = player_coords
                    .iter()
                    .map(|p| player.player_to_world(*p))
                    .collect();
            }

            if let BlockOrTimer::Block(moving_block) = &mut player.block_or_timer {
                let old_len = player_coords.len();
                assert!(old_len != 0);

                // see example in retain docs
                let mut world_coord_iter = world_coords.iter();
                player_coords.retain(|_| {
                    f(
                        *world_coord_iter.next().unwrap(),
                        &mut moving_block.square_content,
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
                    moving_block.set_player_coords(&player_coords, moving_block.center);
                }
            }
        }

        for player_idx in need_new_block {
            self.new_block(player_idx);
        }

        match &mut self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                for (y, row) in landed_rows.iter_mut().enumerate() {
                    for (x, cell) in row.iter_mut().enumerate() {
                        let point = (x as i16, y as i16);
                        if let Some(content) = cell {
                            if !f(point, content, None) {
                                *cell = None;
                            }
                        }
                    }
                }
            }
        }
    }

    // returns bomb locations that were affected
    pub fn finish_explosion(
        &mut self,
        old_bomb_points: &Vec<WorldPoint>,
        old_flashing_points: &Vec<WorldPoint>,
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
        let mut moving_block_timer_decremented: Vec<bool> = vec![];
        moving_block_timer_decremented.resize(self.players.len(), false);

        self.filter_and_mutate_all_squares_in_place(|point, square_content, player_idx| {
            match square_content {
                SquareContent::Bomb { id, timer } if *id == Some(bomb_id) => {
                    found_bombs = true;
                    // timer can already be zero, if other bombs are exploding (holds async lock)
                    if *timer > 0
                        && (player_idx.is_none()
                            || !moving_block_timer_decremented[player_idx.unwrap()])
                    {
                        *timer -= 1;
                    }
                    if *timer == 0 {
                        result.push(point);
                    }
                    if let Some(i) = player_idx {
                        moving_block_timer_decremented[i] = true;
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
