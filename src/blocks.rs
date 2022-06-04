use crate::ansi::Color;
use crate::player::PlayerPoint;
use rand::seq::SliceRandom;

#[derive(Copy, Clone)]
pub struct SquareContent {
    pub text: [char; 2],
    pub color: Color,
}

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
    (Color::MAGENTA_BACKGROUND, &[(-1, 0), (0, 0), (1, 0), (0, -1)]),
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

    pub fn get_coords(&self) -> Vec<PlayerPoint> {
        self.add_center(&self.relative_coords)
    }

    pub fn set_player_coords(&mut self, coords: &[PlayerPoint], new_center: PlayerPoint) {
        self.center = new_center;
        let (cx, cy) = new_center;
        self.relative_coords = coords
            .iter()
            .map(|(x, y)| ((x - cx) as i8, (y - cy) as i8))
            .collect();
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
