use crate::ansi::Color;
use crate::blocks::MovingBlock;
use crate::client::Client;
use crate::game_logic::Game;
use crate::game_logic::Mode;
use crate::game_logic::BOTTLE_MAP;
use crate::game_logic::RING_MAP;
use crate::game_logic::RING_OUTER_RADIUS;
use crate::player::BlockOrTimer;
use crate::player::Player;
use crate::render::RenderBuffer;
use crate::render::RenderData;
use std::cell::RefCell;
use std::cmp::max;

fn render_name_lines(
    players: &[RefCell<Player>],
    highlight_client_id: u64,
    buffer: &mut RenderBuffer,
    x_offset: usize,
    width_per_player: usize,
    name_y: usize,
    line_y: usize,
    o_ends: bool,
) {
    for (i, player) in players.iter().enumerate() {
        let left = x_offset + (i * width_per_player);
        let right = left + width_per_player;
        let text = player.borrow().get_name_string(width_per_player);
        let color = Color {
            fg: player.borrow().color,
            bg: 0,
        };
        let free_space = width_per_player - text.chars().count();
        buffer.add_text_with_color(left + (free_space / 2), name_y, &text, color);

        let line_character = if player.borrow().client_id == highlight_client_id {
            "="
        } else {
            "-"
        };

        if o_ends {
            buffer.add_text_with_color(left, line_y, "o", color);
            buffer.add_text_with_color(right - 1, line_y, "o", color);
            for x in (left + 1)..(right - 1) {
                buffer.add_text_with_color(x, line_y, line_character, color);
            }
        } else {
            for x in left..right {
                buffer.add_text_with_color(x, line_y, line_character, color);
            }
        }
    }
}

fn wrap_text(s: &str, line_maxlen: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    let mut last_line_len = line_maxlen;

    for word in s.split_whitespace() {
        let word_len = word.chars().count();

        if last_line_len + 1 + word_len <= line_maxlen {
            // it fits on the last line
            lines.last_mut().unwrap().push(' ');
            lines.last_mut().unwrap().push_str(word);
            last_line_len += 1 + word_len;
        } else if word_len <= line_maxlen {
            // it fits on a line of its own
            lines.push(word.to_string());
            last_line_len = word_len;
        } else {
            // doesn't fit nicely, just add each character...
            if last_line_len == line_maxlen {
                // ...to a new line, because previous line is full...
                lines.push("".to_string());
                last_line_len = 0;
            } else {
                // ...or at the end of the previous line
                lines.last_mut().unwrap().push(' ');
                last_line_len += 1;
            }
            for ch in word.chars() {
                if last_line_len == line_maxlen {
                    lines.push("".to_string());
                    last_line_len = 0;
                }
                lines.last_mut().unwrap().push(ch);
                last_line_len += 1;
            }
        }
    }
    lines
}

fn wrap_text_ignoring_whitespace(s: &str, line_maxlen: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    let mut last_line_len = line_maxlen;

    for ch in s.chars() {
        if last_line_len == line_maxlen {
            lines.push("".to_string());
            last_line_len = 0;
        }
        lines.last_mut().unwrap().push(ch);
        last_line_len += 1;
    }
    lines
}

fn get_wrapped_name(player: &Player, letter: char) -> String {
    let counts = RING_MAP
        .iter()
        .map(|row| row.matches(letter).count())
        .filter(|n| *n != 0)
        .collect::<Vec<usize>>();
    let width = counts[0];
    let height = counts.len();
    for c in counts {
        assert!(c == width);
    }

    let unwrapped_name = player.get_name_string(width * height);
    let mut wrapped = wrap_text(&unwrapped_name, width);
    if wrapped.len() > height {
        wrapped = wrap_text_ignoring_whitespace(&unwrapped_name, width);
    }
    assert!(wrapped.len() <= height);

    let mut result = "".to_string();

    let unused_height = height - wrapped.len();
    for _ in 0..(unused_height / 2) {
        for _ in 0..width {
            result.push(' ');
        }
    }

    for row in wrapped {
        result.push_str(&match letter {
            'w' | 's' => format!("{:^width$}", row),
            'a' => format!("{:<width$}", row),
            'd' => format!("{:>width$}", row),
            _ => panic!(),
        });
    }
    result
}

fn prepare_player_for_ring_game_rendering(
    players: &[RefCell<Player>],
    this_player_client_id: u64,
    letter: char,
) -> (String, Color) {
    let (down_x, down_y) = players
        .iter()
        .map(|p| p.borrow())
        .find(|p| p.client_id == this_player_client_id)
        .unwrap()
        .down_direction;

    let other_down_dir = match letter {
        'w' => (down_x, down_y),
        'a' => (down_y, -down_x),
        's' => (-down_x, -down_y),
        'd' => (-down_y, down_x),
        _ => panic!(),
    };

    players
        .iter()
        .map(|p| p.borrow())
        .find(|p| p.down_direction == other_down_dir)
        .map(|p| (get_wrapped_name(&*p, letter), Color { fg: p.color, bg: 0 }))
        .unwrap_or_else(|| ("".to_string(), Color::DEFAULT))
}

fn render_walls(game: &Game, buffer: &mut RenderBuffer, client_id: u64) {
    match game.mode {
        Mode::Traditional => {
            buffer.set_char(0, 1, 'o');
            buffer.set_char(2 * game.get_width() + 1, 1, 'o');
            render_name_lines(
                &game.players,
                client_id,
                buffer,
                1,
                2 * game.get_width_per_player().unwrap(),
                0,
                1,
                false,
            );

            for y in 2..(2 + game.get_height()) {
                buffer.set_char(0, y, '|');
                buffer.set_char(2 * game.get_width() + 1, y, '|');
            }

            let bottom_y = 2 + game.get_height();
            buffer.set_char(0, bottom_y, 'o');
            buffer.set_char(2 * game.get_width() + 1, bottom_y, 'o');
            for x in 1..(2 * game.get_width() + 1) {
                buffer.set_char(x, bottom_y, '-');
            }
        }
        Mode::Bottle => {
            for (player_idx, player) in game.players.iter().enumerate() {
                let left = player_idx * BOTTLE_MAP[0].len();
                let color = Color {
                    fg: player.borrow().color,
                    bg: 0,
                };
                for (y, line) in BOTTLE_MAP.iter().enumerate() {
                    let is_in_personal_space = !line.starts_with('|');
                    for (i, ch) in line.chars().enumerate() {
                        let is_at_edge = (player_idx == 0 && i == 0)
                            || (player_idx == game.players.len() - 1 && i == line.len() - 1);
                        if ch != 'x'
                            && ch != ' '
                            && (ch != '|' || is_in_personal_space || is_at_edge)
                        {
                            buffer.set_char_with_color(left + i, y, ch, color);
                        }
                    }
                }
            }
            render_name_lines(
                &game.players,
                client_id,
                buffer,
                0,
                BOTTLE_MAP[0].len(),
                BOTTLE_MAP.len() + 1,
                BOTTLE_MAP.len(),
                true,
            );
        }
        Mode::Ring => {
            let (w_text, w_color) =
                prepare_player_for_ring_game_rendering(&game.players, client_id, 'w');
            let (a_text, a_color) =
                prepare_player_for_ring_game_rendering(&game.players, client_id, 'a');
            let (s_text, s_color) =
                prepare_player_for_ring_game_rendering(&game.players, client_id, 's');
            let (d_text, d_color) =
                prepare_player_for_ring_game_rendering(&game.players, client_id, 'd');
            let mut w_chars = w_text.chars();
            let mut a_chars = a_text.chars();
            let mut s_chars = s_text.chars();
            let mut d_chars = d_text.chars();

            for (y, line) in RING_MAP.iter().enumerate() {
                for (x, spec_char) in line.chars().enumerate() {
                    let ch = match spec_char {
                        'w' => w_chars.next().unwrap_or(' '),
                        'a' => a_chars.next().unwrap_or(' '),
                        's' => s_chars.next().unwrap_or(' '),
                        'd' => d_chars.next().unwrap_or(' '),
                        'x' | ' ' => continue,
                        other => other,
                    };
                    let color = match spec_char {
                        'w' => w_color,
                        'a' => a_color,
                        's' => s_color,
                        'd' => d_color,
                        '|' if (1..(line.len() / 2)).contains(&x) => a_color,
                        '|' if ((line.len() / 2)..(line.len() - 1)).contains(&x) => d_color,
                        '=' => w_color,
                        '-' if y != 0 && y != RING_MAP.len() - 1 => s_color,
                        _ => Color::DEFAULT,
                    };
                    buffer.set_char_with_color(x, y, ch, color);
                }
            }
        }
    }
}

fn render_blocks(game: &Game, buffer: &mut RenderBuffer, client_id: u64) {
    let player_idx = game
        .players
        .iter()
        .position(|cell| cell.borrow().client_id == client_id)
        .unwrap();

    let (offset_x, offset_y) = match game.mode {
        Mode::Traditional => (1, 2),
        Mode::Bottle => (1, 0),
        Mode::Ring => {
            let r = RING_OUTER_RADIUS as i32;
            (1 + 2 * r, 1 + r)
        }
    };

    let mut trace_points = game.predict_landing_place(player_idx);

    // Don't trace on top of current player's moving block or flashing
    {
        let player = game.players[player_idx].borrow();
        match &player.block_or_timer {
            BlockOrTimer::Block(block) => {
                for point in block.get_coords() {
                    trace_points.retain(|p| *p != player.player_to_world(point));
                }
            }
            _ => {}
        }
    }
    trace_points.retain(|p| !game.flashing_points.contains_key(p));

    let (viewer_dir_x, viewer_dir_y) = game.players[player_idx].borrow().down_direction;

    let (x_start, x_end, y_start, y_end) = game.get_bounds_in_player_coords();
    for x in x_start..x_end {
        for y in y_start..y_end {
            let world_point = game.players[player_idx].borrow().player_to_world((x, y));
            if !game.is_valid_landed_block_coords(world_point) {
                continue;
            }

            let buffer_x = (offset_x + 2 * x) as usize;
            let buffer_y = (offset_y + y) as usize;

            if let Some(flash_bg) = game.flashing_points.get(&world_point) {
                buffer.add_text_with_color(
                    buffer_x,
                    buffer_y,
                    "  ",
                    Color {
                        fg: 0,
                        bg: *flash_bg,
                    },
                );
            } else if let Some((content, relative_coords, owner_idx)) =
                game.get_moving_square(world_point, None)
            {
                let (moving_x, moving_y) = game.players[owner_idx].borrow().down_direction;
                content.render(
                    buffer,
                    buffer_x,
                    buffer_y,
                    Some((relative_coords, (moving_x as i8, moving_y as i8))),
                    (viewer_dir_x as i8, viewer_dir_y as i8),
                );
            } else if let Some(content) = game.get_landed_square(world_point) {
                content.render(
                    buffer,
                    buffer_x,
                    buffer_y,
                    None,
                    (viewer_dir_x as i8, viewer_dir_y as i8),
                );
            }

            if trace_points.contains(&world_point)
                && buffer.get_char(buffer_x, buffer_y) == ' '
                && buffer.get_char(buffer_x + 1, buffer_y) == ' '
            {
                buffer.add_text_without_changing_color(buffer_x, buffer_y, "::");
            }
        }
    }
}

fn get_size_without_stuff_on_side(game: &Game) -> (usize, usize) {
    let (extra_w, extra_h) = match game.mode {
        Mode::Traditional => (2, 3), // 3 = player names, dashes below them, dashes at bottom
        Mode::Bottle | Mode::Ring => (2, 2),
    };
    (game.get_width() * 2 + extra_w, game.get_height() + extra_h)
}

pub const SCORE_TEXT_COLOR: Color = Color::CYAN_FOREGROUND;

fn render_block(
    block: &MovingBlock,
    buffer: &mut RenderBuffer,
    text_x: usize,
    text_y: usize,
    text: &str,
) {
    /*
    text goes here

      xxxxxxxxxx
      xxxxxxxxxx
      xxxx()xxxx    <-- () is the center
      xxxxxxxxxx
      xxxxxxxxxx
    */
    buffer.add_text(text_x, text_y, text);
    let center_x = (text_x as isize) + 6;
    let center_y = (text_y as isize) + 4;

    for (x, y) in block.get_relative_coords() {
        block.square_content.render(
            buffer,
            (center_x + 2 * (*x as isize)) as usize,
            (center_y + (*y as isize)) as usize,
            Some(((*x, *y), (0, 1))),
            (0, 1),
        );
    }
}

fn render_stuff_on_side(
    game: &Game,
    buffer: &mut RenderBuffer,
    client: &Client,
    lobby_id: &str,
    x_offset: usize,
) {
    if client.lobby_id_hidden {
        buffer.add_text(x_offset, 4, "Lobby ID: ******");
    } else {
        buffer.add_text(x_offset, 4, &format!("Lobby ID: {}", lobby_id));
    }

    buffer.add_text_with_color(
        x_offset,
        5,
        &format!("Score: {}", game.get_score()),
        SCORE_TEXT_COLOR,
    );

    if client.prefer_rotating_counter_clockwise {
        buffer.add_text(x_offset, 6, &"Counter-clockwise");
    }

    let player = game
        .players
        .iter()
        .find(|p| p.borrow().client_id == client.id)
        .unwrap()
        .borrow();
    render_block(&player.next_block, buffer, x_offset, 8, "Next:");

    if let Some(block) = &player.block_in_hold {
        render_block(block, buffer, x_offset, 16, "Holding:");
    } else {
        buffer.add_text(x_offset, 16, "Nothing in hold");
        buffer.add_text(x_offset, 17, "   (press h)");
    }
}

pub fn render(game: &Game, render_data: &mut RenderData, client: &Client, lobby_id: &str) {
    let (w, h) = get_size_without_stuff_on_side(game);
    let room_for_stuff_on_side_size = 20;
    render_data.clear(max(w + room_for_stuff_on_side_size, 80), max(h, 24));
    render_walls(game, &mut render_data.buffer, client.id);
    render_blocks(game, &mut render_data.buffer, client.id);
    render_stuff_on_side(game, &mut render_data.buffer, client, lobby_id, w + 2);
}
