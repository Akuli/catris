use crate::blocks::MovingBlock;
use crate::lobby::ClientInfo;

// Relatively big ints in player coords because in ring mode they just grow as blocks wrap around.
pub type PlayerPoint = (i32, i32);
pub type WorldPoint = (i8, i8);

#[derive(Debug)]
pub enum BlockOrTimer {
    Block(MovingBlock),
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

    pub fn get_moved_coords(&self, dx: i8, dy: i8) -> Vec<PlayerPoint> {
        match self {
            BlockOrTimer::Block(block) => block.get_moved_coords(dx, dy),
            _ => vec![],
        }
    }

    pub fn get_rotated_coords(&self) -> Vec<PlayerPoint> {
        match self {
            BlockOrTimer::Block(block) => block.get_rotated_coords(),
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
    pub next_block: MovingBlock,
    pub fast_down: bool,
}

impl Player {
    pub fn new(spawn_point: PlayerPoint, client_info: &ClientInfo) -> Player {
        Player {
            client_id: client_info.client_id,
            name: client_info.name.to_string(),
            color: client_info.color,
            spawn_point: spawn_point,
            block_or_timer: BlockOrTimer::Block(MovingBlock::new()),
            next_block: MovingBlock::new(),
            fast_down: false,
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

    pub fn world_to_player(&self, point: WorldPoint) -> PlayerPoint {
        let (x, y) = point;
        (x as i32, y as i32)
    }

    pub fn player_to_world(&self, point: PlayerPoint) -> WorldPoint {
        let (x, y) = point;
        (x as i8, y as i8)
    }
}
