use crate::ansi::Color;
use crate::ansi::KeyPress;
use crate::client::Client;
use crate::game_logic::Mode;
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
use std::collections::HashSet;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::watch;

const ASCII_ART: &[&str] = &[
    "",
    r"  __     ___    _____   _____   _____   ___ ",
    r" / _)   / _ \  |_   _| |  __ \ |_   _| / __)",
    r"| (_   / /_\ \   | |   |  _  /  _| |_  \__ \",
    r" \__) /_/   \_\  |_|   |_| \_\ |_____| (___/",
    "https://github.com/Akuli/catris",
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
                if current_text.len() > 0 {
                    current_text.pop();
                }
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
            if name.len() == 0 {
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
        &self.items[self.selected_index].as_ref().unwrap()
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
        return false;
    }
}

pub async fn ask_if_new_lobby(client: &mut Client) -> Result<bool, io::Error> {
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
                .add_centered_text(18, "If you want to play alone, just make a new lobby.");
            render_data.buffer.add_centered_text(
                20,
                "For multiplayer, one player makes a lobby and others join it.",
            );
            render_data.changed.notify_one();
        }

        let key = client.receive_key_press().await?;
        if menu.handle_key_press(key) {
            return match menu.selected_text() {
                "New lobby" => Ok(true),
                "Join an existing lobby" => Ok(false),
                "Quit" => Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
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
                                    io::ErrorKind::ConnectionAborted,
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
    "",
    "Keys:",
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
        for y in 0..GAMEPLAY_TIPS.len() {
            let mut x = 2;
            let mut string = GAMEPLAY_TIPS[y];
            loop {
                match string.chars().next() {
                    Some('[') => {
                        color = Color::MAGENTA_FOREGROUND;
                        string = &string[1..];
                    }
                    Some('{') => {
                        color = Color::CYAN_FOREGROUND;
                        string = &string[1..];
                    }
                    Some(']') | Some('}') => {
                        color = Color::DEFAULT;
                        string = &string[1..];
                    }
                    Some(_) => {
                        let i = string.find(|c| "[]{}".contains(c)).unwrap_or(string.len());
                        x = render_data
                            .buffer
                            .add_text_with_color(x, y, &string[..i], color);
                        string = &string[i..];
                    }
                    None => break,
                }
            }
        }

        menu.render(&mut render_data.buffer, 19);
        render_data.changed.notify_one();
    }

    while !menu.handle_key_press(client.receive_key_press().await?) {}
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

    // TODO: should these be .subscribe()? grep for subscribe to find another place that needs it
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

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{}sec", seconds)
    } else {
        format!("{}min", seconds / 60)
    }
}

fn render_game_over_message(buffer: &mut RenderBuffer, game_result: &GameResult, smile: bool) {
    if smile {
        buffer.add_centered_text(2, "Game over :)");
    } else {
        buffer.add_centered_text(2, "Game over :(");
    }

    let duration_text = format_duration(game_result.duration);
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
    buffer.add_text(0, 6, &format!("{:=^80}", header));
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

        if result.chars().count() < maxlen {
            return result;
        }
        limit -= 1;
    }
}

fn render_high_scores_table(
    buffer: &mut RenderBuffer,
    top_results: &Vec<GameResult>,
    this_game_index: Option<usize>,
    multiplayer: bool,
) {
    buffer.add_text(
        0,
        8,
        if multiplayer {
            "| Score | Duration | Players"
        } else {
            "| Score | Duration | Player"
        },
    );

    let header_prefix = "|-------|----------|-";
    buffer.add_text(0, 9, &format!("{:-<80}", header_prefix));

    for (i, result) in top_results.iter().enumerate() {
        let row = format!(
            "| {:<6}| {:<9}| {}",
            result.score,
            &format_duration(result.duration),
            &format_player_names(&result.players, 80 - header_prefix.len() - 1)
        );
        if this_game_index == Some(i) {
            buffer.add_text_with_color(0, 10 + i, &format!("{:<80}", row), Color::GREEN_BACKGROUND);
        } else {
            buffer.add_text(0, 10 + i, &row);
        }
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
                match key? {
                    KeyPress::Enter => {
                        return Ok(());
                    }
                    _ => {},
                }
            }
        }
    }
}
