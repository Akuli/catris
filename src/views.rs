use std::collections::HashSet;
use std::io;
use std::io::Write;
use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio::time::sleep;
use weak_table::WeakValueHashMap;

use crate::ansi;
use crate::client;
use crate::game_logic;
use crate::lobby;
use crate::render;

const ASCII_ART: &str = r"
                   __     ___    _____   _____   _____   ___
                  / _)   / _ \  |_   _| |  __ \ |_   _| / __)
                 | (_   / /_\ \   | |   |  _  /  _| |_  \__ \
                  \__) /_/   \_\  |_|   |_| \_\ |_____| (___/
                        https://github.com/Akuli/catris
";

fn add_ascii_art(buffer: &mut render::Buffer) {
    let mut y = 0;
    for line in ASCII_ART.lines() {
        buffer.add_text(0, y, line);
        y += 1;
    }
}

async fn prompt<F>(
    client: &mut client::Client,
    prompt: &str,
    mut enter_pressed_callback: F,
    add_extra_text: Option<fn(&mut render::Buffer)>,
    min_duration_between_enter_presses: Duration,
) -> Result<(), io::Error>
where
    F: FnMut(&str, &mut client::Client) -> Option<String>,
{
    let mut error = Some("".to_string());
    let mut current_text = "".to_string();
    let mut last_enter_press: Option<Instant> = None;

    loop {
        {
            let mut render_data = client.render_data.lock().unwrap();
            render_data.buffer.clear();
            render_data.buffer.resize(80, 24);

            add_ascii_art(&mut render_data.buffer);
            let mut x = render_data.buffer.add_text(20, 10, prompt);
            x = render_data.buffer.add_text(x, 10, &current_text);
            render_data.cursor_pos = Some((x, 10));
            render_data.buffer.add_text_with_color(
                2,
                13,
                &error.clone().unwrap_or_default(),
                ansi::RED_FOREGROUND,
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
            ansi::KeyPress::Character('\n') if last_enter_press == None => {
                error = Some(
                    "Your terminal doesn't seem to be in raw mode. Run 'stty raw' and try again."
                        .to_string(),
                );
            }
            ansi::KeyPress::Character(ch) => {
                // 15 chars is enough for names and lobby IDs
                // It's important to have limit (potential out of mem dos attack otherwise)
                if current_text.chars().count() < 15 {
                    current_text.push(ch);
                }
            }
            ansi::KeyPress::BackSpace => {
                if current_text.len() > 0 {
                    current_text.pop();
                }
            }
            ansi::KeyPress::Enter => {
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

fn add_name_asking_notes(buffer: &mut render::Buffer) {
    buffer.add_centered_text(17, "If you play well, your name will be");
    buffer.add_centered_text(18, "visible to everyone in the high scores.");

    buffer.add_centered_text(20, "Your IP will be logged on the server only if you");
    buffer.add_centered_text(21, "connect 5 or more times within the same minute.");
}

pub async fn ask_name(
    client: &mut client::Client,
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
    client: &mut client::Client,
    lobbies: lobby::Lobbies,
) -> Result<(), io::Error> {
    prompt(
        client,
        "Lobby ID (6 characters): ",
        |id, client| {
            let id = id.to_uppercase();
            if !lobby::looks_like_lobby_id(&id) {
                return Some("The text you entered doesn't look like a lobby ID.".to_string());
            }

            let lobbies = lobbies.lock().unwrap();
            if let Some(lobby) = lobbies.get(&id) {
                client.join_lobby(lobby);
                return None;
            }

            return Some(format!("There is no lobby with ID '{}'.", id));
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

fn case_insensitive_starts_with(s: &str, prefix: char) -> bool {
    return s.chars().next().unwrap().to_lowercase().to_string()
        == prefix.to_lowercase().to_string();
}

impl Menu {
    fn selected_text(&self) -> &str {
        &self.items[self.selected_index].as_ref().unwrap()
    }

    fn render(&self, buffer: &mut render::Buffer, top_y: usize) {
        for i in 0..self.items.len() {
            if let Some(text) = &self.items[i] {
                let centered_text = format!("{:^35}", text);
                if i == self.selected_index {
                    buffer.add_centered_text_with_color(
                        top_y + i,
                        &centered_text,
                        ansi::BLACK_ON_WHITE,
                    );
                } else {
                    buffer.add_centered_text(top_y + i, &centered_text);
                }
            }
        }
    }

    // true means enter pressed
    fn handle_key_press(&mut self, key: ansi::KeyPress) -> bool {
        let last = self.items.len() - 1;
        match key {
            ansi::KeyPress::Up if self.selected_index != 0 => {
                self.selected_index -= 1;
                while self.items[self.selected_index].is_none() {
                    self.selected_index -= 1;
                }
            }
            ansi::KeyPress::Down if self.selected_index != last => {
                self.selected_index += 1;
                while self.items[self.selected_index].is_none() {
                    self.selected_index += 1;
                }
            }
            ansi::KeyPress::Character(ch) => {
                // pressing r selects Ring Game
                for i in 0..self.items.len() {
                    if let Some(text) = &self.items[i] {
                        if case_insensitive_starts_with(text, ch) {
                            self.selected_index = i;
                            break;
                        }
                    }
                }
            }
            ansi::KeyPress::Enter => {
                return true;
            }
            _ => {}
        }
        return false;
    }
}

pub async fn ask_if_new_lobby(client: &mut client::Client) -> Result<bool, io::Error> {
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
            render_data.buffer.clear();
            render_data.buffer.resize(80, 24);
            render_data.cursor_pos = None;

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

// None return value means show gameplay tips
pub async fn choose_game_mode(
    client: &mut client::Client,
    selected_index: &mut usize,
) -> Result<Option<game_logic::GameMode>, io::Error> {
    let mut items = vec![];
    items.resize(game_logic::ALL_GAME_MODES.len(), Some("".to_string()));
    items.push(None);
    items.push(Some("Gameplay tips".to_string()));
    items.push(Some("Quit".to_string()));
    let mut menu = Menu {
        items: items,
        selected_index: *selected_index,
    };

    loop {
        for i in 0..game_logic::ALL_GAME_MODES.len() {
            let mode = game_logic::ALL_GAME_MODES[i];
            menu.items[i] = Some(format!(
                "{} (0/{} players)",
                mode.name(),
                mode.max_players()
            ));
        }

        {
            let mut render_data = client.render_data.lock().unwrap();
            render_data.buffer.clear();
            render_data.buffer.resize(80, 24);
            render_data.cursor_pos = None;

            add_ascii_art(&mut render_data.buffer);
            menu.render(&mut render_data.buffer, 13);
            render_data.changed.notify_one();
        }

        let key = client.receive_key_press().await?;
        if menu.handle_key_press(key) {
            *selected_index = menu.selected_index;
            return match menu.selected_text() {
                "Gameplay tips" => Ok(None),
                "Quit" => Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "user selected \"Quit\" in menu",
                )),
                _ => Ok(Some(game_logic::ALL_GAME_MODES[menu.selected_index])),
            };
        }
    }
}
