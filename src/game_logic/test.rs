// Unit test or integration test? You decide.
// If you view the game_logic/ subdirectory as a single unit, this is its unit test.
// If you view it as separate units, each file being a unit, this is an integration test.
use crate::ansi::Color;
use crate::client::ClientLogger;
use crate::game_logic::blocks::BlockType;
use crate::game_logic::blocks::FallingBlock;
use crate::game_logic::blocks::Shape;
use crate::game_logic::game::Game;
use crate::game_logic::game::Mode;
use crate::game_logic::player::BlockOrTimer;
use crate::lobby::ClientInfo;

fn dump_game_state(game: &Game) -> Vec<String> {
    let mut result = vec![];
    for y in 0..game.get_height() {
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

fn create_tiny_game(mode: Mode) -> Game {
    let mut game = Game::new(mode);
    game.truncate_height(3);
    game.add_player(&ClientInfo {
        name: "Alice".to_string(),
        client_id: 123,
        color: Color::RED_FOREGROUND.fg,
        logger: ClientLogger { client_id: 123 },
    });

    let mut block1 = FallingBlock::new(BlockType::Normal(Shape::L));
    let block2 = FallingBlock::new(BlockType::Normal(Shape::L));

    block1.spawn_at(game.players[0].borrow().spawn_point);
    game.players[0].borrow_mut().block_or_timer = BlockOrTimer::Block(block1);
    game.players[0].borrow_mut().next_block = block2;

    game
}

#[test]
fn test_spawning_and_landing_and_game_over() {
    let mut game = create_tiny_game(Mode::Traditional);

    // Blocks should spawn just on top of the game area.
    // It should take one move to make them partially visible.
    assert_eq!(
        dump_game_state(&game),
        [
            "                    ",
            "                    ",
            "                    ",
        ]
    );
    game.move_blocks_down(false);
    assert_eq!(
        dump_game_state(&game),
        [
            "        FFFFFF      ",
            "                    ",
            "                    ",
        ]
    );

    game.move_blocks_down(false);
    game.move_blocks_down(false);
    assert_eq!(
        dump_game_state(&game),
        [
            "                    ",
            "            FF      ",
            "        FFFFFF      ",
        ]
    );

    // This move lands the blocks and prepares a new block that is initially off-screen.
    game.move_blocks_down(false);
    assert_eq!(
        dump_game_state(&game),
        [
            "                    ",
            "            LL      ",
            "        LLLLLL      ",
        ]
    );
    game.move_blocks_down(false);
    assert_eq!(
        dump_game_state(&game),
        [
            "        FFFFFF      ",
            "            LL      ",
            "        LLLLLL      ",
        ]
    );

    // The block can't land because it doesn't fit. Player gets a pending timer.
    game.move_blocks_down(false);
    assert!(matches!(
        game.players[0].borrow().block_or_timer,
        BlockOrTimer::TimerPending
    ));
    // The not-fitting block disappeared. Not super important, feel free to change.
    assert_eq!(
        dump_game_state(&game),
        [
            "                    ",
            "            LL      ",
            "        LLLLLL      ",
        ]
    );

    // We can now query the players whose timer is pending.
    // Usually this would return client IDs, but it returns None to indicate game over.
    assert!(game.start_pending_please_wait_counters().is_none());
}
