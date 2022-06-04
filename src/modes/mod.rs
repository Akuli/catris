use std::cell::RefCell;
use std::collections::HashMap;

use crate::ansi::Color;
use crate::ansi::KeyPress;
use crate::lobby::ClientInfo;
use crate::lobby::MAX_CLIENTS_PER_LOBBY;
use crate::logic_base::Player;
use crate::logic_base::PlayerPoint;
use crate::logic_base::SquareContent;
use crate::logic_base::WorldPoint;
use crate::modes::traditional::TraditionalGame;
use crate::render;

mod traditional;

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum GameMode {
    Traditional,
    Bottle,
    Ring,
}

impl GameMode {
    pub const ALL_MODES: &'static [GameMode] =
        &[GameMode::Traditional, GameMode::Bottle, GameMode::Ring];

    pub fn name(self) -> &'static str {
        match self {
            GameMode::Traditional => "Traditional game",
            GameMode::Bottle => "Bottle game",
            GameMode::Ring => "Ring game",
        }
    }

    pub fn max_players(self) -> usize {
        match self {
            GameMode::Traditional | GameMode::Bottle => MAX_CLIENTS_PER_LOBBY,
            GameMode::Ring => 4,
        }
    }
}

#[impl_enum::with_methods {
    pub fn add_player(&mut self, client_info: &ClientInfo) {}
    pub fn remove_player_if_exists(&mut self, client_id: u64) {}
    fn is_valid_moving_block_coords(&self, point: PlayerPoint) -> bool {}
    fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool {}
    fn square_belongs_to_player(&self, player_idx: usize, point: WorldPoint) -> bool {}
    fn get_landed_square(&self, location: WorldPoint) -> Option<SquareContent> {}
    fn set_landed_square(&mut self, location: WorldPoint, value: Option<SquareContent>) {}
    // Returns location in buf where (0,0) in game maps
    fn render_world_edges_to_buf(&self, buffer: &mut render::Buffer) -> (i8, i8) {}
    pub fn find_full_rows(&self) -> Vec<WorldPoint> {}
    pub fn remove_full_rows(&mut self, full_points: &[WorldPoint]) {}
}]
pub enum AnyGame {
    Traditional(TraditionalGame),
}

impl AnyGame {
    pub fn new(mode: GameMode) -> AnyGame {
        match mode {
            GameMode::Traditional => AnyGame::Traditional(TraditionalGame::new()),
            _ => panic!("not implemented"),
        }
    }

    pub fn mode(&self) -> GameMode {
        match self {
            AnyGame::Traditional(_) => GameMode::Traditional,
        }
    }

    fn get_players(&self) -> &Vec<RefCell<Player>> {
        match self {
            AnyGame::Traditional(game) => &game.players,
        }
    }

    pub fn get_flashing_points(&self) -> &HashMap<WorldPoint, u8> {
        match self {
            AnyGame::Traditional(game) => &game.flashing_points,
        }
    }

    pub fn get_flashing_points_mut(&mut self) -> &mut HashMap<WorldPoint, u8> {
        match self {
            AnyGame::Traditional(game) => &mut game.flashing_points,
        }
    }

    pub fn get_player_count(&self) -> usize {
        self.get_players().len()
    }

    fn square_is_occupied(&self, point: WorldPoint, exclude_player_idx: Option<usize>) -> bool {
        (self.is_valid_landed_block_coords(point) && self.get_landed_square(point).is_some())
            || self.get_players().iter().enumerate().any(|(i, player)| {
                exclude_player_idx != Some(i)
                    && player
                        .borrow()
                        .block
                        .get_player_coords()
                        .iter()
                        .any(|p| player.borrow().player_to_world(*p) == point)
            })
    }

    fn rotate_if_possible(&self, player_idx: usize) -> bool {
        let player = &self.get_players()[player_idx];
        let coords = player.borrow().block.get_rotated_coords();
        let can_rotate = coords.iter().all(|p| {
            let stays_in_bounds = self.is_valid_moving_block_coords(*p);
            let goes_on_top_of_something =
                self.square_is_occupied(player.borrow().player_to_world(*p), Some(player_idx));
            stays_in_bounds && !goes_on_top_of_something
        });
        if can_rotate {
            player.borrow_mut().block.rotate();
        }
        can_rotate
    }

    fn move_if_possible(&self, player_idx: usize, dx: i8, dy: i8) -> bool {
        let player = &self.get_players()[player_idx];
        let coords = player.borrow().block.get_moved_coords(dx, dy);
        let can_move = coords.iter().all(|p| {
            let stays_in_bounds = self.is_valid_moving_block_coords(*p);
            let goes_on_top_of_something =
                self.square_is_occupied(player.borrow().player_to_world(*p), Some(player_idx));
            stays_in_bounds && !goes_on_top_of_something
        });
        if can_move {
            player.borrow_mut().block.m0v3(dx, dy);
        }
        can_move
    }

    fn predict_landing_place(&self, player_idx: usize) -> Vec<WorldPoint> {
        let player = &self.get_players()[player_idx];

        let mut working_coords: Option<Vec<WorldPoint>> = None;
        for offset in 0..40 {
            // enough even in ring mode
            let coords = player.borrow().block.get_moved_coords(0, offset);
            let can_move = coords.iter().all(|p| {
                let stays_in_bounds = self.is_valid_moving_block_coords(*p);
                let goes_on_top_of_something =
                    self.square_is_occupied(player.borrow().player_to_world(*p), Some(player_idx));
                stays_in_bounds && !goes_on_top_of_something
            });
            if can_move {
                working_coords = Some(
                    coords
                        .iter()
                        .map(|p| player.borrow().player_to_world(*p))
                        .collect(),
                );
            } else {
                // offset 0 always works, so it shouldn't be None
                return working_coords.unwrap();
            }
        }

        // Block won't land if it moves down. Happens a lot in ring mode.
        return vec![];
    }

    pub fn render_to_buf(&self, client_id: u64, buffer: &mut render::Buffer) {
        let player_idx = self
            .get_players()
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

                let content_that_doesnt_get_tracing_marks = self
                    .get_flashing_points()
                    .get(&(x, y))
                    .map(|color| SquareContent {
                        text: [' ', ' '],
                        color: Color { fg: 0, bg: *color },
                    })
                    .or_else(|| {
                        // This point isn't flashing, check if some player's block is here
                        self.get_players()
                            .iter()
                            .find(|player| {
                                player
                                    .borrow()
                                    .block
                                    .get_player_coords()
                                    .iter()
                                    .any(|p| player.borrow().player_to_world(*p) == (x, y))
                            })
                            .map(|player| player.borrow().block.get_square_content())
                    });

                let content = if let Some(c) = content_that_doesnt_get_tracing_marks {
                    c
                } else {
                    // Trace of a drill block can go on top of landed
                    // squares, but not on top of anything else
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
                    traced_content
                };

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

    pub fn move_blocks_down(&mut self, fast: bool) {
        let mut landing = vec![];

        for (player_idx, player) in self.get_players().iter().enumerate() {
            if player.borrow().fast_down == fast {
                if !self.move_if_possible(player_idx, 0, 1) {
                    // land
                    let player_coords = player.borrow().block.get_player_coords();
                    for player_point in player_coords {
                        let player = player.borrow();
                        let world_point = player.player_to_world(player_point);
                        let square_content = player.block.get_square_content();
                        landing.push((world_point, square_content));
                    }
                    player.borrow_mut().fast_down = false;
                    player.borrow_mut().new_block();
                }
            }
        }

        for (point, content) in landing {
            self.set_landed_square(point, Some(content));
        }
    }

    pub fn handle_key_press(&mut self, client_id: u64, key: KeyPress) -> bool {
        let player_idx = self
            .get_players()
            .iter()
            .position(|cell| cell.borrow().client_id == client_id)
            .unwrap();

        let need_render = match key {
            KeyPress::Down | KeyPress::Character('S') | KeyPress::Character('s') => {
                let mut player = self.get_players()[player_idx].borrow_mut();
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

        let mut player = self.get_players()[player_idx].borrow_mut();
        player.fast_down = false;
        need_render
    }

    pub fn remove_overlapping_landed_squares(&mut self) {
        let mut gonna_clear = vec![];
        for player in self.get_players() {
            let player_coords = player.borrow().block.get_player_coords();
            for player_point in player_coords {
                gonna_clear.push(player.borrow().player_to_world(player_point));
            }
        }

        for world_point in gonna_clear {
            if self.is_valid_landed_block_coords(world_point) {
                self.set_landed_square(world_point, None);
            }
        }
    }
}
