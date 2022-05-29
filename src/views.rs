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
use tokio::sync::watch;
use tokio::time::sleep;
use weak_table::WeakValueHashMap;

use crate::ansi;
use crate::client;
use crate::render;

pub trait View: Send {
    fn render(&self, buffer: &mut render::Buffer);

    // None means hide cursor
    fn get_cursor_pos(&self) -> Option<(usize, usize)> {
        None
    }
}

// Outer Arc so that the ref as a whole can be passed around. Changes will appear everywhere.
// Inner Arc so that you can hold a specific type of view while the ref uses it too.
// Both need mutex to make compiler happy?
// TODO: pretty sure single level of nesting enough? maybe Box for inner?
pub type ViewRef = Arc<Mutex<Arc<Mutex<dyn View>>>>;

pub struct DummyView {}

impl View for DummyView {
    fn render(&self, buffer: &mut render::Buffer) {
        buffer.resize(80, 24);
    }
}

const ASCII_ART: &str = r"
                   __     ___    _____   _____   _____   ___
                  / _)   / _ \  |_   _| |  __ \ |_   _| / __)
                 | (_   / /_\ \   | |   |  _  /  _| |_  \__ \
                  \__) /_/   \_\  |_|   |_| \_\ |_____| (___/
                        https://github.com/Akuli/catris
";

pub struct TextEntryView {
    prompt: String,
    current_text: String,
    error: String,
    end_text: Vec<String>,
}

impl TextEntryView {
    pub fn new(prompt: String, end_text: Vec<String>) -> TextEntryView {
        TextEntryView {
            prompt: prompt,
            current_text: "".to_string(),
            error: "".to_string(),
            end_text: end_text,
        }
    }

    pub async fn run(&mut self, client: &mut client::Client) -> Result<String, io::Error> {
        loop {
            let k = client.receive_key_press().await?;
            println!("Got {:?}", k);
        }
    }
}

impl View for TextEntryView {
    fn render(&self, buffer: &mut render::Buffer) {
        buffer.resize(80, 24);
        let mut y = 0;
        for line in ASCII_ART.lines() {
            buffer.add_text(0, y, line.to_string(), ansi::Colors { fg: 0, bg: 0 });
            y += 1;
        }
    }
}
