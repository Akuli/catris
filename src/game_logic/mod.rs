// This module contains pure game logic. IO and async are done elsewhere.
pub mod blocks;
pub mod game;
pub mod player;

// PlayerPoint numbers must be big, they don't wrap around in ring mode
pub type PlayerPoint = (i32, i32); // player-specific in ring mode, (0,1) = downwards
pub type WorldPoint = (i16, i16); // the same for all players, differs from PlayerPoint only in ring mode
pub type BlockRelativeCoords = (i8, i8); // (0,0) = center of falling block

#[cfg(test)]
mod test {
    use crate::ansi::Color;
    use crate::client::ClientLogger;
    use crate::game_logic::blocks::BlockType;
    use crate::game_logic::blocks::FallingBlock;
    use crate::game_logic::blocks::Shape;
    use crate::game_logic::game::Game;
    use crate::game_logic::game::Mode;
    use crate::game_logic::player::BlockOrTimer;
    use crate::lobby::ClientInfo;

    fn dump_game_state(game: &Game, row_count: usize) -> Vec<String> {
        let mut result = vec![];
        for y in 0..row_count {
            let mut row = "".to_string();
            for x in 0..game.get_width() {
                if game.get_moving_square((x as i16, y as i16), None).is_some() {
                    row.push_str("FF");
                } else if game.get_landed_square((x as i16, y as i16)).is_some() {
                    row.push_str("LL");
                } else {
                    row.push_str("  ");
                }
            }
            result.push(row);
        }
        result
    }

    fn create_game(mode: Mode) -> Game {
        let mut game = Game::new(mode);
        game.add_player(&ClientInfo {
            name: "Alice".to_string(),
            client_id: 123,
            color: Color::RED_FOREGROUND.fg,
            logger: ClientLogger { client_id: 123 },
        });
        let mut block = FallingBlock::new(BlockType::Normal(Shape::L));
        block.spawn_at(game.players[0].borrow().spawn_point);
        game.players[0].borrow_mut().block_or_timer = BlockOrTimer::Block(block);
        game
    }

    #[test]
    fn test_spawning() {
        let mut game = create_game(Mode::Traditional);

        // Blocks should spawn just on top of the game area.
        // It should take one move to make them partially visible.
        assert_eq!(
            dump_game_state(&game, 3),
            [
                "                    ",
                "                    ",
                "                    "
            ]
        );
        game.move_blocks_down(false);
        assert_eq!(
            dump_game_state(&game, 3),
            [
                "        FFFFFF      ",
                "                    ",
                "                    "
            ]
        );
        game.move_blocks_down(false);
        assert_eq!(
            dump_game_state(&game, 3),
            [
                "            FF      ",
                "        FFFFFF      ",
                "                    "
            ]
        );
    }
}
