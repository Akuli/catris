use crate::client::ClientLogger;
use crate::escapes::Color;
use crate::escapes::KeyPress;
use crate::escapes::TerminalType;
use crate::game_logic::blocks::BlockType;
use crate::game_logic::blocks::FallingBlock;
use crate::game_logic::blocks::Shape;
use crate::game_logic::blocks::SquareContent;
use crate::game_logic::game::Game;
use crate::game_logic::game::Mode;
use crate::game_logic::game::RING_OUTER_RADIUS;
use crate::game_logic::player::BlockOrTimer;
use crate::game_logic::BlockRelativeCoords;
use crate::game_logic::WorldPoint;
use crate::lobby::ClientInfo;
use crate::RenderBuffer;
use rand::Rng;
use std::collections::HashSet;

fn square_content_to_string(
    content: SquareContent,
    falling_block_data: Option<(BlockRelativeCoords, (i8, i8))>,
) -> String {
    let mut buffer = RenderBuffer::new(TerminalType::ANSI);
    buffer.resize(80, 24); // smallest size allowed
    content.render(&mut buffer, 0, 0, falling_block_data, (0, 1));
    let chars = [buffer.get_char(0, 0), buffer.get_char(1, 0)];
    chars.iter().collect::<String>()
}

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
                row.push('~');
                continue;
            }
            let x = x.unwrap();

            let point = game.players[0].borrow().player_to_world((x, y));
            if !game.is_valid_landed_block_coords(point) {
                row.push_str("..");
            } else if let Some((content, relative_coords, player_idx)) =
                game.get_falling_square((x as i16, y as i16))
            {
                let (down_x, down_y) = game.players[player_idx].borrow().down_direction;
                let text = square_content_to_string(
                    content,
                    Some((relative_coords, (down_x as i8, down_y as i8))),
                );
                if text == "  " {
                    row.push_str("FF");
                } else {
                    row.push_str(&text);
                }
            } else if let Some(content) = game.get_landed_square((x as i16, y as i16)) {
                let text = square_content_to_string(content, None);
                if text == "  " {
                    row.push_str("LL");
                } else {
                    row.push_str(&text);
                }
            } else {
                row.push_str("  ");
            }
        }
        result.push(row);
    }
    result
}

fn create_game(mode: Mode, player_count: usize, shape: Shape) -> Game {
    let mut game = Game::new(mode);
    game.set_block_factory(match shape {
        Shape::L => |_| FallingBlock::normal_from_shape(Shape::L),
        Shape::S => |_| FallingBlock::normal_from_shape(Shape::S),
        _ => unimplemented!(),
    });
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
    let mut game = create_game(Mode::Traditional, 1, Shape::L);
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
    let mut game = create_game(Mode::Traditional, 2, Shape::L);
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
    let mut game = create_game(Mode::Traditional, 2, Shape::L);
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
    assert_eq!(HashSet::from_iter(full.iter().copied()), expected_full);

    assert_eq!(dump_game_state(&game), before_clear);
    game.remove_full_rows(&full);
    assert_eq!(dump_game_state(&game), after_clear);
}

#[test]
fn test_bottle_clearing() {
    let mut game = create_game(Mode::Bottle, 2, Shape::L);
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
        "                  ||                  ",
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
        "                  ||                  ",
        "                  ||                  ",
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
    let mut game = create_game(Mode::Ring, 2, Shape::L);
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
    let mut game = create_game(Mode::Ring, 2, Shape::L);
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

#[test]
fn test_rotating_and_bumping_to_walls() {
    let mut game = create_game(Mode::Traditional, 1, Shape::L);
    game.truncate_height(5);
    game.move_blocks_down(false);
    game.move_blocks_down(false);
    assert_eq!(
        dump_game_state(&game),
        vec![
            "            FF      ",
            "        FFFFFF      ",
            "                    ",
            "                    ",
            "                    "
        ]
    );

    game.handle_key_press(0, false, KeyPress::Up);
    assert_eq!(
        dump_game_state(&game),
        vec![
            "          FF        ",
            "          FF        ",
            "          FFFF      ",
            "                    ",
            "                    "
        ]
    );

    // Move block all the way to left, shouldn't rotate when against wall
    for _ in 0..100 {
        game.handle_key_press(0, false, KeyPress::Left);
    }
    let all_the_way_to_left = vec![
        "FF                  ",
        "FF                  ",
        "FFFF                ",
        "                    ",
        "                    ",
    ];
    assert_eq!(dump_game_state(&game), all_the_way_to_left);
    game.handle_key_press(0, false, KeyPress::Up);
    assert_eq!(dump_game_state(&game), all_the_way_to_left);

    // Move away from wall
    game.handle_key_press(0, false, KeyPress::Right);
    game.handle_key_press(0, false, KeyPress::Up);
    assert_eq!(
        dump_game_state(&game),
        vec![
            "                    ",
            "FFFFFF              ",
            "FF                  ",
            "                    ",
            "                    "
        ]
    );

    for _ in 0..6 {
        game.move_blocks_down(false);
    }
    game.handle_key_press(0, false, KeyPress::Left);
    game.handle_key_press(0, false, KeyPress::Left);
    game.handle_key_press(0, false, KeyPress::Left);
    let landed_block_prevents_rotation = vec![
        "                    ",
        "      FF            ",
        "  FFFFFF            ",
        "LLLLLL              ",
        "LL                  ",
    ];
    assert_eq!(dump_game_state(&game), landed_block_prevents_rotation);
    game.handle_key_press(0, false, KeyPress::Up);
    assert_eq!(dump_game_state(&game), landed_block_prevents_rotation);

    // Move falling block to the right side of landed block, so it can't move left
    game.handle_key_press(0, false, KeyPress::Right);
    game.handle_key_press(0, false, KeyPress::Right);
    game.move_blocks_down(false);
    for _ in 0..10 {
        game.handle_key_press(0, false, KeyPress::Left);
    }
    assert_eq!(
        dump_game_state(&game),
        vec![
            "                    ",
            "                    ",
            "          FF        ",
            "LLLLLLFFFFFF        ",
            "LL                  ",
        ]
    );

    // Should be possible to slide block under another landed block before it lands
    game.move_blocks_down(false);
    for _ in 0..10 {
        game.handle_key_press(0, false, KeyPress::Left);
    }
    assert_eq!(
        dump_game_state(&game),
        vec![
            "                    ",
            "                    ",
            "                    ",
            "LLLLLLFF            ",
            "LLFFFFFF            ",
        ]
    );

    game.move_blocks_down(false);
    assert_eq!(
        dump_game_state(&game),
        vec![
            "                    ",
            "                    ",
            "                    ",
            "LLLLLLLL            ",
            "LLLLLLLL            ",
        ]
    );
}

// Z blocks aren't tested because they are very similar (mirror image)
#[test]
fn test_rotating_s_blocks() {
    let mut game = create_game(Mode::Traditional, 1, Shape::S);
    game.truncate_height(5);

    game.move_blocks_down(false);
    game.move_blocks_down(false);
    game.move_blocks_down(false);

    let state1 = vec![
        "                    ",
        "          FFFF      ",
        "        FFFF        ",
        "                    ",
        "                    ",
    ];
    let state2 = vec![
        "                    ",
        "        FF          ",
        "        FFFF        ",
        "          FF        ",
        "                    ",
    ];
    assert_eq!(dump_game_state(&game), state1);

    // S and Z blocks should go back to their original state after two rotations.
    // The rotations should be the same regardless of whether user prefers clockwise or counter-clockwise.
    for _ in 0..10 {
        game.handle_key_press(0, rand::thread_rng().gen::<bool>(), KeyPress::Up);
        assert_eq!(dump_game_state(&game), state2);
        game.handle_key_press(0, rand::thread_rng().gen::<bool>(), KeyPress::Up);
        assert_eq!(dump_game_state(&game), state1);
    }
}

fn create_ring_game_with_drills() -> Game {
    let mut game = Game::new(Mode::Ring);
    game.set_block_factory(|_| FallingBlock::new(BlockType::Drill));
    for i in 0..3 {
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
fn test_ring_game_directions() {
    let game = create_ring_game_with_drills();

    // Players 0 and 1 are in opposite directions. Player 2 is perpendicular to both.
    assert!(game.players[0].borrow().down_direction == (0, 1));
    assert!(game.players[1].borrow().down_direction == (0, -1));
    assert!(game.players[2].borrow().down_direction == (1, 0));
}

#[test]
fn test_displaying_and_animating_falling_drills() {
    let mut game = create_ring_game_with_drills();
    game.move_blocks_down(false);
    game.move_blocks_down(false);
    game.move_blocks_down(false);

    let expected_dump = vec![
        r"......~            | .|              ~......",
        r"......~            |. |              ~......",
        r"......~             \/               ~......",
        r"~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        r"      ~                              ~      ",
        r"      ~                              ~      ",
        r"      ~                              ~      ",
        r"      ~                              ~      ",
        r"      ~        ..............        ~      ",
        r"      ~        ..............        ~      ",
        r"      ~        ..............        ~      ",
        r"----. ~        ..............        ~      ",
        r"/__/.'~        ..............        ~      ",
        r"      ~        ..............        ~      ",
        r"      ~        ..............        ~      ",
        r"      ~                              ~      ",
        r"      ~                              ~      ",
        r"      ~                              ~      ",
        r"      ~                              ~      ",
        r"~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        r"......~               /\             ~......",
        r"......~              |. |            ~......",
        r"......~              | /|            ~......",
    ];
    assert_eq!(dump_game_state(&game), expected_dump);

    // Top and bottom drills should rotate with 4 pictures to choose from.
    // Side drills have 3 pictures instead.
    let mut top_matches = "".to_string();
    let mut middle_matches = "".to_string();
    let mut bottom_matches = "".to_string();
    for _ in 0..20 {
        let actual_dump = dump_game_state(&game);

        if actual_dump[..5] == expected_dump[..5] {
            top_matches.push('m');
        } else {
            top_matches.push('-');
        }

        if actual_dump[5..15] == expected_dump[5..15] {
            middle_matches.push('m');
        } else {
            middle_matches.push('-');
        }

        if actual_dump[15..] == expected_dump[15..] {
            bottom_matches.push('m');
        } else {
            bottom_matches.push('-');
        }

        assert!(game.animate_drills());
    }

    assert_eq!(top_matches, "m---m---m---m---m---");
    assert_eq!(middle_matches, "m--m--m--m--m--m--m-");
    assert_eq!(bottom_matches, "m---m---m---m---m---");
}

#[test]
fn test_displaying_landed_drills() {
    let mut game = Game::new(Mode::Ring);
    game.set_block_factory(|_| FallingBlock::new(BlockType::Drill));
    for i in 0..3 {
        game.add_player(&ClientInfo {
            name: format!("Player {}", i),
            client_id: i as u64,
            color: Color::RED_FOREGROUND.fg,
            logger: ClientLogger {
                client_id: i as u64,
            },
        });
    }

    // Make sure that drills show up correctly once landed
    let has_landed_squares = |game: &Game| {
        let r = RING_OUTER_RADIUS as i16;
        (-r..=r).any(|x| (-r..=r).any(|y| game.get_landed_square((x, y)).is_some()))
    };

    let mut dump_before_land = vec![];
    while !has_landed_squares(&game) {
        dump_before_land = dump_game_state(&game);
        game.move_blocks_down(false);
    }

    // Landing shouldn't change how the blocks look.
    // Achieving this in the code is more complicated than you would expect...
    assert_eq!(dump_game_state(&game), dump_before_land);
    assert_eq!(
        dump_before_land,
        vec![
            r"......~                              ~......",
            r"......~                              ~......",
            r"......~                              ~......",
            r"~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
            r"      ~            |/ |              ~      ",
            r"      ~            | .|              ~      ",
            r"      ~            |. |              ~      ",
            r"      ~             \/               ~      ",
            r"      ~        ..............        ~      ",
            r"      ~        ..............        ~      ",
            r"      ~        ..............        ~      ",
            r"      ~------. ..............        ~      ",
            r"      ~__/__/.'..............        ~      ",
            r"      ~        ..............        ~      ",
            r"      ~        ..............        ~      ",
            r"      ~               /\             ~      ",
            r"      ~              |. |            ~      ",
            r"      ~              | /|            ~      ",
            r"      ~              |/ |            ~      ",
            r"~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
            r"......~                              ~......",
            r"......~                              ~......",
            r"......~                              ~......"
        ]
    );

    // Animating shouldn't do anything to landed drills
    game.animate_drills();
    assert_eq!(dump_game_state(&game), dump_before_land);
}
