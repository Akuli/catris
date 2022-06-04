use crate::ansi::Color;
use crate::ansi::KeyPress;
use crate::blocks::BlockOrTimer;
use crate::blocks::MovingBlock;
use crate::blocks::SquareContent;
use crate::lobby::ClientInfo;
use crate::lobby::MAX_CLIENTS_PER_LOBBY;
use crate::player::Player;
use crate::player::PlayerPoint;
use crate::player::WorldPoint;
use crate::render::RenderBuffer;
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
    players: Vec<RefCell<Player>>,
    pub flashing_points: HashMap<WorldPoint, u8>,
    mode_specific_data: ModeSpecificData,
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
        }
    }

    pub fn mode(&self) -> Mode {
        match &self.mode_specific_data {
            ModeSpecificData::Traditional { .. } => Mode::Traditional,
        }
    }

    fn get_width_per_player(&self) -> usize {
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

    fn get_width(&self) -> usize {
        match self.mode() {
            Mode::Traditional => self.get_width_per_player() * self.players.len(),
            _ => unimplemented!(),
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
                BlockOrTimer::Timer(_) => {}
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

    pub fn get_player_count(&self) -> usize {
        self.players.len()
    }

    pub fn find_full_rows(&self) -> Vec<WorldPoint> {
        match &self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                let mut full_points = vec![];
                for (y, row) in landed_rows.iter().enumerate() {
                    if !row.iter().any(|cell| cell.is_none()) {
                        for (x, _) in row.iter().enumerate() {
                            full_points.push((x as i8, y as i8));
                        }
                    }
                }
                full_points
            }
        }
    }

    fn is_valid_moving_block_coords(&self, point: PlayerPoint) -> bool {
        match &self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                let (x, y) = point;
                let w = self.get_width() as i32;
                let h = landed_rows.len() as i32;
                (0..w).contains(&x) && (..h).contains(&y)
            }
        }
    }

    fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool {
        match &self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                let (x, y) = point;
                let w = self.get_width() as i8;
                let h = landed_rows.len() as i8;
                (0..w).contains(&x) && (0..h).contains(&y)
            }
        }
    }

    fn square_belongs_to_player(&self, player_idx: usize, point: WorldPoint) -> bool {
        match self.mode() {
            Mode::Traditional => {
                let (x, _) = point;
                let start = (player_idx * self.get_width_per_player()) as i8;
                let end = ((player_idx + 1) * self.get_width_per_player()) as i8;
                (start..end).contains(&x)
            }
            _ => unimplemented!(),
        }
    }

    // returns the location of world coords (0,0) within the buffer
    fn render_world_edges_to_buf(&self, buffer: &mut RenderBuffer) -> (i8, i8) {
        match &self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                for y in 0..landed_rows.len() {
                    buffer.set_char(0, y, '|');
                    buffer.set_char(2 * self.get_width() + 1, y, '|');
                }
                (1, 0)
            }
        }
    }

    fn get_landed_square(&self, point: WorldPoint) -> Option<SquareContent> {
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

    fn predict_landing_place(&self, player_idx: usize) -> Vec<WorldPoint> {
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

    pub fn render_to_buf(&self, client_id: u64, buffer: &mut RenderBuffer) {
        let player_idx = self
            .players
            .iter()
            .position(|cell| cell.borrow().client_id == client_id)
            .unwrap();

        let (offset_x, offset_y) = self.render_world_edges_to_buf(buffer);
        let trace_points = self.predict_landing_place(player_idx);

        // TODO: optimize lol?
        for x in i8::MIN..i8::MAX {
            for y in i8::MIN..i8::MAX {
                if !self.is_valid_landed_block_coords((x, y)) {
                    continue;
                }

                // If flashing, display the flashing
                let mut content = self
                    .flashing_points
                    .get(&(x, y))
                    .map(|color| SquareContent {
                        text: [' ', ' '],
                        color: Color { fg: 0, bg: *color },
                    });

                // If not flashing and there's a player's block, show that
                if content.is_none() {
                    for player in &self.players {
                        match &player.borrow().block_or_timer {
                            BlockOrTimer::Block(block) => {
                                if block
                                    .get_coords()
                                    .iter()
                                    .any(|p| player.borrow().player_to_world(*p) == (x, y))
                                {
                                    content = Some(block.get_square_content());
                                    break;
                                }
                            }
                            BlockOrTimer::Timer(_) => {}
                        }
                    }
                }

                // If still nothing found, use landed squares or leave empty.
                // These are the only ones that can get trace markers "::" on top of them.
                // Traces of drill blocks usually go on top of landed squares.
                if content.is_none() {
                    let mut traced_content =
                        self.get_landed_square((x, y)).unwrap_or(SquareContent {
                            text: [' ', ' '],
                            color: Color::DEFAULT,
                        });
                    if trace_points.contains(&(x, y))
                        && traced_content.text[0] == ' '
                        && traced_content.text[1] == ' '
                    {
                        traced_content.text[0] = ':';
                        traced_content.text[1] = ':';
                    }
                    content = Some(traced_content);
                };

                let content = content.unwrap();
                buffer.set_char_with_color(
                    (2 * x + offset_x) as usize,
                    (y + offset_y) as usize,
                    content.text[0],
                    content.color,
                );
                buffer.set_char_with_color(
                    (2 * x + offset_x) as usize + 1,
                    (y + offset_y) as usize,
                    content.text[1],
                    content.color,
                );
            }
        }
    }

    pub fn move_blocks_down(&mut self, fast: bool) -> bool {
        let mut landing = vec![];
        let mut need_render = false;

        for (player_idx, player) in self.players.iter().enumerate() {
            if player.borrow().fast_down != fast {
                continue;
            }
            need_render = true;

            if self.move_if_possible(player_idx, 0, 1) {
                continue;
            }

            let (player_coords, square_content) = match &player.borrow().block_or_timer {
                BlockOrTimer::Block(b) => (b.get_coords(), b.get_square_content()),
                BlockOrTimer::Timer(_) => continue,
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
                player.block_or_timer = BlockOrTimer::Timer(30);
                // TODO: start a timer task somehow
                //self.client_ids_starting_timer.push(player.client_id);
            }

            player.borrow_mut().fast_down = false;
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
            _ => {
                println!("Unhandled Key Press!! {:?}", key);
                false
            }
        };

        let mut player = self.players[player_idx].borrow_mut();
        player.fast_down = false;
        need_render
    }

    pub fn remove_full_rows(&mut self, full: &[WorldPoint]) {
        let w = self.get_width();

        match &mut self.mode_specific_data {
            ModeSpecificData::Traditional { landed_rows } => {
                let mut should_wipe = vec![];
                should_wipe.resize(landed_rows.len(), false);
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

    pub fn new_block(&self, player_idx: usize) {
        let mut player = self.players[player_idx].borrow_mut();
        player.block_or_timer = BlockOrTimer::Block(MovingBlock::new(player.spawn_point));
        // TODO: start please wait countdown if there are overlaps
    }
}
