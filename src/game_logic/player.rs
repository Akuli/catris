use crate::game_logic::blocks::FallingBlock;
use crate::game_logic::game::wrap_around;
use crate::game_logic::game::Mode;
use crate::game_logic::game::RING_OUTER_RADIUS;
use crate::game_logic::PlayerPoint;
use crate::game_logic::WorldPoint;
use crate::lobby::ClientInfo;

#[derive(Debug)]
pub enum BlockOrTimer {
    Block(FallingBlock),
    TimerPending,
    Timer(u8),
}
impl BlockOrTimer {
    pub fn get_coords(&self) -> Vec<PlayerPoint> {
        match self {
            BlockOrTimer::Block(block) => block.get_coords(),
            _ => vec![],
        }
    }
}

#[derive(Debug)]
pub struct Player {
    pub client_id: u64,
    pub name: String,
    pub color: u8,
    pub spawn_point: PlayerPoint,
    pub block_or_timer: BlockOrTimer,
    pub next_block_queue: Vec<FallingBlock>, // Never empty
    pub block_in_hold: Option<FallingBlock>,
    pub fast_down: bool,
    pub down_direction: WorldPoint, // this vector always has length 1
    game_mode: Mode,
}

impl Player {
    pub fn new(
        spawn_point: PlayerPoint,
        client_info: &ClientInfo,
        down_direction: WorldPoint,
        game_mode: Mode,
        first_block: FallingBlock,
        second_block: FallingBlock,
    ) -> Self {
        Self {
            client_id: client_info.client_id,
            name: client_info.name.to_string(),
            color: client_info.color,
            spawn_point,
            block_or_timer: BlockOrTimer::Block(first_block),
            next_block_queue: vec![second_block],
            block_in_hold: None,
            fast_down: false,
            down_direction,
            game_mode,
        }
    }

    pub fn get_name_string(&self, max_len: usize) -> String {
        let mut name = self.name.clone();
        loop {
            let formatted: String = match self.block_or_timer {
                BlockOrTimer::Timer(n) => format!("[{}] {}", name, n),
                _ => name.clone(),
            };
            if formatted.chars().count() <= max_len {
                return formatted;
            }

            assert!(!name.is_empty());
            name.pop();
        }
    }

    pub fn player_to_world(&self, point: PlayerPoint) -> WorldPoint {
        let (x, mut y) = point;
        wrap_around(self.game_mode, &mut y);
        let x = x as i16;
        let y = y as i16;
        let (down_x, down_y) = self.down_direction;
        let (offset_x, offset_y) = match self.game_mode {
            Mode::Traditional | Mode::Bottle => (0, 0),
            Mode::Ring => (RING_OUTER_RADIUS, RING_OUTER_RADIUS),
        };

        // a couple ways to derive this: complex number multiplication, rotation matrices
        // to check, it should return the point unchanged when down_direction is the usual (0,1)
        // also, rotating (x,y) or (down_x,down_y) should rotate the result similarly
        let rotated_x = x * down_y + y * down_x;
        let rotated_y = -x * down_x + y * down_y;

        (offset_x + rotated_x, offset_y + rotated_y)
    }
}
