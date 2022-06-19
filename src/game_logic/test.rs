use crate::ansi::Color;
use crate::client::ClientLogger;
use crate::game_logic::blocks::BlockType;
use crate::game_logic::blocks::FallingBlock;
use crate::game_logic::blocks::Shape;
use crate::game_logic::blocks::SquareContent;
use crate::game_logic::game::Game;
use crate::game_logic::game::Mode;
use crate::game_logic::player::BlockOrTimer;
use crate::game_logic::WorldPoint;
use crate::lobby::ClientInfo;
use std::collections::HashSet;

fn dump_game_state(game: &Game) -> Vec<String> {
    let mut result: Vec<String> = vec![];
    let (x_top, x_bottom, y_top, y_bottom) = game.get_bounds_in_player_coords();

    let mut x_coords: Vec<Option<i32>> = vec![];
    let mut y_coords: Vec<Option<i32>> = vec![];

    match game.mode {
        Mode::Traditional => {
            x_coords.append(&mut (x_top..x_bottom).map(Some).collect());
            y_coords.append(&mut (0..(game.get_height() as i32)).map(Some).collect());
        }
        Mode::Bottle => {
            x_coords.append(&mut (x_top..x_bottom).map(Some).collect());
            y_coords.append(&mut (0..4).map(Some).collect());
            y_coords.push(None);
            y_coords.append(&mut ((y_bottom - 4)..y_bottom).map(Some).collect());
        }
        Mode::Ring => {
            x_coords.append(&mut (y_top..(y_top + 3)).map(Some).collect());
            x_coords.push(None);
            x_coords.append(&mut (-7..=7).map(Some).collect());
            x_coords.push(None);
            x_coords.append(&mut ((y_bottom - 3)..y_bottom).map(Some).collect());
            y_coords = x_coords.clone();
        }
    }

    for y in &y_coords {
        if y.is_none() {
            result.push(result[0].chars().map(|_| '~').collect());
            continue;
        }
        let y = y.unwrap();

        let mut row = "".to_string();
        for x in &x_coords {
            if x.is_none() {
                row.push_str("~");
                continue;
            }
            let x = x.unwrap();

            let point = game.players[0].borrow().player_to_world((x, y));
            if !game.is_valid_landed_block_coords(point) {
                row.push_str("..");
            } else if game
                .get_falling_square((x as i16, y as i16), None)
                .is_some()
            {
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
    for _ in 0..29 {
        assert!(game.tick_please_wait_counter(1));
    }
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

    let mut expected_full: HashSet<WorldPoint> = HashSet::new();
    for y in [1, 3] {
        for x in 0..(game.get_width() as i16) {
            expected_full.insert((x, y));
        }
    }
    assert_eq!(HashSet::from_iter(full.iter().map(|p| *p)), expected_full);

    assert_eq!(dump_game_state(&game), before_clear);
    game.remove_full_rows(&full);
    assert_eq!(dump_game_state(&game), after_clear);
}

#[test]
fn test_bottle_clearing() {
    let mut game = create_game(Mode::Bottle, 2);
    for y in 0..3 {
        for x in 2..7 {
            if (x, y) != (3, 0) && (x, y) != (5, 2) {
                game.set_landed_square(
                    (x, y),
                    Some(SquareContent::with_color(Color::YELLOW_FOREGROUND)),
                );
            }
        }
    }
    for y in (game.get_height() as i16 - 3)..(game.get_height() as i16) {
        for x in 0..(game.get_width() as i16) {
            if (x, y) != (3, game.get_height() as i16 - 3)
                && (x, y) != (15, game.get_height() as i16 - 1)
            {
                game.set_landed_square(
                    (x, y),
                    Some(SquareContent::with_color(Color::YELLOW_FOREGROUND)),
                );
            }
        }
    }
    // Add unrelated squares to other player's area.
    // These should move by 1 unit when both players area is cleared.
    // If it moves down by 2 units, it is a bug.
    for x in 12..17 {
        game.set_landed_square(
            (x, x % 3),
            Some(SquareContent::with_color(Color::YELLOW_FOREGROUND)),
        );
    }

    let before_clear = vec![
        "....LL  LLLLLL..........LL    LL  ....",
        "....LLLLLLLLLL..........  LL    LL....",
        "....LLLLLL  LL..........    LL    ....",
        "....          ..........          ....",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "                  LL                  ",
        "LLLLLL  LLLLLLLLLLLLLLLLLLLLLLLLLLLLLL",
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL",
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLL  LLLLLL",
    ];
    let after_clear = vec![
        "....          ..........          ....",
        "....          ..........LL    LL  ....",
        "....LL  LLLLLL..........  LL    LL....",
        "....LLLLLL  LL..........    LL    ....",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "                  LL                  ",
        "                  LL                  ",
        "LLLLLL  LLLLLLLLLLLLLLLLLLLLLLLLLLLLLL",
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLL  LLLLLL",
    ];

    assert_eq!(dump_game_state(&game), before_clear);

    assert_eq!(game.get_score(), 0);
    let full = game.find_full_rows_and_increment_score();
    // 10 points for player-specific row, 2*10 for a row shared with two players
    assert_eq!(game.get_score(), 30);

    assert_eq!(dump_game_state(&game), before_clear);
    game.remove_full_rows(&full);
    assert_eq!(dump_game_state(&game), after_clear);
}

#[test]
fn test_ring_mode_clearing() {
    let mut game = create_game(Mode::Ring, 2);
    for x in -6..=6 {
        for y in -6..=6 {
            if game.is_valid_landed_block_coords((x, y)) && (x, y) != (5, -2) {
                game.set_landed_square(
                    (x, y),
                    Some(SquareContent::with_color(Color::YELLOW_FOREGROUND)),
                );
            }
        }
    }

    // These unrelated squares would be deleted if the clears were done in the wrong order
    game.set_landed_square(
        (6, -7),
        Some(SquareContent::with_color(Color::YELLOW_FOREGROUND)),
    );
    game.set_landed_square(
        (0, -7),
        Some(SquareContent::with_color(Color::YELLOW_FOREGROUND)),
    );

    let before_clear = vec![
        "......~                              ~......",
        "......~                              ~......",
        "......~                              ~......",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "      ~              LL          LL  ~      ",
        "      ~  LLLLLLLLLLLLLLLLLLLLLLLLLL  ~      ",
        "      ~  LLLLLLLLLLLLLLLLLLLLLLLLLL  ~      ",
        "      ~  LLLLLLLLLLLLLLLLLLLLLLLLLL  ~      ",
        "      ~  LLLLLL..............LLLLLL  ~      ",
        "      ~  LLLLLL..............LL  LL  ~      ",
        "      ~  LLLLLL..............LLLLLL  ~      ",
        "      ~  LLLLLL..............LLLLLL  ~      ",
        "      ~  LLLLLL..............LLLLLL  ~      ",
        "      ~  LLLLLL..............LLLLLL  ~      ",
        "      ~  LLLLLL..............LLLLLL  ~      ",
        "      ~  LLLLLLLLLLLLLLLLLLLLLLLLLL  ~      ",
        "      ~  LLLLLLLLLLLLLLLLLLLLLLLLLL  ~      ",
        "      ~  LLLLLLLLLLLLLLLLLLLLLLLLLL  ~      ",
        "      ~                              ~      ",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "......~                              ~......",
        "......~                              ~......",
        "......~                              ~......",
    ];
    let after_clear = vec![
        "......~                              ~......",
        "......~                              ~......",
        "......~                              ~......",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "      ~              LL        LL    ~      ",
        "      ~      LLLLLLLLLLLLLLLLLL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LL..............        ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LLLLLLLLLLLLLLLLLL      ~      ",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "......~                              ~......",
        "......~                              ~......",
        "......~                              ~......",
    ];
    assert_eq!(dump_game_state(&game), before_clear);

    assert_eq!(game.get_score(), 0);
    let full = game.find_full_rows_and_increment_score();
    // two rows, so 10+20, with double score because two players
    assert_eq!(game.get_score(), 60);

    game.remove_full_rows(&full);
    assert_eq!(dump_game_state(&game), after_clear);
}

// Sometimes, a clear in ring mode causes another clear to trigger.
// This is because inner rings are smaller, and shoving squares into smaller space can get rid of gaps.
#[test]
fn test_ring_mode_double_clear() {
    let mut game = create_game(Mode::Ring, 2);
    for x in -5..=5 {
        for y in -5..=5 {
            if game.is_valid_landed_block_coords((x, y)) && (x.abs() != 5 || y.abs() != 5) {
                game.set_landed_square(
                    (x, y),
                    Some(SquareContent::with_color(Color::YELLOW_FOREGROUND)),
                );
            }
        }
    }

    // bigger part of corner missing in top left, shouldn't affect anything
    game.set_landed_square((-4, -5), None);
    // also check how this square moves during the clears
    game.set_landed_square(
        (5, -6),
        Some(SquareContent::with_color(Color::YELLOW_FOREGROUND)),
    );

    let before_clears = vec![
        "......~                              ~......",
        "......~                              ~......",
        "......~                              ~......",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "      ~                              ~      ",
        "      ~                        LL    ~      ",
        "      ~        LLLLLLLLLLLLLLLL      ~      ",
        "      ~    LLLLLLLLLLLLLLLLLLLLLL    ~      ",
        "      ~    LLLL..............LLLL    ~      ",
        "      ~    LLLL..............LLLL    ~      ",
        "      ~    LLLL..............LLLL    ~      ",
        "      ~    LLLL..............LLLL    ~      ",
        "      ~    LLLL..............LLLL    ~      ",
        "      ~    LLLL..............LLLL    ~      ",
        "      ~    LLLL..............LLLL    ~      ",
        "      ~    LLLLLLLLLLLLLLLLLLLLLL    ~      ",
        "      ~      LLLLLLLLLLLLLLLLLL      ~      ",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "......~                              ~......",
        "......~                              ~......",
        "......~                              ~......",
    ];
    let between_clears = vec![
        "......~                              ~......",
        "......~                              ~......",
        "......~                              ~......",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "      ~                        LL    ~      ",
        "      ~      LLLLLLLLLLLLLLLLLL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LL..............LL      ~      ",
        "      ~      LLLLLLLLLLLLLLLLLL      ~      ",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "......~                              ~......",
        "......~                              ~......",
        "......~                              ~......",
    ];
    let after_clears = vec![
        "......~                              ~......",
        "......~                              ~......",
        "......~                              ~......",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "      ~                      LL      ~      ",
        "      ~        ..............        ~      ",
        "      ~        ..............        ~      ",
        "      ~        ..............        ~      ",
        "      ~        ..............        ~      ",
        "      ~        ..............        ~      ",
        "      ~        ..............        ~      ",
        "      ~        ..............        ~      ",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "      ~                              ~      ",
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        "......~                              ~......",
        "......~                              ~......",
        "......~                              ~......",
    ];

    assert_eq!(dump_game_state(&game), before_clears);

    let full = game.find_full_rows_and_increment_score();
    game.remove_full_rows(&full);
    assert_eq!(dump_game_state(&game), between_clears);

    let full = game.find_full_rows_and_increment_score();
    game.remove_full_rows(&full);
    assert_eq!(dump_game_state(&game), after_clears);

    let full = game.find_full_rows_and_increment_score();
    assert!(full.is_empty());
    assert_eq!(dump_game_state(&game), after_clears);

    // TODO: you should probably get more score for this than you currently do
    // currently it's 10 per clear, with *2 because two players
    assert_eq!(game.get_score(), 40);
}
