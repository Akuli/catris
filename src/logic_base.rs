use rand::seq::SliceRandom;

use crate::ansi::Color;
use crate::lobby::ClientInfo;

#[derive(Copy, Clone)]
pub struct SquareContent {
    pub text: [char; 2],
    pub color: Color,
}

// Relatively big ints in player coords because in ring mode they just grow as blocks wrap around.
pub type PlayerPoint = (i32, i32);
pub type WorldPoint = (i8, i8);
type BlockRelativeCoords = (i8, i8);

#[rustfmt::skip]
const STANDARD_BLOCKS: &[(Color, &[BlockRelativeCoords])] = &[
    // Colors from here: https://tetris.fandom.com/wiki/Tetris_Guideline
    // The white block should be orange, but that would mean using colors
    // that don't work on windows cmd (I hope nobody actually uses this on cmd though)
    (Color::WHITE_BACKGROUND, &[(-1, 0), (0, 0), (1, 0), (1, -1)]),
    (Color::CYAN_BACKGROUND, &[(-2, 0), (-1, 0), (0, 0), (1, 0)]),
    (Color::BLUE_BACKGROUND, &[(-1, -1), (-1, 0), (0, 0), (1, 0)]),
    (Color::YELLOW_BACKGROUND, &[(-1, 0), (0, 0), (0, -1), (-1, -1)]),
    (Color::PURPLE_BACKGROUND, &[(-1, 0), (0, 0), (1, 0), (0, -1)]),
    (Color::RED_BACKGROUND, &[(-1, -1), (0, -1), (0, 0), (1, 0)]),
    (Color::GREEN_BACKGROUND, &[(1, -1), (0, -1), (0, 0), (-1, 0)]),
];

#[derive(Copy, Clone, Debug)]
enum RotateMode {
    NoRotating,
    NextCounterClockwiseThenBack,
    NextClockwiseThenBack,
    FullRotating,
}

// Checks if a and b are the same shape, but possibly in different locations.
fn shapes_match(a: &[BlockRelativeCoords], b: &[BlockRelativeCoords]) -> bool {
    // Also assumes that a and b don't have duplicates, couldn't figure out easy way to assert that
    assert!(a.len() != 0);
    assert!(b.len() != 0);
    assert!(a.len() == b.len());

    // Try to find the vector v that produces b when added to elements of a.
    // It is v = b[i]-a[j], where i and j are indexes of corresponding elements.
    let vx = b.iter().map(|(x, _)| x).min().unwrap() - a.iter().map(|(x, _)| x).min().unwrap();
    let vy = b.iter().map(|(_, y)| y).min().unwrap() - a.iter().map(|(_, y)| y).min().unwrap();
    let shifted_a: Vec<BlockRelativeCoords> = a.iter().map(|(ax, ay)| (ax + vx, ay + vy)).collect();
    return b.iter().all(|p| shifted_a.contains(p));
}

fn choose_initial_rotate_mode(not_rotated: &[BlockRelativeCoords]) -> RotateMode {
    let rotated_once: Vec<BlockRelativeCoords> =
        not_rotated.iter().map(|(x, y)| (-y, *x)).collect();
    if shapes_match(not_rotated, &rotated_once) {
        return RotateMode::NoRotating;
    }

    let rotated_twice: Vec<BlockRelativeCoords> =
        not_rotated.iter().map(|(x, y)| (-x, -y)).collect();
    if shapes_match(not_rotated, &rotated_twice) {
        return RotateMode::NextCounterClockwiseThenBack;
    }

    RotateMode::FullRotating
}

#[derive(Debug)]
pub struct MovingBlock {
    pub center: PlayerPoint,
    relative_coords: Vec<BlockRelativeCoords>,
    color: Color,
    rotate_mode: RotateMode,
}
impl MovingBlock {
    pub fn new(spawn_location: PlayerPoint) -> MovingBlock {
        let (color, coords) = STANDARD_BLOCKS.choose(&mut rand::thread_rng()).unwrap();
        MovingBlock {
            center: spawn_location,
            color: *color,
            relative_coords: coords.to_vec(),
            rotate_mode: choose_initial_rotate_mode(coords),
        }
    }

    pub fn get_square_content(&self) -> SquareContent {
        SquareContent {
            text: [' ', ' '],
            color: self.color,
        }
    }

    fn add_center(&self, relative: &[BlockRelativeCoords]) -> Vec<PlayerPoint> {
        let (cx, cy) = self.center;
        relative
            .iter()
            .map(|(dx, dy)| (cx + (*dx as i32), cy + (*dy as i32)))
            .collect()
    }

    pub fn get_player_coords(&self) -> Vec<PlayerPoint> {
        self.add_center(&self.relative_coords)
    }

    pub fn set_player_coords(&mut self, coords: &[PlayerPoint]) {
        let (cx, cy) = self.center;
        self.relative_coords = coords.iter().map(|(x, y)| ((x-cx) as i8, (y-cy) as i8)).collect();
    }

    fn get_moved_relative_coords(&self, dx: i8, dy: i8) -> Vec<BlockRelativeCoords> {
        self.relative_coords
            .iter()
            .map(|(x, y)| (x + dx, y + dy))
            .collect::<Vec<BlockRelativeCoords>>()
    }

    fn get_rotated_relative_coords(&self) -> Vec<BlockRelativeCoords> {
        match self.rotate_mode {
            RotateMode::NoRotating => self.relative_coords.clone(),
            // TODO: pressing r should switch rotate dir
            RotateMode::NextClockwiseThenBack | RotateMode::FullRotating => {
                self.relative_coords.iter().map(|(x, y)| (-y, *x)).collect()
            }
            RotateMode::NextCounterClockwiseThenBack => {
                self.relative_coords.iter().map(|(x, y)| (*y, -x)).collect()
            }
        }
    }

    pub fn get_moved_coords(&self, dx: i8, dy: i8) -> Vec<PlayerPoint> {
        self.add_center(&self.get_moved_relative_coords(dx, dy))
    }

    pub fn get_rotated_coords(&self) -> Vec<PlayerPoint> {
        self.add_center(&self.get_rotated_relative_coords())
    }

    // move is a keyword
    pub fn m0v3(&mut self, dx: i8, dy: i8) {
        let (cx, cy) = self.center;
        self.center = (cx + (dx as i32), cy + (dy as i32));
    }

    pub fn rotate(&mut self) {
        self.relative_coords = self.get_rotated_relative_coords();
        self.rotate_mode = match self.rotate_mode {
            RotateMode::NextCounterClockwiseThenBack => RotateMode::NextClockwiseThenBack,
            RotateMode::NextClockwiseThenBack => RotateMode::NextCounterClockwiseThenBack,
            other => other,
        };
    }
}

#[derive(Debug)]
pub struct Player {
    pub client_id: u64,
    pub name: String,
    pub color: u8,
    pub spawn_point: PlayerPoint,
    pub block: MovingBlock,
    pub fast_down: bool,
}
impl Player {
    pub fn new(spawn_point: PlayerPoint, client_info: &ClientInfo) -> Player {
        Player {
            client_id: client_info.client_id,
            name: client_info.name.to_string(),
            color: client_info.color,
            spawn_point: spawn_point,
            block: MovingBlock::new(spawn_point),
            fast_down: false,
        }
    }

    pub fn world_to_player(&self, point: WorldPoint) -> PlayerPoint {
        let (x, y) = point;
        (x as i32, y as i32)
    }

    pub fn player_to_world(&self, point: PlayerPoint) -> WorldPoint {
        let (x, y) = point;
        (x as i8, y as i8)
    }

    pub fn new_block(&mut self) {
        self.block = MovingBlock::new(self.spawn_point);
        println!("new block {:?}", self.block);
        // TODO: start please wait countdown if there are overlaps
    }
}
