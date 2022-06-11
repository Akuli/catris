use crate::ansi::Color;
use crate::blocks::MovingBlock;
use crate::client::Client;
use crate::game_logic::Game;
use crate::game_logic::Mode;
use crate::game_logic::BOTTLE_MAP;
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
        _ => panic!(),
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
        _ => panic!(),
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

    // TODO: optimize lol?
    for x in -100..100 {
        for y in -100..100 {
            if !game.is_valid_landed_block_coords((x, y)) {
                continue;
            }

            let buffer_x = (offset_x + 2 * x) as usize;
            let buffer_y = (offset_y + y) as usize;

            if let Some(flash_bg) = game.flashing_points.get(&(x, y)) {
                buffer.add_text_with_color(
                    buffer_x,
                    buffer_y,
                    "  ",
                    Color {
                        fg: 0,
                        bg: *flash_bg,
                    },
                );
            } else if let Some((content, relative_coords)) = game.get_moving_square((x, y), None) {
                content.render(buffer, buffer_x, buffer_y, Some(relative_coords));
            } else if let Some(content) = game.get_landed_square((x, y)) {
                content.render(buffer, buffer_x, buffer_y, None);
            }

            if trace_points.contains(&(x, y))
                && buffer.get_char(buffer_x, buffer_y) == ' '
                && buffer.get_char(buffer_x + 1, buffer_y) == ' '
            {
                buffer.add_text_without_changing_color(buffer_x, buffer_y, "::");
            }
        }
    }
}

fn get_size_without_stuff_on_side(game: &Game) -> (usize, usize) {
    match game.mode {
        Mode::Traditional => (game.get_width() * 2 + 2, game.get_height() + 3),
        Mode::Bottle => (game.get_width() * 2 + 2, game.get_height() + 2),
        _ => panic!(),
    }
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
            Some((*x, *y)),
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
