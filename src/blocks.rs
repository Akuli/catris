use crate::ansi::Color;
use crate::game_logic::BlockRelativeCoords;
use crate::game_logic::PlayerPoint;
use crate::render::RenderBuffer;
use rand::seq::SliceRandom;
use rand::Rng;

#[rustfmt::skip]
const DRILL_PICTURES: [[&str; 5]; 4] = [
    [
        r"| /|",
        r"|/ |",
        r"| .|",
        r"|. |",
        r" \/ ",
    ],
    [
        r"|/ |",
        r"| .|",
        r"|. |",
        r"| /|",
        r" \/ ",
    ],
    [
        r"| .|",
        r"|. |",
        r"| /|",
        r"|/ |",
        r" \/ ",
    ],
    [
        r"|. |",
        r"| /|",
        r"|/ |",
        r"| .|",
        r" \/ ",
    ],
];

fn get_drill_text(animation_counter: u8, relative_coords: BlockRelativeCoords) -> &'static str {
    let (relative_x, relative_y) = relative_coords;
    let a_index = animation_counter as usize;
    let x_index = (2 * (relative_x + 1)) as usize;
    let y_index = (relative_y + 2) as usize;
    &DRILL_PICTURES[a_index][y_index][x_index..(x_index + 2)]
}

#[derive(Copy, Clone, Debug)]
pub enum SquareContent {
    Normal([(char, Color); 2]),
    Bomb { timer: u8, id: Option<u64> },
    MovingDrill { animation_counter: u8 },
    LandedDrill { text: [char; 2] },
}
impl SquareContent {
    pub fn is_bomb(&self) -> bool {
        matches!(self, Self::Bomb { .. })
    }

    pub fn is_drill(&self) -> bool {
        matches!(self, Self::MovingDrill { .. } | Self::LandedDrill { .. })
    }

    pub fn can_drill(&self, other: &SquareContent) -> bool {
        self.is_drill() && !other.is_drill()
    }

    pub fn animate(&mut self) -> bool {
        match self {
            Self::MovingDrill { animation_counter } => {
                *animation_counter += 1;
                *animation_counter %= 4;
                true
            }
            _ => false,
        }
    }

    pub fn to_landed_content(&self, relative_coords: BlockRelativeCoords) -> Self {
        match self {
            Self::MovingDrill { animation_counter } => {
                let mut chars = get_drill_text(*animation_counter, relative_coords).chars();
                Self::LandedDrill {
                    text: [chars.next().unwrap(), chars.next().unwrap()],
                }
            }
            other => *other,
        }
    }

    // relative coords needed only for moving drill blocks
    pub fn render(
        &self,
        buffer: &mut RenderBuffer,
        x: usize,
        y: usize,
        relative_coords: Option<BlockRelativeCoords>,
    ) {
        match self {
            Self::Normal(chars_and_colors) => {
                let (char1, color1) = chars_and_colors[0];
                let (char2, color2) = chars_and_colors[1];
                buffer.set_char_with_color(x, y, char1, color1);
                buffer.set_char_with_color(x + 1, y, char2, color2);
            }
            Self::Bomb { timer, .. } => {
                let color = if *timer > 3 {
                    Color::YELLOW_FOREGROUND
                } else {
                    Color::RED_FOREGROUND
                };
                buffer.add_text_with_color(x, y, &format!("{:<2}", *timer), color);
            }
            Self::MovingDrill { animation_counter } => {
                let text = get_drill_text(*animation_counter, relative_coords.unwrap());
                buffer.add_text(x, y, text);
            }
            Self::LandedDrill { text } => {
                buffer.set_char_with_color(x, y, text[0], Color::GRAY_BACKGROUND);
                buffer.set_char_with_color(x + 1, y, text[1], Color::GRAY_BACKGROUND);
            }
        };
    }
}

const L_COORDS: &[BlockRelativeCoords] = &[(-1, 0), (0, 0), (1, 0), (1, -1)];
const I_COORDS: &[BlockRelativeCoords] = &[(-2, 0), (-1, 0), (0, 0), (1, 0)];
const J_COORDS: &[BlockRelativeCoords] = &[(-1, -1), (-1, 0), (0, 0), (1, 0)];
const O_COORDS: &[BlockRelativeCoords] = &[(-1, 0), (0, 0), (0, -1), (-1, -1)];
const T_COORDS: &[BlockRelativeCoords] = &[(-1, 0), (0, 0), (1, 0), (0, -1)];
const Z_COORDS: &[BlockRelativeCoords] = &[(-1, -1), (0, -1), (0, 0), (1, 0)];
const S_COORDS: &[BlockRelativeCoords] = &[(1, -1), (0, -1), (0, 0), (-1, 0)];

// x coordinates should be same as in O_COORDS
const DRILL_COORDS: &[BlockRelativeCoords] = &[
    (-1, -2),
    (0, -2),
    (-1, -1),
    (0, -1),
    (-1, 0),
    (0, 0),
    (-1, 1),
    (0, 1),
    (-1, 2),
    (0, 2),
];

#[rustfmt::skip]
const STANDARD_BLOCKS: &[(Color, &[BlockRelativeCoords])] = &[
    // Colors from here: https://tetris.fandom.com/wiki/Tetris_Guideline
    (Color::WHITE_BACKGROUND, L_COORDS),  // should be orange, but wouldn't work on windows cmd
    (Color::CYAN_BACKGROUND, I_COORDS),
    (Color::BLUE_BACKGROUND, J_COORDS),
    (Color::YELLOW_BACKGROUND, O_COORDS),
    (Color::MAGENTA_BACKGROUND, T_COORDS),
    (Color::RED_BACKGROUND, Z_COORDS),
    (Color::GREEN_BACKGROUND, S_COORDS),
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

fn choose_initial_rotate_mode(
    not_rotated: &[BlockRelativeCoords],
    content: &SquareContent,
) -> RotateMode {
    if content.is_drill() {
        return RotateMode::NoRotating;
    }

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

fn add_extra_square(coords: &mut Vec<BlockRelativeCoords>) {
    loop {
        let existing = coords.choose(&mut rand::thread_rng()).unwrap();
        let diff: BlockRelativeCoords = *[(-1, 0), (1, 0), (0, -1), (0, 1)]
            .choose(&mut rand::thread_rng())
            .unwrap();
        let (ex, ey) = existing;
        let (dx, dy) = diff;
        let shifted_point = (ex + dx, ey + dy);
        if !coords.contains(&shifted_point) {
            coords.push(shifted_point);
            return;
        }
    }
}

fn maybe(probability: f32) -> bool {
    rand::thread_rng().gen_range(0.0..100.0) < probability
}

#[derive(Debug)]
pub struct MovingBlock {
    pub square_content: SquareContent,
    pub has_been_in_hold: bool,
    pub center: PlayerPoint,
    relative_coords: Vec<BlockRelativeCoords>,
    rotate_mode: RotateMode,
}
impl MovingBlock {
    pub fn new(score: usize) -> MovingBlock {
        let score = score as f32;

        let bomb_probability = score / 800.0 + 1.0;
        let drill_probability = score / 2000.0;
        let cursed_probability = (score - 500.0) / 200.0;

        let (content, coords) = if maybe(bomb_probability) {
            let content = SquareContent::Bomb {
                timer: if maybe(20.0) { 3 } else { 15 },
                id: None,
            };
            (content, O_COORDS.to_vec())
        } else if maybe(drill_probability) {
            (
                SquareContent::MovingDrill {
                    animation_counter: 0,
                },
                DRILL_COORDS.to_vec(),
            )
        } else {
            let (color, coords) = STANDARD_BLOCKS.choose(&mut rand::thread_rng()).unwrap();
            let mut coords = coords.to_vec();
            if maybe(cursed_probability) {
                add_extra_square(&mut coords);
            }
            (
                SquareContent::Normal([(' ', *color), (' ', *color)]),
                coords,
            )
        };
        MovingBlock {
            square_content: content,
            center: (0, 0), // dummy value, should be changed when spawning the block
            rotate_mode: choose_initial_rotate_mode(&coords, &content),
            relative_coords: coords,
            has_been_in_hold: false,
        }
    }

    pub fn spawn_at(&mut self, spawn_point: PlayerPoint) {
        // Position the block just above the spawn point
        let (spawn_x, spawn_y) = spawn_point;
        let lowest_relative_y = *self.relative_coords.iter().map(|(_, y)| y).max().unwrap();
        let bottom_edge = (lowest_relative_y as i32) + 1;
        self.center = (spawn_x, spawn_y - bottom_edge);

        // spawned bombs get a new tick counter
        if let SquareContent::Bomb { id, .. } = &mut self.square_content {
            *id = None;
        }
    }

    pub fn get_relative_coords(&self) -> &[BlockRelativeCoords] {
        &self.relative_coords
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

    fn get_rotated_relative_coords(
        &self,
        prefer_counter_clockwise: bool,
    ) -> Vec<BlockRelativeCoords> {
        let counter_clockwise = match self.rotate_mode {
            RotateMode::NoRotating => return self.relative_coords.clone(),
            RotateMode::NextClockwiseThenBack => false,
            RotateMode::NextCounterClockwiseThenBack => true,
            RotateMode::FullRotating => prefer_counter_clockwise,
        };
        if counter_clockwise {
            self.relative_coords.iter().map(|(x, y)| (*y, -x)).collect()
        } else {
            self.relative_coords.iter().map(|(x, y)| (-y, *x)).collect()
        }
    }

    pub fn get_moved_coords(&self, dx: i8, dy: i8) -> Vec<PlayerPoint> {
        self.add_center(&self.get_moved_relative_coords(dx, dy))
    }

    pub fn get_rotated_coords(&self, prefer_counter_clockwise: bool) -> Vec<PlayerPoint> {
        self.add_center(&self.get_rotated_relative_coords(prefer_counter_clockwise))
    }

    // move is a keyword
    pub fn m0v3(&mut self, dx: i8, dy: i8) {
        let (cx, cy) = self.center;
        self.center = (cx + (dx as i32), cy + (dy as i32));
    }

    pub fn rotate(&mut self, prefer_counter_clockwise: bool) {
        self.relative_coords = self.get_rotated_relative_coords(prefer_counter_clockwise);
        self.rotate_mode = match self.rotate_mode {
            RotateMode::NextCounterClockwiseThenBack => RotateMode::NextClockwiseThenBack,
            RotateMode::NextClockwiseThenBack => RotateMode::NextCounterClockwiseThenBack,
            other => other,
        };
    }
}
