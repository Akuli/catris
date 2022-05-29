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

pub async fn prompt(
    client: &mut client::Client,
    prompt: String,
    end_text: Vec<String>,
) -> Result<String, io::Error> {
    let mut error = "".to_string();
    let mut current_text = "".to_string();

    loop {
        {
            let mut rd = client.render_data.lock().unwrap();
            rd.buffer.clear();
            rd.buffer.resize(80, 24);
            add_ascii_art(&mut rd.buffer);
            let mut x = rd
                .buffer
                .add_text(20, 10, prompt.clone(), ansi::DEFAULT_COLORS);
            x = rd
                .buffer
                .add_text(x, 10, current_text.clone(), ansi::DEFAULT_COLORS);
            rd.cursor_pos = Some((x, 10));
            rd.changed.notify_one();
        }

        match client.receive_key_press().await? {
            ansi::KeyPress::Enter => {
                return Ok(current_text);
            }
            ansi::KeyPress::Character(ch) => {
                current_text.push(ch);
            }
            ansi::KeyPress::BackSpace => {
                if current_text.len() > 0 {
                    current_text.pop();
                }
            }
            _ => {}
        }
    }
}
