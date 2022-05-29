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
        buffer.add_text(0, y, line.to_string(), ansi::Colors { fg: 0, bg: 0 });
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
            rd.cursor_pos = Some((1, 2));
            rd.changed.notify_one();
        }

        println!("prompt() got {:?}", client.receive_key_press().await?);
    }
}
