use std::io;
use std::io::Write;
use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;
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
        buffer.add_text(0, y, line.to_string(), ansi::DEFAULT_COLORS);
        y += 1;
    }
}

async fn prompt(
    client: &mut client::Client,
    prompt: String,
    end_lines: Vec<String>,
    validator: fn(&String) -> Option<String>,
    add_extra_text: Option<fn(&mut render::Buffer)>,
) -> Result<String, io::Error> {
    let mut error = Some("".to_string());
    let mut current_text = "".to_string();

    loop {
        {
            let mut render_data = client.render_data.lock().unwrap();
            render_data.buffer.clear();
            render_data.buffer.resize(80, 24);

            add_ascii_art(&mut render_data.buffer);
            let mut x =
                render_data
                    .buffer
                    .add_text(20, 10, prompt.clone(), ansi::DEFAULT_COLORS);
            x = render_data
                .buffer
                .add_text(x, 10, current_text.clone(), ansi::DEFAULT_COLORS);
            render_data.cursor_pos = Some((x, 10));
            render_data
                .buffer
                .add_text(2, 13, error.clone().unwrap_or_default(), ansi::RED_FG);
            match add_extra_text {
                Some(f) => f(&mut render_data.buffer),
                None => {}
            }

            render_data.changed.notify_one();
        }

        match client.receive_key_press().await? {
            ansi::KeyPress::Character(ch) => {
                // 15 chars is enough for names and lobby IDs
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
                let text = current_text.trim().to_string();
                error = validator(&text);
                if error == None {
                    return Ok(text);
                }
            }
            _ => {}
        }
    }
}

// latin-1 chars, aka bytes(range(256)).decode("latin-1"), with some removed
// It's important to ban characters that are more than 1 unit wide on terminal.
const VALID_NAME_CHARS: &str = concat!(
    " !\"#$%&'()*+-./:;<=>?@\\^_`{|}~¡¢£¤¥¦§¨©ª«¬®¯°±²³´µ¶·¸¹º»¼½¾¿×÷",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
    "abcdefghijklmnopqrstuvwxyz",
    "0123456789",
    "ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÐÑÒÓÔÕÖØÙÚÛÜÝÞßàáâãäåæçèéêëìíîïðñòóôõöøùúûüýþÿ",
);

fn name_validator(name: &String) -> Option<String> {
    if name.len() == 0 {
        return Some("Please write a name before pressing Enter.".to_string());
    }
    for ch in name.chars() {
        if !VALID_NAME_CHARS.contains(ch) {
            return Some(format!("The name can't contain a '{}' character.", ch));
        }
    }
    None
}

fn add_name_asking_notes(buffer: &mut render::Buffer) {
    buffer.add_centered_text(
        17,
        "If you play well, your name will be".to_string(),
        ansi::DEFAULT_COLORS,
    );
    buffer.add_centered_text(
        18,
        "visible to everyone in the high scores.".to_string(),
        ansi::DEFAULT_COLORS,
    );

    // FIXME: ip logging is currently a false claim, always printed
    buffer.add_centered_text(
        20,
        "Your IP will be logged on the server only if you".to_string(),
        ansi::DEFAULT_COLORS,
    );
    buffer.add_centered_text(
        21,
        "connect 5 or more times within the same minute.".to_string(),
        ansi::DEFAULT_COLORS,
    );
}

pub async fn ask_name(client: &mut client::Client) -> Result<String, io::Error> {
    return prompt(
        client,
        "Name: ".to_string(),
        vec![],
        name_validator,
        Some(add_name_asking_notes),
    )
    .await;
}
