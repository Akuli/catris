pub struct MovingBlock {
    // Relatively big ints. In ring mode (not implemented yet) these just grow as the blocks wrap around.
    pub center_x: i32,
    pub center_y: i32,
    pub relative_coords: Vec<(i8, i8)>,
}

pub struct Player {
    pub name: String,
    pub block: MovingBlock,
}

pub struct Game {
    pub players: Vec<Player>,
}

impl Game {
    pub fn move_blocks_down(&mut self) {
        for player in &mut self.players {
            for pair in &mut player.block.relative_coords {
                pair.1 += 1;
            }
        }
    }
}
