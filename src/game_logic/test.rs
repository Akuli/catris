use crate::ansi::Color;
use crate::client::ClientLogger;
use crate::game_logic::blocks::BlockType;
use crate::game_logic::blocks::FallingBlock;
use crate::game_logic::blocks::Shape;
use crate::game_logic::blocks::SquareContent;
use crate::game_logic::game::Game;
use crate::game_logic::game::Mode;
use crate::game_logic::player::BlockOrTimer;
use crate::lobby::ClientInfo;
use std::collections::HashSet;

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

fn create_game(mode: Mode, player_count: usize) -> Game {
    let mut game = Game::new(mode);
    game.set_block_factory(|_| FallingBlock::new(BlockType::Normal(Shape::L)));
    for i in 0..player_count {
        game.add_player(&ClientInfo {
            name: format!("Player {}", i),
            client_id: i as u64,
            color: Color::RED_FOREGROUND.fg,
            logger: ClientLogger {
                client_id: i as u64,
            },
        });
    }
    game
}

#[test]
fn test_spawning_and_landing_and_game_over() {
    let mut game = create_game(Mode::Traditional, 1);
    game.truncate_height(3);

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

    // We can now query whose timers are pending, but we get None to indicate game over.
    assert!(game.start_pending_please_wait_counters().is_none());
}

#[test]
fn test_wait_counters() {
    let mut game = create_game(Mode::Traditional, 2);
    game.truncate_height(3);

    game.move_blocks_down(false);
    game.move_blocks_down(false);
    game.move_blocks_down(false);
    game.move_blocks_down(false);
    game.move_blocks_down(false);
    assert_eq!(
        dump_game_state(&game),
        [
            "    FFFFFF        FFFFFF    ",
            "            LL        LL    ",
            "        LLLLLL    LLLLLL    ",
        ]
    );
    assert_eq!(game.start_pending_please_wait_counters(), Some(vec![]));

    // Player 0 (left) can still keep going, but player 1 (right) starts their 30sec waiting time
    game.move_blocks_down(false);
    assert_eq!(
        dump_game_state(&game),
        [
            "        FF                  ",
            "    FFFFFF  LL        LL    ",
            "        LLLLLL    LLLLLL    ",
        ]
    );
    assert!(matches!(
        game.players[1].borrow().block_or_timer,
        BlockOrTimer::TimerPending
    ));
    assert_eq!(game.start_pending_please_wait_counters(), Some(vec![1]));
    assert!(matches!(
        game.players[1].borrow().block_or_timer,
        BlockOrTimer::Timer(30)
    ));

    // During the next 30 seconds, the timer ticks from 30 to 1. Then the player gets a new block.
    for _ in 0..28 {
        assert!(game.tick_please_wait_counter(1));
    }
    assert!(matches!(
        game.players[1].borrow().block_or_timer,
        BlockOrTimer::Timer(2)
    ));
    assert!(game.tick_please_wait_counter(1));
    assert!(matches!(
        game.players[1].borrow().block_or_timer,
        BlockOrTimer::Timer(1)
    ));
    assert!(!game.tick_please_wait_counter(1));
    assert!(matches!(
        game.players[1].borrow().block_or_timer,
        BlockOrTimer::Block(_)
    ));
}

#[test]
fn test_traditional_clearing() {
    let mut game = create_game(Mode::Traditional, 2);
    game.truncate_height(5);
    for y in 1..5 {
        for x in 0..(game.get_width() as i16) {
            if (x, y) != (5, 2) && (x, y) != (8, 4) && (x, y) != (12, 4) {
                game.set_landed_square(
                    (x, y),
                    Some(SquareContent::with_color(Color::YELLOW_FOREGROUND)),
                );
            }
        }
    }
    let before_clear = vec![
        "                            ",
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLL",
        "LLLLLLLLLL  LLLLLLLLLLLLLLLL",
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLL",
        "LLLLLLLLLLLLLLLL  LLLLLL  LL",
    ];
    let after_clear = vec![
        "                            ",
        "                            ",
        "                            ",
        "LLLLLLLLLL  LLLLLLLLLLLLLLLL",
        "LLLLLLLLLLLLLLLL  LLLLLL  LL",
    ];
    assert_eq!(dump_game_state(&game), before_clear);

    assert_eq!(game.get_score(), 0);
    let full = game.find_full_rows_and_increment_score();
    // two full rows --> 10 for first + 20 for second
    // two players --> double score
    assert_eq!(game.get_score(), 60);

    assert_eq!(
        full.iter().map(|(x, _)| *x).collect::<HashSet<i16>>(),
        (0..(game.get_width() as i16)).collect::<HashSet<i16>>()
    );
    assert_eq!(
        full.iter().map(|(_, y)| *y).collect::<HashSet<i16>>(),
        HashSet::from([1, 3])
    );

    assert_eq!(dump_game_state(&game), before_clear);
    game.remove_full_rows(&full);
    assert_eq!(dump_game_state(&game), after_clear);
}
