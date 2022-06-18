use crate::ansi::Color;
use crate::game_logic::BlockRelativeCoords;
use crate::game_logic::PlayerPoint;
use crate::render::RenderBuffer;
use rand::distributions::Distribution;
use rand::distributions::WeightedIndex;
use rand::seq::SliceRandom;
use rand::Rng;

#[derive(Copy, Clone, Debug)]
enum DrillDirection {
    Upwards = 0,
    Downwards = 1,
    RightToLeft = 2,
    LeftToRight = 3,
}

#[rustfmt::skip]
const DRILL_PICTURES: [&[&[&str]]; 4] = [
    &[
        &[
            r" /\ ",
            r"|. |",
            r"| /|",
            r"|/ |",
            r"| .|",
        ],
        &[
            r" /\ ",
            r"| .|",
            r"|. |",
            r"| /|",
            r"|/ |",
        ],
        &[
            r" /\ ",
            r"|/ |",
            r"| .|",
            r"|. |",
            r"| /|",
        ],
        &[
            r" /\ ",
            r"| /|",
            r"|/ |",
            r"| .|",
            r"|. |",
        ],
    ],
    &[
        &[
            r"| /|",
            r"|/ |",
            r"| .|",
            r"|. |",
            r" \/ ",
        ],
        &[
            r"|/ |",
            r"| .|",
            r"|. |",
            r"| /|",
            r" \/ ",
        ],
        &[
            r"| .|",
            r"|. |",
            r"| /|",
            r"|/ |",
            r" \/ ",
        ],
        &[
            r"|. |",
            r"| /|",
            r"|/ |",
            r"| .|",
            r" \/ ",
        ],
    ],
    &[
        &[
            r" .--------",
            r"'._\__\__\",
        ],
        &[
            r" .--------",
            r"'.__\__\__",
        ],
        &[
            r" .--------",
            r"'.\__\__\_",
        ],
    ],
    &[
        &[
            r"--------. ",
            r"_/__/__/.'",
        ],
        &[
            r"--------. ",
            r"/__/__/_.'",
        ],
        &[
            r"--------. ",
            r"__/__/__.'",
        ],
    ],
];

fn choose_drill_direction(
    viewer_direction: (i8, i8),
    driller_direction: (i8, i8),
) -> DrillDirection {
    let (x, y) = viewer_direction;

    if driller_direction == (x, y) {
        DrillDirection::Downwards
    } else if driller_direction == (y, -x) {
        DrillDirection::LeftToRight
    } else if driller_direction == (-x, -y) {
        DrillDirection::Upwards
    } else if driller_direction == (-y, x) {
        DrillDirection::RightToLeft
    } else {
        panic!()
    }
}

fn direction_to_0123(direction: (i8, i8)) -> usize {
    match direction {
        (0, -1) => 0,
        (0, 1) => 1,
        (-1, 0) => 2,
        (1, 0) => 3,
        _ => panic!(),
    }
}

fn get_drill_text(
    animation_counter: u8,
    direction: DrillDirection,
    relative_coords: BlockRelativeCoords,
) -> &'static str {
    let p_index = direction as usize;
    let a_index = (animation_counter as usize) % (DRILL_PICTURES[p_index].len());

    // get nonnegative values for relative coords, easier to think about
    // rotating them will be messy anyway because width=2 and center is between the places
    let (mut relative_x, mut relative_y) = relative_coords;
    relative_x += 1;
    relative_y += 2;

    let (rotated_relative_x, rotated_relative_y) = match direction {
        DrillDirection::Downwards => (relative_x, relative_y),
        DrillDirection::Upwards => (1 - relative_x, 4 - relative_y),
        DrillDirection::RightToLeft => (4 - relative_y, relative_x),
        DrillDirection::LeftToRight => (relative_y, 1 - relative_x),
    };

    let x_index = (2 * rotated_relative_x) as usize;
    let y_index = rotated_relative_y as usize;
    &DRILL_PICTURES[p_index][a_index][y_index][x_index..(x_index + 2)]
}

#[derive(Copy, Clone, Debug)]
pub enum SquareContent {
    Normal([(char, Color); 2]),
    Bomb {
        timer: u8,
        id: Option<u64>,
    },
    MovingDrill {
        animation_counter: u8,
    },
    LandedDrill {
        texts_by_viewer_direction: [&'static str; 4], // indexed by direction_to_0123()
    },
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
                *animation_counter %= 12; // won't mess up 3-pic or 4-pic animations
                true
            }
            _ => false,
        }
    }

    pub fn get_landed_content(
        &self,
        relative_coords: BlockRelativeCoords,
        player_direction: (i8, i8),
    ) -> Self {
        match self {
            Self::MovingDrill { animation_counter } => {
                let mut texts_by_viewer_direction = ["", "", "", ""];
                for viewer_dir in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    texts_by_viewer_direction[direction_to_0123(viewer_dir)] = get_drill_text(
                        *animation_counter,
                        choose_drill_direction(viewer_dir, player_direction),
                        relative_coords,
                    );
                }
                Self::LandedDrill {
                    texts_by_viewer_direction,
                }
            }
            other => *other,
        }
    }

    pub fn get_trace_color(&self) -> Color {
        match self {
            Self::Bomb { timer, .. } => {
                if *timer > 3 {
                    Color::YELLOW_FOREGROUND
                } else {
                    Color::RED_FOREGROUND
                }
            }
            _ => Color::DEFAULT,
        }
    }

    // relative coords needed only for moving drill blocks
    pub fn render(
        &self,
        buffer: &mut RenderBuffer,
        x: usize,
        y: usize,
        /*
        (i8, i8) here are always unit vectors, i.e. one component zero and the other +-1.
        These represent the directions of players, and will be compared with each other.

        Moving blocks need to know what direction is down for the player who owns the moving block.
        All blocks need to know the direction of the player who will see the rendering result.
        */
        moving_block_data: Option<(BlockRelativeCoords, (i8, i8))>,
        viewer_direction: (i8, i8),
    ) {
        match self {
            Self::Normal(chars_and_colors) => {
                let (char1, color1) = chars_and_colors[0];
                let (char2, color2) = chars_and_colors[1];
                buffer.set_char_with_color(x, y, char1, color1);
                buffer.set_char_with_color(x + 1, y, char2, color2);
            }
            Self::Bomb { timer, .. } => {
                let color = self.get_trace_color();
                buffer.add_text_with_color(x, y, &format!("{:<2}", *timer), color);
            }
            Self::MovingDrill { animation_counter } => {
                let (relative_coords, driller_direction) = moving_block_data.unwrap();
                let direction = choose_drill_direction(viewer_direction, driller_direction);
                let text = get_drill_text(*animation_counter, direction, relative_coords);
                buffer.add_text(x, y, text);
            }
            Self::LandedDrill {
                texts_by_viewer_direction,
            } => {
                let text = texts_by_viewer_direction[direction_to_0123(viewer_direction)];
                buffer.add_text_with_color(x, y, text, Color::GRAY_BACKGROUND);
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
    assert!(!a.is_empty());
    assert!(!b.is_empty());
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

#[derive(Copy, Clone)]
pub enum BlockType {
    Normal,
    Cursed,
    Drill,
    Bomb,
}

impl BlockType {
    pub fn from_score(score: usize) -> Self {
        let score_kilos = score as f32 / 1000.0;

        let items = [
            // Weight x means it's x times as likely as normal block.
            (BlockType::Normal, 1.0),
            // Cursed blocks only appear at score>=500 and then become very common.
            // The intent is to surprise new players.
            (BlockType::Cursed, (score_kilos - 0.5).max(0.0) / 20.0),
            // Drills are rare, but always possible.
            // They're also very powerful when you happen to get one.
            (BlockType::Drill, score_kilos / 200.0),
            // Bombs are initially just 1% of normal squares.
            // But they get much more common as you get more points.
            (BlockType::Bomb, score_kilos / 80.0 + 0.01),
        ];
        let distribution = WeightedIndex::new(items.iter().map(|(_, weight)| weight)).unwrap();
        let index = distribution.sample(&mut rand::thread_rng());
        let (result, _) = items[index];
        result
    }
}

#[derive(Debug)]
pub struct FallingBlock {
    pub square_content: SquareContent,
    pub has_been_in_hold: bool,
    pub center: PlayerPoint,
    relative_coords: Vec<BlockRelativeCoords>,
    rotate_mode: RotateMode,
}
impl FallingBlock {
    pub fn new(block_type: BlockType) -> FallingBlock {
        let content;
        let mut coords;

        match block_type {
            BlockType::Normal => {
                let (color, coord_array) = STANDARD_BLOCKS.choose(&mut rand::thread_rng()).unwrap();
                content = SquareContent::Normal([(' ', *color), (' ', *color)]);
                coords = coord_array.to_vec();
            }
            BlockType::Cursed => {
                let (color, coord_array) = STANDARD_BLOCKS.choose(&mut rand::thread_rng()).unwrap();
                content = SquareContent::Normal([(' ', *color), (' ', *color)]);
                coords = coord_array.to_vec();
                add_extra_square(&mut coords);
            }
            BlockType::Drill => {
                content = SquareContent::MovingDrill {
                    animation_counter: 0,
                };
                coords = DRILL_COORDS.to_vec();
            }
            BlockType::Bomb => {
                content = SquareContent::Bomb {
                    timer: if maybe(20.0) { 3 } else { 15 },
                    id: None,
                };
                coords = O_COORDS.to_vec();
            }
        }

        FallingBlock {
            square_content: content,
            center: (0, 0), // dummy value, should be changed when spawning the block
            rotate_mode: choose_initial_rotate_mode(&coords, &content),
            relative_coords: coords,
            has_been_in_hold: false,
        }
    }

    pub fn from_score(score: usize) -> Self {
        Self::new(BlockType::from_score(score))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constructing() {
        let normal = FallingBlock::new(BlockType::Normal);
        let cursed = FallingBlock::new(BlockType::Cursed);
        let drill = FallingBlock::new(BlockType::Drill);
        let bomb = FallingBlock::new(BlockType::Bomb);

        assert!(normal.relative_coords.len() == 4);
        assert!(cursed.relative_coords.len() == 5);
        assert!(drill.relative_coords.len() == 10);
        assert!(bomb.relative_coords.len() == 4);

        assert!(drill.square_content.is_drill());
        assert!(bomb.square_content.is_bomb());
        assert!(!bomb.square_content.is_drill());
    }
}
