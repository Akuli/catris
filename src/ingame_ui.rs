use crate::client::Client;
use crate::ansi::Color;
use crate::blocks::SquareContent;
use crate::game::Game;
use crate::game::Mode;
use crate::render::RenderBuffer;
use crate::render::RenderData;
use std::cmp::max;

fn render_walls(game: &Game, buffer: &mut RenderBuffer, client_id: u64) {
    match game.mode() {
        Mode::Traditional => {
            for (i, player) in game.players.iter().enumerate() {
                let w = 2 * game.get_width_per_player();
                let left = 1 + (i * w);
                let text = player.borrow().get_name_string(w);
                let color = Color {
                    fg: player.borrow().color,
                    bg: 0,
                };
                let free_space = w - text.chars().count();
                buffer.add_text_with_color(left + (free_space / 2), 0, &text, color);

                let line_character = if player.borrow().client_id == client_id {
                    "="
                } else {
                    "-"
                };
                for x in left..(left + w) {
                    buffer.add_text_with_color(x, 1, line_character, color);
                }
            }

            buffer.set_char(0, 1, 'o');
            buffer.set_char(2 * game.get_width() + 1, 1, 'o');
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
        _ => panic!(),
    }
}

fn render_blocks(game: &Game, buffer: &mut RenderBuffer, client_id: u64) {
    let player_idx = game
        .players
        .iter()
        .position(|cell| cell.borrow().client_id == client_id)
        .unwrap();

    let (offset_x, offset_y) = match game.mode() {
        Mode::Traditional => (1, 2),
        _ => panic!(),
    };

    let trace_points = game.predict_landing_place(player_idx);

    // TODO: optimize lol?
    for x in i8::MIN..i8::MAX {
        for y in i8::MIN..i8::MAX {
            if !game.is_valid_landed_block_coords((x, y)) {
                continue;
            }

            // If flashing, display the flashing
            let mut content = game
                .flashing_points
                .get(&(x, y))
                .map(|color| SquareContent {
                    text: [' ', ' '],
                    color: Color { fg: 0, bg: *color },
                });

            // If not flashing and there's a player's block, show that
            if content.is_none() {
                content = game.get_moving_square((x, y));
            }

            // If still nothing found, use landed squares or leave empty.
            // These are the only ones that can get trace markers "::" on top of them.
            // Traces of drill blocks usually go on top of landed squares.
            if content.is_none() {
                let mut traced_content = game.get_landed_square((x, y)).unwrap_or(SquareContent {
                    text: [' ', ' '],
                    color: Color::DEFAULT,
                });
                if trace_points.contains(&(x, y))
                    && traced_content.text[0] == ' '
                    && traced_content.text[1] == ' '
                {
                    traced_content.text[0] = ':';
                    traced_content.text[1] = ':';
                }
                content = Some(traced_content);
            };

            let content = content.unwrap();
            buffer.set_char_with_color(
                (2 * x + offset_x) as usize,
                (y + offset_y) as usize,
                content.text[0],
                content.color,
            );
            buffer.set_char_with_color(
                (2 * x + offset_x) as usize + 1,
                (y + offset_y) as usize,
                content.text[1],
                content.color,
            );
        }
    }
}

fn get_size_without_stuff_on_side(game: &Game) -> (usize, usize) {
    match game.mode() {
        Mode::Traditional => (game.get_width() * 2 + 2, game.get_height() + 3),
        _ => panic!(),
    }
}

const SCORE_TEXT_COLOR: Color = Color::CYAN_FOREGROUND;

fn render_stuff_on_side(
    _game: &Game,
    buffer: &mut RenderBuffer,
    client: &Client,
    x_offset: usize,
) {
    if client.lobby_id_hidden {
        buffer.add_text(x_offset, 4, "Lobby ID: ******");
    } else {
        let id = &client.lobby.as_ref().unwrap().lock().unwrap().id;
        buffer.add_text(x_offset, 4, &format!("Lobby ID: {}", id));
    }

    // TODO: implement scores properly
    buffer.add_text_with_color(x_offset, 5, "Score: 123", SCORE_TEXT_COLOR);

    if client.prefer_rotating_counter_clockwise {
        buffer.add_text(x_offset, 6, &"Counter-clockwise");
    }

    buffer.add_text(x_offset, 9, "Next:");
    //TODO: show next

    if true {
    buffer.add_text(x_offset, 17, "Nothing in hold");
    buffer.add_text(x_offset, 18, "   (press h)");
    } else {
        buffer.add_text(x_offset, 17, "Holding:");
        //TODO: show hold
    }
}

pub fn render(game: &Game, render_data: &mut RenderData, client: &Client) {
    let (w, h) = get_size_without_stuff_on_side(game);
    let room_for_stuff_on_side_size = 20;
    render_data.clear(max(w + room_for_stuff_on_side_size, 80), max(h, 24));
    render_walls(game, &mut render_data.buffer, client.id);
    render_blocks(game, &mut render_data.buffer, client.id);
    render_stuff_on_side(game, &mut render_data.buffer, client, w+2);
}
