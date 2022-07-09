use crate::ansi::Color;
use crate::ansi::KeyPress;
use crate::client::Client;
use crate::game_logic::game::Mode;
use crate::game_wrapper::GameStatus;
use crate::high_scores::GameResult;
use crate::ingame_ui;
use crate::lobby::join_game_in_a_lobby;
use crate::lobby::looks_like_lobby_id;
use crate::lobby::Lobbies;
use crate::lobby::Lobby;
use crate::lobby::MAX_CLIENTS_PER_LOBBY;
use crate::render;
use crate::render::RenderBuffer;
use chrono::Utc;
use std::collections::HashSet;
use std::io;
use std::io::ErrorKind;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use tokio::fs::OpenOptions;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::sync::watch;

const ASCII_ART: &[&str] = &[
    "",
    r"  __     ___    _____   _____   _____   ___ ",
    r" / _)   / _ \  |_   _| |  __ \ |_   _| / __)",
    r"| (_   / /_\ \   | |   |  _  /  _| |_  \__ \",
    r" \__) /_/   \_\  |_|   |_| \_\ |_____| (___/",
    "",
    "Play online: https://akuli.github.io/catris",
    "Code: https://github.com/Akuli/catris",
    "",
];

fn add_ascii_art(buffer: &mut RenderBuffer) {
    for (y, line) in ASCII_ART.iter().enumerate() {
        buffer.add_centered_text(y, line);
    }
}

async fn prompt<F>(
    client: &mut Client,
    prompt: &str,
    mut enter_pressed_callback: F,
    add_extra_text: Option<fn(&mut RenderBuffer)>,
    min_duration_between_enter_presses: Duration,
) -> Result<(), io::Error>
where
    F: FnMut(&str, &mut Client) -> Option<String>,
{
    let mut error = Some("".to_string());
    let mut current_text = "".to_string();
    let mut last_enter_press: Option<Instant> = None;

    loop {
        {
            let mut render_data = client.render_data.lock().unwrap();
            render_data.clear(80, 24);

            add_ascii_art(&mut render_data.buffer);
            let mut x = render_data.buffer.add_text(20, 10, prompt);
            x = render_data.buffer.add_text(x, 10, &current_text);
            render_data.cursor_pos = Some((x, 10));
            render_data.buffer.add_text_with_color(
                2,
                13,
                &error.clone().unwrap_or_default(),
                Color::RED_FOREGROUND,
            );
            if let Some(f) = add_extra_text {
                f(&mut render_data.buffer);
            }

            render_data.changed.notify_one();
        }

        match client.receive_key_press().await? {
            /*
            \r\n: Enter press in windows cmd.exe
            \r:   Enter press in other os with raw mode
            \n:   Enter press in other os without raw mode (bad)

            \r is also known as KeyPress::Enter. If we haven't gotten that
            yet, and we get \n, it means someone forgot to set raw mode.
            */
            KeyPress::Character('\n') if last_enter_press == None => {
                error = Some(
                    "Your terminal doesn't seem to be in raw mode. Run 'stty raw' and try again."
                        .to_string(),
                );
            }
            KeyPress::Character(ch) => {
                // 15 chars is enough for names and lobby IDs
                // It's important to have limit (potential out of mem dos attack otherwise)
                if current_text.chars().count() < 15 {
                    current_text.push(ch);
                }
            }
            KeyPress::BackSpace => {
                current_text.pop();
            }
            KeyPress::Enter => {
                if last_enter_press == None
                    || last_enter_press.unwrap().elapsed() > min_duration_between_enter_presses
                {
                    last_enter_press = Some(Instant::now());
                    error = enter_pressed_callback(current_text.trim(), client);
                    if error == None {
                        return Ok(());
                    }
                }
            }
            _ => {}
        }
    }
}

// I started with all 256 latin-1 chars and removed some of them.
// It's important to ban characters that are more than 1 unit wide on terminal.
const VALID_NAME_CHARS: &str = concat!(
    " !\"#$%&'()*+-./:;<=>?@\\^_`{|}~¡¢£¤¥¦§¨©ª«¬®¯°±²³´µ¶·¸¹º»¼½¾¿×÷",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
    "abcdefghijklmnopqrstuvwxyz",
    "0123456789",
    "ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÐÑÒÓÔÕÖØÙÚÛÜÝÞßàáâãäåæçèéêëìíîïðñòóôõöøùúûüýþÿ",
);

fn add_name_asking_notes(buffer: &mut RenderBuffer) {
    buffer.add_centered_text(17, "If you play well, your name will be");
    buffer.add_centered_text(18, "visible to everyone in the high scores.");

    buffer.add_centered_text(20, "Your IP will be logged on the server only if you");
    buffer.add_centered_text(21, "connect 5 or more times within the same minute.");
}

pub async fn ask_name(
    client: &mut Client,
    used_names: Arc<Mutex<HashSet<String>>>,
) -> Result<(), io::Error> {
    prompt(
        client,
        "Name: ",
        |name, client| {
            if name.is_empty() {
                return Some("Please write a name before pressing Enter.".to_string());
            }
            for ch in name.chars() {
                if !VALID_NAME_CHARS.contains(ch) {
                    return Some(format!("The name can't contain a '{}' character.", ch));
                }
            }
            if !client.set_name(name, used_names.clone()) {
                return Some("This name is in use. Try a different name.".to_string());
            }
            None
        },
        Some(add_name_asking_notes),
        Duration::ZERO,
    )
    .await?;
    Ok(())
}

pub async fn ask_lobby_id_and_join_lobby(
    client: &mut Client,
    lobbies: Lobbies,
) -> Result<(), io::Error> {
    prompt(
        client,
        "Lobby ID (6 characters): ",
        |id, client| {
            let id = id.to_uppercase();
            if !looks_like_lobby_id(&id) {
                return Some("The text you entered doesn't look like a lobby ID.".to_string());
            }

            let lobbies = lobbies.lock().unwrap();
            return if let Some(lobby) = lobbies.get(&id) {
                if client.join_lobby(lobby) {
                    None
                } else {
                    Some(format!(
                        "Lobby '{}' is full. It already has {} players.",
                        id, MAX_CLIENTS_PER_LOBBY
                    ))
                }
            } else {
                Some(format!("There is no lobby with ID '{}'.", id))
            };
        },
        None,
        // prevent brute-force-guessing lobby IDs, max 1 attempt per second
        Duration::from_secs(1),
    )
    .await?;
    Ok(())
}

struct Menu {
    items: Vec<Option<String>>, // None is a separator
    selected_index: usize,
}

impl Menu {
    fn selected_text(&self) -> &str {
        self.items[self.selected_index].as_ref().unwrap()
    }

    fn render(&self, buffer: &mut RenderBuffer, top_y: usize) {
        for (i, item) in self.items.iter().enumerate() {
            if let Some(text) = item {
                let centered_text = format!("{:^35}", text);
                if i == self.selected_index {
                    buffer.add_centered_text_with_color(
                        top_y + i,
                        &centered_text,
                        Color::BLACK_ON_WHITE,
                    );
                } else {
                    buffer.add_centered_text(top_y + i, &centered_text);
                }
            }
        }
    }

    // true means enter pressed
    fn handle_key_press(&mut self, key: KeyPress) -> bool {
        let last = self.items.len() - 1;
        match key {
            KeyPress::Up if self.selected_index != 0 => {
                self.selected_index -= 1;
                while self.items[self.selected_index].is_none() {
                    self.selected_index -= 1;
                }
            }
            KeyPress::Down if self.selected_index != last => {
                self.selected_index += 1;
                while self.items[self.selected_index].is_none() {
                    self.selected_index += 1;
                }
            }
            KeyPress::Character(ch) => {
                // pressing r selects Ring Game
                for (i, item) in self.items.iter().enumerate() {
                    if item
                        .as_ref()
                        .unwrap_or(&"".to_string())
                        .to_lowercase()
                        .starts_with(&ch.to_lowercase().to_string())
                    {
                        self.selected_index = i;
                        break;
                    }
                }
            }
            KeyPress::Enter => {
                return true;
            }
            _ => {}
        }
        false
    }
}

async fn read_motd() -> Result<Vec<String>, io::Error> {
    let file = OpenOptions::new()
        .read(true)
        .open("catris_motd.txt")
        .await?;
    let buf_reader = BufReader::new(file);
    let mut lines = buf_reader.lines();
    let mut result = vec![];
    while let Some(line) = lines.next_line().await? {
        result.push(line);
    }
    Ok(result)
}

pub async fn ask_if_new_lobby(client: &mut Client) -> Result<bool, io::Error> {
    let motd = match read_motd().await {
        Ok(lines) => lines,
        Err(e) if e.kind() == ErrorKind::NotFound => vec![],
        Err(e) => {
            client
                .logger()
                .log(&format!("reading motd file failed: {:?}", e));
            vec![]
        }
    };
    let mut menu = Menu {
        items: vec![
            Some("New lobby".to_string()),
            Some("Join an existing lobby".to_string()),
            Some("Quit".to_string()),
        ],
        selected_index: 0,
    };

    loop {
        {
            let mut render_data = client.render_data.lock().unwrap();
            render_data.clear(80, 24);

            add_ascii_art(&mut render_data.buffer);
            menu.render(&mut render_data.buffer, 10);
            render_data
                .buffer
                .add_centered_text(16, "If you want to play alone, just make a new lobby.");
            render_data.buffer.add_centered_text(
                17,
                "For multiplayer, one player makes a lobby and others join it.",
            );
            for (i, line) in motd.iter().enumerate() {
                render_data.buffer.add_centered_text_with_color(
                    19 + i,
                    line,
                    Color::GREEN_FOREGROUND,
                );
            }

            render_data.changed.notify_one();
        }

        let key = client.receive_key_press().await?;
        if menu.handle_key_press(key) {
            return match menu.selected_text() {
                "New lobby" => Ok(true),
                "Join an existing lobby" => Ok(false),
                "Quit" => Err(io::Error::new(
                    ErrorKind::ConnectionAborted,
                    "user selected \"Quit\" in menu",
                )),
                _ => panic!(),
            };
        }
    }
}

fn render_lobby_status(client: &Client, render_data: &mut render::RenderData, lobby: &Lobby) {
    let mut x = 3;
    x = render_data.buffer.add_text(x, 2, "Lobby ID: ");
    if client.lobby_id_hidden {
        x = render_data.buffer.add_text(x, 2, "******");
        x = render_data.buffer.add_text_with_color(
            x,
            2,
            " (press i to show)",
            Color::GRAY_FOREGROUND,
        );
    } else {
        x = render_data.buffer.add_text(x, 2, &lobby.id);
        x = render_data.buffer.add_text_with_color(
            x,
            2,
            " (press i to hide)",
            Color::GRAY_FOREGROUND,
        );
    }

    for (i, info) in lobby.clients.iter().enumerate() {
        let y = 5 + i;

        x = 6;
        x = render_data.buffer.add_text(x, y, &format!("{}. ", i + 1));
        x = render_data.buffer.add_text_with_color(
            x,
            y,
            &info.name,
            Color {
                fg: info.color,
                bg: 0,
            },
        );
        if info.client_id == client.id {
            render_data
                .buffer
                .add_text_with_color(x, y, " (you)", Color::GRAY_FOREGROUND);
        }
    }

    _ = x; // silence compiler warning
}

// None return value means show gameplay tips
pub async fn choose_game_mode(
    client: &mut Client,
    selected_index: &mut usize,
) -> Result<Option<Mode>, io::Error> {
    let mut items = vec![];
    items.resize(Mode::ALL_MODES.len(), None);
    items.push(None);
    items.push(Some("Gameplay tips".to_string()));
    items.push(Some("Quit".to_string()));
    let mut menu = Menu {
        items,
        selected_index: *selected_index,
    };

    let mut changed_receiver = client
        .lobby
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .changed_receiver
        .clone();

    loop {
        {
            let mut render_data = client.render_data.lock().unwrap();
            render_data.clear(80, 24);

            let mut selected_game_is_full = false;
            {
                let idk_why_i_need_this = client.lobby.clone().unwrap();
                let lobby = idk_why_i_need_this.lock().unwrap();
                render_lobby_status(client, &mut *render_data, &lobby);

                for (i, mode) in Mode::ALL_MODES.iter().enumerate() {
                    let count = lobby.get_player_count(*mode);
                    let max = mode.max_players();
                    menu.items[i] = Some(format!("{} ({}/{} players)", mode.name(), count, max));
                    if i == menu.selected_index && count == max {
                        selected_game_is_full = true;
                    }
                }
            }

            menu.render(&mut render_data.buffer, 13);
            if selected_game_is_full {
                render_data.buffer.add_centered_text_with_color(
                    21,
                    "This game is full.",
                    Color::RED_FOREGROUND,
                );
            }
            render_data.changed.notify_one();
        }

        tokio::select! {
            key_or_error = client.receive_key_press() => {
                match key_or_error? {
                    KeyPress::Character('I') | KeyPress::Character('i') => {
                        client.lobby_id_hidden = !client.lobby_id_hidden;
                    }
                    key => {
                        if menu.handle_key_press(key) {
                            *selected_index = menu.selected_index;
                            return match menu.selected_text() {
                                "Gameplay tips" => Ok(None),
                                "Quit" => Err(io::Error::new(
                                    ErrorKind::ConnectionAborted,
                                    "user selected \"Quit\" in menu",
                                )),
                                _ => Ok(Some(Mode::ALL_MODES[menu.selected_index])),
                            };
                        }
                    }
                }
            }
            res = changed_receiver.changed() => {
                // It errors if the sender no longer exists.
                // But the sender is in the lobby which exists as long as there are clients.
                // So this should never fail.
                res.unwrap();
            }
        }
    }
}

const GAMEPLAY_TIPS: &[&str] = &[
    "Keys:",
    "  [Ctrl+C], [Ctrl+D] or [Ctrl+Q]: quit",
    "  [Ctrl+R]: redraw the whole screen (may be needed after resizing the window)",
    "  [W]/[A]/[S]/[D] or [↑]/[←]/[↓]/[→]: move and rotate (don't hold down [S] or [↓])",
    "  [H]: hold (aka save) block for later, switch to previously held block if any",
    "  [R]: change rotating direction",
    "  [P]: pause/unpause (affects all players)",
    "  [F]: flip the game upside down (only available in ring mode with 1 player)",
    "",
    "There's only one score. {You play together}, not against other players. Try to",
    "work together and make good use of everyone's blocks.",
    "",
    "With multiple players, when your playing area fills all the way to the top,",
    "you need to wait 30 seconds before you can continue playing. The game ends",
    "when all players are simultaneously on their 30 seconds waiting time. This",
    "means that if other players are doing well, you can {intentionally fill your",
    "playing area} to do your waiting time before others mess up.",
];

pub async fn show_gameplay_tips(client: &mut Client) -> Result<(), io::Error> {
    let mut menu = Menu {
        items: vec![Some("Back to menu".to_string())],
        selected_index: 0,
    };

    {
        let mut render_data = client.render_data.lock().unwrap();
        render_data.clear(80, 24);

        let mut color = Color::DEFAULT;
        let mut y = 0;

        for line_ref in GAMEPLAY_TIPS {
            let mut line = *line_ref;
            if line.contains("Ctrl+") && client.is_connected_with_websocket() {
                continue;
            }

            let mut x = 2;
            y += 1;

            loop {
                match line.chars().next() {
                    Some('[') => {
                        color = Color::MAGENTA_FOREGROUND;
                        line = &line[1..];
                    }
                    Some('{') => {
                        color = Color::CYAN_FOREGROUND;
                        line = &line[1..];
                    }
                    Some(']') | Some('}') => {
                        color = Color::DEFAULT;
                        line = &line[1..];
                    }
                    Some(_) => {
                        let i = line.find(|c| "[]{}".contains(c)).unwrap_or(line.len());
                        x = render_data
                            .buffer
                            .add_text_with_color(x, y, &line[..i], color);
                        line = &line[i..];
                    }
                    None => break,
                }
            }
        }

        menu.render(&mut render_data.buffer, 19);
        render_data.changed.notify_one();
    }

    while !menu.handle_key_press(client.receive_key_press().await?) {
        // Clear the key that user typed, although no need to re-render
        client.render_data.lock().unwrap().changed.notify_one();
    }
    Ok(())
}

const PAUSE_SCREEN: &[&str] = &[
    "o============================================================o",
    "|                                                            |",
    "|                                                            |",
    "|                        Game paused                         |",
    "|                       ^^^^^^^^^^^^^                        |",
    "|                                                            |",
    "|                                                            |",
    "|                                                            |",
    "|                                                            |",
    "|                                                            |",
    "|                                                            |",
    "|         You will be disconnected automatically if          |",
    "|          you don't press any keys for 10 minutes.          |",
    "|                                                            |",
    "|                                                            |",
    "o============================================================o",
];

fn render_pause_screen(buffer: &mut RenderBuffer, menu: &Menu) {
    let top_y = (buffer.height - PAUSE_SCREEN.len()) / 2;
    for (i, text) in PAUSE_SCREEN.iter().enumerate() {
        buffer.add_centered_text_with_color(top_y + i, text, Color::GREEN_FOREGROUND);
    }
    menu.render(buffer, top_y + 7);
}

pub async fn play_game(client: &mut Client, mode: Mode) -> Result<(), io::Error> {
    /*
    Grab lobby ID before we lock the game.

    Locking the lobby while game is locked would cause deadlocks, because
    there's lots of other code that locks the game while keeping the lobby
    locked.
    */
    let lobby_id = client.lobby.as_ref().unwrap().lock().unwrap().id.clone();

    let mut pause_menu = Menu {
        items: vec![
            Some("Continue playing".to_string()),
            Some("Quit game".to_string()),
        ],
        selected_index: 0,
    };

    let (game_wrapper, auto_leave_token) = {
        if let Some(result) =
            join_game_in_a_lobby(client.lobby.as_ref().unwrap().clone(), client.id, mode)
        {
            result
        } else {
            // game full
            return Ok(());
        }
    };

    let mut receiver = game_wrapper.status_receiver.clone();
    let mut paused = false;

    loop {
        {
            let mut render_data = client.render_data.lock().unwrap();
            render_data.clear(80, 24);
            let game = game_wrapper.game.lock().unwrap();
            ingame_ui::render(&*game, &mut *render_data, client, &lobby_id);
            if paused {
                render_pause_screen(&mut render_data.buffer, &pause_menu);
            } else {
                pause_menu.selected_index = 0;
            }
            render_data.changed.notify_one();
        }

        tokio::select! {
            result = receiver.changed() => {
                result.unwrap(); // shouldn't fail, because game wrapper still has the sender
                let game_over = match *receiver.borrow() {
                    GameStatus::Playing => { paused = false; false }
                    GameStatus::Paused(_) => { paused = true; false }
                    _ => true,
                };
                if game_over {
                    drop(auto_leave_token);
                    // Locking the lobby here is fine, because we're not locking the game.
                    client.lobby.as_ref().unwrap().lock().unwrap().mark_changed();
                    return show_high_scores(client, receiver).await;
                }
            }
            key = client.receive_key_press() => {
                match key? {
                    KeyPress::Character('P') | KeyPress::Character('p') => {
                        game_wrapper.set_paused(None);
                    }
                    KeyPress::Character('R') | KeyPress::Character('r') => {
                        client.prefer_rotating_counter_clockwise = !client.prefer_rotating_counter_clockwise;
                    }
                    k => {
                        if paused {
                            if pause_menu.handle_key_press(k) {
                                match pause_menu.selected_text() {
                                    "Continue playing" => game_wrapper.set_paused(Some(false)),
                                    "Quit game" => {
                                        // Locking the lobby here is fine, because we're not locking the game.
                                        // We only have access to the immutable GameWrapper.
                                        client.lobby.as_ref().unwrap().lock().unwrap().mark_changed();
                                        return Ok(());
                                    }
                                    _ => panic!(),
                                }
                            }
                        } else {
                            let did_something = game_wrapper.game.lock().unwrap().handle_key_press(
                                client.id, client.prefer_rotating_counter_clockwise, k
                            );
                            if did_something {
                                game_wrapper.mark_changed();
                            }
                        }
                    }
                }
            }
        }
    }
}

fn format_game_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{}sec", seconds)
    } else {
        format!("{}min", seconds / 60)
    }
}

// longest possible return value looks like "42 seconds ago" (14 characters)
fn format_how_long_ago(timestamp: chrono::DateTime<Utc>) -> String {
    let diff = Utc::now() - timestamp;
    let (amount, unit) = if diff.num_seconds() == 0 {
        return "now".to_string();
    } else if diff.num_minutes() == 0 {
        (diff.num_seconds(), "second")
    } else if diff.num_hours() == 0 {
        (diff.num_minutes(), "minute")
    } else if diff.num_days() == 0 {
        (diff.num_hours(), "hour")
    } else if diff.num_weeks() == 0 {
        (diff.num_days(), "day")
        // there's no num_months()
    } else if diff.num_days() <= 30 {
        (diff.num_weeks(), "week")
    } else if diff.num_days() <= 365 {
        (((diff.num_days() as f32) / 365.25 * 12.0) as i64, "month")
    } else {
        (((diff.num_days() as f32) / 365.25) as i64, "year")
    };

    if amount == 0 {
        format!("1 {} ago", unit)
    } else {
        format!("{} {}s ago", amount, unit)
    }
}

fn render_game_over_message(buffer: &mut RenderBuffer, game_result: &GameResult, smile: bool) {
    if smile {
        buffer.add_centered_text(2, "Game over :)");
    } else {
        buffer.add_centered_text(2, "Game over :(");
    }

    let duration_text = format_game_duration(game_result.duration);
    let score_text = format!("{}", game_result.score);

    let (_, right) = buffer.add_centered_text(
        3,
        &format!(
            "The game lasted {} and it ended with score {}.",
            &duration_text, &score_text
        ),
    );
    buffer.add_text_with_color(
        right - ".".len() - score_text.len(),
        3,
        &score_text,
        ingame_ui::SCORE_TEXT_COLOR,
    );
}

fn render_header(buffer: &mut RenderBuffer, this_game_result: &GameResult) {
    let multiplayer = this_game_result.players.len() >= 2;
    let header = format!(
        " HIGH SCORES: {} with {} ",
        this_game_result.mode.name(),
        if multiplayer {
            "multiplayer"
        } else {
            "single player"
        }
    );

    buffer.fill_row_with_char(6, '=');
    buffer.add_centered_text(6, &header);
}

fn format_player_names(full_names: &Vec<String>, maxlen: usize) -> String {
    let mut limit = full_names.iter().map(|n| n.chars().count()).max().unwrap();
    loop {
        let mut result = "".to_string();
        for name in full_names {
            if !result.is_empty() {
                result.push_str(", ");
            }
            if name.chars().count() > limit {
                for ch in name.chars().take(limit - 3) {
                    result.push(ch);
                }
                result.push_str("...");
            } else {
                result.push_str(name);
            }
        }

        if result.chars().count() <= maxlen {
            return result;
        }
        limit -= 1;
    }
}

fn render_table_row(buffer: &mut RenderBuffer, y: usize, text_places: &[usize], texts: &[&str]) {
    for (x, text) in text_places.iter().zip(texts) {
        buffer.add_text(*x, y, text);
    }
}

fn render_high_scores_table(
    buffer: &mut RenderBuffer,
    top_results: &[GameResult],
    this_game_index: Option<usize>,
    multiplayer: bool,
) {
    let last_title = if multiplayer { "Players" } else { "Player" };
    let titles = ["Score", "Duration", "When", last_title];

    let mut rows: Vec<Vec<String>> = top_results
        .iter()
        .map(|result| {
            vec![
                format!("{}", result.score),
                format_game_duration(result.duration),
                result
                    .timestamp
                    .map(format_how_long_ago)
                    .unwrap_or_else(|| "-".to_string()),
                // player names are added later once we know how much width is available
            ]
        })
        .collect();

    let mut separator_places = vec![0];
    for column in 0..3 {
        let width = rows
            .iter()
            .map(|row| row[column].len())
            .chain([titles[column].len()])
            .max()
            .unwrap();
        let last_separator = separator_places.last().unwrap();
        let next_separator = last_separator + 2 + width + 1;
        separator_places.push(next_separator);
    }

    let last_column_text_starts_at = separator_places.last().unwrap() + 2;
    let last_column_max_width = buffer.width - last_column_text_starts_at;
    for (row, result) in rows.iter_mut().zip(top_results) {
        row.push(format_player_names(&result.players, last_column_max_width));
    }

    buffer.fill_row_with_char(9, '-');
    for x in &separator_places {
        for y in 8..(10 + top_results.len()) {
            buffer.set_char(*x, y, '|');
        }
    }

    let text_places: Vec<usize> = separator_places.iter().map(|x| x + 2).collect();
    render_table_row(buffer, 8, &text_places, &titles);
    for (i, row) in rows.iter().enumerate() {
        render_table_row(
            buffer,
            10 + i,
            &text_places,
            &row.iter().map(|s| -> &str { &*s }).collect::<Vec<_>>(),
        );
    }
    if let Some(i) = this_game_index {
        buffer.set_row_color(10 + i, Color::GREEN_BACKGROUND);
    }
}

async fn show_high_scores(
    client: &mut Client,
    mut receiver: watch::Receiver<GameStatus>,
) -> Result<(), io::Error> {
    loop {
        {
            let mut render_data = client.render_data.lock().unwrap();
            render_data.clear(80, 24);
            match &*receiver.borrow() {
                GameStatus::HighScoresLoading => {
                    render_data.buffer.add_centered_text(9, "Loading...");
                }
                GameStatus::HighScoresError => {
                    // hopefully nobody ever sees this...
                    render_data.buffer.add_centered_text_with_color(
                        9,
                        "High Scores Error",
                        Color::RED_FOREGROUND,
                    );
                }
                GameStatus::HighScoresLoaded {
                    this_game_result,
                    top_results,
                    this_game_index,
                } => {
                    render_game_over_message(
                        &mut render_data.buffer,
                        this_game_result,
                        this_game_index.is_some(),
                    );
                    render_header(&mut render_data.buffer, this_game_result);
                    render_high_scores_table(
                        &mut render_data.buffer,
                        top_results,
                        *this_game_index,
                        this_game_result.players.len() >= 2,
                    );
                }
                GameStatus::Playing | GameStatus::Paused(_) => panic!(),
            }

            render_data
                .buffer
                .add_centered_text(19, "Press Enter to continue...");
            render_data.changed.notify_one();
        }

        tokio::select! {
            result = receiver.changed() => {
                result.unwrap(); // apparently never fails, not sure why
            }
            key = client.receive_key_press() => {
                if key? == KeyPress::Enter {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::connection::Receiver;
    use std::path::PathBuf;
    use weak_table::WeakValueHashMap;

    #[tokio::test]
    async fn test_name_entering_on_windows_cmd_exe() {
        let mut client = Client::new(123, Receiver::Test("WindowsUsesCRLF\r\n".to_string()));
        ask_name(&mut client, Arc::new(Mutex::new(HashSet::new())))
            .await
            .unwrap();
        assert_eq!(client.get_name(), Some("WindowsUsesCRLF"));
    }

    #[tokio::test]
    async fn test_forgot_stty_raw() {
        let mut client = Client::new(123, Receiver::Test("Oops\n".to_string()));
        let result = ask_name(&mut client, Arc::new(Mutex::new(HashSet::new()))).await;
        assert!(result.is_err());
        assert_eq!(client.get_name(), None);
        assert!(client
            .text()
            .contains("Your terminal doesn't seem to be in raw mode"));
    }

    #[tokio::test]
    async fn test_entering_name_on_raw_linux_terminal() {
        let mut client = Client::new(123, Receiver::Test("linux_usr\r".to_string()));
        ask_name(&mut client, Arc::new(Mutex::new(HashSet::new())))
            .await
            .unwrap();
        assert_eq!(client.get_name(), Some("linux_usr"));
    }

    #[tokio::test]
    async fn test_long_name() {
        let mut client = Client::new(
            123,
            Receiver::Test("VeryVeryLongNameGoesHere\r".to_string()),
        );
        ask_name(&mut client, Arc::new(Mutex::new(HashSet::new())))
            .await
            .unwrap();
        assert_eq!(client.get_name(), Some("VeryVeryLongNam"));

        // Name should show up as truncated to the user entering it
        assert!(client.text().contains("Name: VeryVeryLongNam"));
        assert!(!client.text().contains("Name: VeryVeryLongName"));
    }

    #[tokio::test]
    async fn test_empty_name() {
        for input in ["\r", "    \r"] {
            let mut client = Client::new(123, Receiver::Test(input.to_string()));
            let result = ask_name(&mut client, Arc::new(Mutex::new(HashSet::new()))).await;
            assert!(result.is_err());
            assert_eq!(client.get_name(), None);
            assert!(client
                .text()
                .contains("Please write a name before pressing Enter"));
        }
    }

    #[tokio::test]
    async fn test_invalid_character_in_name() {
        let mut client = Client::new(123, Receiver::Test(":]\r".to_string()));
        let result = ask_name(&mut client, Arc::new(Mutex::new(HashSet::new()))).await;
        assert!(result.is_err());
        assert_eq!(client.get_name(), None);
        assert!(client
            .text()
            .contains("The name can't contain a ']' character."));
    }

    #[tokio::test]
    async fn test_name_in_use() {
        let names = Arc::new(Mutex::new(HashSet::new()));

        let mut alice = Client::new(1, Receiver::Test("my NAME\r".to_string()));
        let result = ask_name(&mut alice, names.clone()).await;
        assert!(result.is_ok());
        assert_eq!(alice.get_name(), Some("my NAME"));

        // used names are case insensitive
        let mut bob = Client::new(123, Receiver::Test("MY name\r".to_string()));
        let result = ask_name(&mut bob, names.clone()).await;
        assert!(result.is_err());
        assert_eq!(bob.get_name(), None);
        assert!(bob
            .text()
            .contains("This name is in use. Try a different name."));

        drop(alice);
        bob = Client::new(123, Receiver::Test("MY name\r".to_string()));
        let result = ask_name(&mut bob, names.clone()).await;
        assert!(result.is_ok());
        assert_eq!(bob.get_name(), Some("MY name"));
    }

    struct CdToTemporaryDir {
        old_dir: PathBuf,
        _tempdir: tempfile::TempDir,
    }
    impl CdToTemporaryDir {
        fn new() -> Self {
            let old_dir = std::env::current_dir().unwrap();
            let tempdir = tempfile::tempdir().unwrap();
            std::env::set_current_dir(tempdir.path()).unwrap();
            Self {
                old_dir,
                _tempdir: tempdir,
            }
        }
    }
    impl Drop for CdToTemporaryDir {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.old_dir).unwrap();
        }
    }

    #[tokio::test]
    async fn test_motd() {
        let _temp_cd_handle = CdToTemporaryDir::new(); // don't touch user's catris_motd.txt
        let mut client = Client::new(123, Receiver::Test("John7\r".to_string()));
        tokio::fs::write("catris_motd.txt", "Hello World\nSecond line of text\n")
            .await
            .unwrap();
        ask_name(&mut client, Arc::new(Mutex::new(HashSet::new())))
            .await
            .unwrap();

        assert!(ask_if_new_lobby(&mut client).await.is_err());
        assert!(client.text().contains("   Hello World   "));
        assert!(client.text().contains("   Second line of text   "));
    }

    #[tokio::test]
    async fn test_new_lobby_and_select_various_games() {
        let mut client = Client::new(
            123,
            Receiver::Test(
                concat!(
                    "John\r",         // name
                    "\r",             // new lobby
                    "\r",             // select traditional game (first item in list)
                    "g\r",            // select gameplay tips
                    "\x1b[A\x1b[A\r", // arrow up twice to select bottle game
                    "\x1b[B\r",       // arrow down to select ring game
                )
                .to_string(),
            ),
        );
        let result = ask_name(&mut client, Arc::new(Mutex::new(HashSet::new()))).await;
        assert!(result.is_ok());
        let result = ask_if_new_lobby(&mut client).await;
        assert!(result.unwrap());
        client.make_lobby(Arc::new(Mutex::new(WeakValueHashMap::new())));

        let mut selected_index = 0;
        let result = choose_game_mode(&mut client, &mut selected_index).await;
        assert_eq!(result.unwrap(), Some(Mode::Traditional));
        let result = choose_game_mode(&mut client, &mut selected_index).await;
        assert_eq!(result.unwrap(), None); // gameplay tips
        let result = choose_game_mode(&mut client, &mut selected_index).await;
        assert_eq!(result.unwrap(), Some(Mode::Bottle));
        let result = choose_game_mode(&mut client, &mut selected_index).await;
        assert_eq!(result.unwrap(), Some(Mode::Ring));
    }

    #[tokio::test]
    async fn test_quit_items() {
        // Press q to select quit just after entering name
        let mut client = Client::new(1, Receiver::Test("Alice\rq\r".to_string()));
        let result = ask_name(&mut client, Arc::new(Mutex::new(HashSet::new()))).await;
        assert!(result.is_ok());
        let result = ask_if_new_lobby(&mut client).await;
        assert_eq!(
            result.unwrap_err().to_string(),
            "user selected \"Quit\" in menu"
        );

        // Make a new lobby before pressing quit, with two consecutive enter presses.
        // The lobby view has another quit button.
        let mut client = Client::new(2, Receiver::Test("Bob\r\rq\r".to_string()));
        let result = ask_name(&mut client, Arc::new(Mutex::new(HashSet::new()))).await;
        assert!(result.is_ok());
        let result = ask_if_new_lobby(&mut client).await;
        assert!(result.unwrap());
        client.make_lobby(Arc::new(Mutex::new(WeakValueHashMap::new())));
        let result = choose_game_mode(&mut client, &mut 0).await;
        assert_eq!(
            result.unwrap_err().to_string(),
            "user selected \"Quit\" in menu"
        );
    }

    async fn make_client_and_enter_lobby_id(
        name: &str,
        id_to_enter: &str,
        lobbies: Lobbies,
    ) -> Client {
        let client_id = name.chars().map(|c| u32::from(c) as u64).sum();
        let mut client = Client::new(
            client_id,
            Receiver::Test(format!("{}\r{}\r", name, id_to_enter)),
        );
        let result = ask_name(&mut client, Arc::new(Mutex::new(HashSet::new()))).await;
        assert!(result.is_ok());
        _ = ask_lobby_id_and_join_lobby(&mut client, lobbies).await;
        client
    }

    #[tokio::test]
    async fn test_joining_existing_lobby() {
        let lobbies = Arc::new(Mutex::new(WeakValueHashMap::new()));

        // Alice makes a new lobby
        let mut alice = Client::new(1, Receiver::Test("Alice\r".to_string()));
        let result = ask_name(&mut alice, Arc::new(Mutex::new(HashSet::new()))).await;
        assert!(result.is_ok());
        alice.make_lobby(lobbies.clone());

        let lobby_id = alice.lobby.as_ref().unwrap().lock().unwrap().id.clone();
        assert_eq!(lobby_id.len(), 6);
        assert!(lobby_id.is_ascii());
        assert_eq!(lobby_id, lobby_id.to_ascii_uppercase());

        // Bob attempts to join the lobby, with various IDs
        let bob = make_client_and_enter_lobby_id("Bob", "hello", lobbies.clone()).await;
        assert!(bob
            .text()
            .contains("The text you entered doesn't look like a lobby ID."));
        assert!(!bob.text().contains("Alice"));

        let bob = make_client_and_enter_lobby_id("Bob", "", lobbies.clone()).await;
        assert!(bob
            .text()
            .contains("The text you entered doesn't look like a lobby ID."));
        assert!(!bob.text().contains("Alice"));

        // Bob finally enters the correct ID. Lobby IDs are case insensitive.
        let mut bob =
            make_client_and_enter_lobby_id("Bob", &lobby_id.to_lowercase(), lobbies.clone()).await;

        assert!(choose_game_mode(&mut bob, &mut 0).await.is_err());
        assert!(bob.text().contains(&lobby_id));
        assert!(bob.text().contains("1. Alice"));
        assert!(bob.text().contains("2. Bob (you)"));
        drop(alice);
        assert!(choose_game_mode(&mut bob, &mut 0).await.is_err());
        assert!(bob.text().contains(&lobby_id));
        assert!(!bob.text().contains("Alice"));
        assert!(bob.text().contains("1. Bob (you)"));
        drop(bob);

        // Lobby stops existing once everyone quits
        let charlie = make_client_and_enter_lobby_id("Charlie", &lobby_id, lobbies).await;
        assert!(charlie.text().contains("There is no lobby with ID '"));
    }

    #[tokio::test]
    async fn test_lobby_full() {
        let lobbies = Arc::new(Mutex::new(WeakValueHashMap::new()));

        let mut alice = Client::new(1, Receiver::Test("Alice\r".to_string()));
        let result = ask_name(&mut alice, Arc::new(Mutex::new(HashSet::new()))).await;
        assert!(result.is_ok());
        alice.make_lobby(lobbies.clone());
        let lobby_id = alice.lobby.as_ref().unwrap().lock().unwrap().id.clone();

        let mut bobs = vec![];
        for i in 1..MAX_CLIENTS_PER_LOBBY {
            bobs.push(
                make_client_and_enter_lobby_id(&format!("Bob {}", i), &lobby_id, lobbies.clone())
                    .await,
            );
        }

        let charlie = make_client_and_enter_lobby_id("Charlie", &lobby_id, lobbies.clone()).await;
        assert!(charlie
            .text()
            .contains("' is full. It already has 6 players."));

        bobs.pop();
        let charlie = make_client_and_enter_lobby_id("Charlie", &lobby_id, lobbies.clone()).await;
        assert!(!charlie.text().contains("is full"));
    }

    #[tokio::test]
    async fn test_game_full() {
        let lobbies = Arc::new(Mutex::new(WeakValueHashMap::new()));
        let mut lobby_id: Option<String> = None;

        // Ring game has max of 4 players, so add 5 clients to the same lobby and see what happens.
        let mut last_client = None;
        for i in 0..5 {
            let text = if i == 0 {
                format!("Client 0\rBLOCK")
            } else if i < 4 {
                format!("Client {}\r{}\rBLOCK", i, lobby_id.as_ref().unwrap())
            } else {
                // Select ring game by pressing R.
                // Other clients skip the game choosing menu in this test
                format!("Client {}\r{}\rR", i, lobby_id.as_ref().unwrap())
            };
            let mut client = Client::new(i, Receiver::Test(text));

            ask_name(&mut client, Arc::new(Mutex::new(HashSet::new())))
                .await
                .unwrap();

            if i == 0 {
                client.make_lobby(lobbies.clone());
                lobby_id = Some(client.lobby.as_ref().unwrap().lock().unwrap().id.clone());
            } else {
                ask_lobby_id_and_join_lobby(&mut client, lobbies.clone())
                    .await
                    .unwrap();
            }

            if i == 4 {
                last_client = Some(client);
            } else {
                tokio::spawn(async move {
                    _ = play_game(&mut client, Mode::Ring).await;
                });
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        let mut client = last_client.unwrap();
        let choose_result = choose_game_mode(&mut client, &mut 0).await;
        assert!(choose_result.is_err());
        assert!(client.text().contains("Ring game (4/4 players)"));
        assert!(client.text().contains("This game is full."));
    }

    #[tokio::test]
    async fn test_show_high_scores() {
        let this_game_result = GameResult {
            duration: Duration::from_secs(123),
            mode: Mode::Traditional,
            score: 500,
            players: vec!["Foo".to_string(), "Bar".to_string()],
            timestamp: Some(Utc::now()),
        };

        let top_results = vec![
            GameResult {
                duration: Duration::from_secs(666),
                mode: Mode::Traditional,
                score: 1000,
                players: vec!["Alice".to_string(), "Bob".to_string()],
                timestamp: None,
            },
            this_game_result.clone(),
            GameResult {
                duration: Duration::from_secs(5),
                mode: Mode::Traditional,
                score: 10,
                players: vec![
                    "very long name i have".to_string(),
                    "IHaveVeryLongName".to_string(),
                    "Long long name".to_string(),
                    "short name".to_string(),
                ],
                timestamp: Some(Utc::now() - chrono::Duration::days(3)),
            },
        ];

        let mut client = Client::new(1, Receiver::Test("\r".to_string()));

        let status = GameStatus::HighScoresLoaded {
            this_game_result,
            top_results,
            this_game_index: Some(1),
        };
        let (_status_sender, status_receiver) = watch::channel(status);
        let result = show_high_scores(&mut client, status_receiver).await;
        assert!(result.is_ok());

        assert!(client.text().starts_with(concat!(
            "                                                                                \n",
            "                                                                                \n",
            "                                  Game over :)                                  \n",
            "                The game lasted 2min and it ended with score 500.               \n",
            "                                                                                \n",
            "                                                                                \n",
            "================ HIGH SCORES: Traditional game with multiplayer ================\n",
            "                                                                                \n",
            "| Score | Duration | When       | Players                                       \n",
            "|-------|----------|------------|-----------------------------------------------\n",
            "| 1000  | 11min    | -          | Alice, Bob                                    \n",
            "| 500   | 2min     | now        | Foo, Bar                                      \n",
            "| 10    | 5sec     | 3 days ago | very lo..., IHaveVe..., Long lo..., short name\n",
            "                                                                                \n",
        )));

        // second row should be highlighted, because it represents the current game
        assert_eq!(
            client.text_with_color(Color::GREEN_BACKGROUND),
            "| 500   | 2min     | now        | Foo, Bar                                      "
        );
    }
}
