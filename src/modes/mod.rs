use std::cell::RefCell;
use std::collections::HashMap;

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
    fn get_square_contents(&self, exclude_player_idx: Option<usize>) -> HashMap<(i8, i8), SquareContent> {}
    fn is_valid_moving_block_coords(&self, point: PlayerPoint) -> bool {}
    fn is_valid_landed_block_coords(&self, point: WorldPoint) -> bool {}
    fn square_belongs_to_player(&self, player_idx: usize, point: WorldPoint) -> bool {}
    pub fn render_to_buf(&self, buffer: &mut render::Buffer) {}
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

    pub fn get_player_count(&self) -> usize { self.get_players().len()}

    fn get_landed_squares(&mut self) -> &mut HashMap<(i8, i8), SquareContent> {
        match self {
            AnyGame::Traditional(game) => &mut game.landed_squares,
        }
    }

    pub fn move_blocks_down(&mut self) {
        let square_contents: Vec<HashMap<(i8, i8), SquareContent>> = (0..self.get_players().len())
            .map(|i| self.get_square_contents(Some(i)))
            .collect();
        let mut landing = vec![];

        for (player, square_contents) in self.get_players().iter().zip(square_contents) {
            let new_relative_coords: Vec<PlayerPoint> = player
                .borrow()
                .block
                .relative_coords
                .iter()
                .map(|(x, y)| (*x, y + 1))
                .collect();
            let (spawn_x, spawn_y) = player.borrow().spawn_point;
            let new_player_coords: Vec<PlayerPoint> = new_relative_coords
                .iter()
                .map(|(dx, dy)| (spawn_x + dx, spawn_y + dy))
                .collect();

            let can_move = new_player_coords.iter().all(|p| {
                let stays_in_bounds = self.is_valid_moving_block_coords(*p);
                let goes_on_top_of_something =
                    square_contents.contains_key(&player.borrow().player_to_world(*p));
                if !stays_in_bounds {
                    println!("out uf bounds");
                }
                if goes_on_top_of_something {
                    println!("goes on top of something");
                }
                stays_in_bounds && !goes_on_top_of_something
            });

            if can_move {
                player.borrow_mut().block.relative_coords = new_relative_coords;
            } else {
                let player_coords = player.borrow().block.get_player_coords();
                for player_point in player_coords {
                    let world_point = player.borrow().player_to_world(player_point);
                    let square_contents = player.borrow().block.get_square_contents();
                    landing.push((world_point, square_contents));
                }
                player.borrow_mut().new_block();
            }
        }

        self.get_landed_squares().extend(landing);
    }

    pub fn handle_key_press(&mut self, _client_id: u64, key: KeyPress) -> bool {
        println!("Key Press!! {:?}", key);
        false
    }
}
