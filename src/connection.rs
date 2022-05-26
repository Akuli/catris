use std::cmp::min;
use std::io::Write;
use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::sleep;

use crate::game_logic;
use crate::render;

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

struct ConnectionLogger {
    ip: IpAddr,
    id: u64,
}

impl ConnectionLogger {
    fn new(ip: IpAddr) -> ConnectionLogger {
        let result = ConnectionLogger {
            ip: ip,
            // https://stackoverflow.com/a/32936288
            id: ID_COUNTER.fetch_add(1, Ordering::SeqCst),
        };
        result.log(format!("New connection from {}", ip));
        result
    }

    fn log(&self, message: String) {
        println!("[client {}] {}", self.id, message);
    }
}

impl Drop for ConnectionLogger {
    fn drop(&mut self) {
        self.log("Disconnected".to_string());
    }
}

fn parse_as_much_utf8_as_possible(bytes: &[u8], dest: &mut String) -> usize {
    let mut i = 0;
    while i < bytes.len() {
        match std::str::from_utf8(&bytes[i..]) {
            Ok(s) => {
                dest.push_str(&s);
                i = bytes.len();
                break;
            }
            Err(e) => {
                let start = i;
                i += e.valid_up_to();
                dest.push_str(std::str::from_utf8(&bytes[start..i]).unwrap());
                if let Some(n) = e.error_len() {
                    // skip invalid utf-8
                    if n <= 0 {
                        panic!("wat");
                    }
                    i += n;
                } else {
                    // stop when utf-8 looks valid so far but only part of a character received
                    break;
                }
            }
        }
    }
    i
}

#[derive(Debug)]
enum KeyPress {
    Up,
    Down,
    Right,
    Left,
    BackSpace, // TODO
    Character(char),
}

fn parse_key_presses(text: String) -> (Vec<KeyPress>, String) {
    let mut result = vec![];
    let mut i = 0;
    while i < text.len() {
        if text[i..].to_string() == "\x1b".to_string()
            || text[i..].to_string() == "\x1b[".to_string()
        {
            // part of arrow key, not fully received yet
            return (result, text[i..].to_string());
        }

        let mut n = 3;
        match &text[i..min(text.len(), i + 3)] {
            "\x1b[A" => result.push(KeyPress::Up),
            "\x1b[B" => result.push(KeyPress::Down),
            "\x1b[C" => result.push(KeyPress::Right),
            "\x1b[D" => result.push(KeyPress::Left),
            _ => {
                n = 1;
                match &text[i..i + 1] {
                    "\x7f" => result.push(KeyPress::BackSpace), // linux/mac terminal
                    "\x08" => result.push(KeyPress::BackSpace), // windows cmd
                    // FIXME: use text.chars() instead of indexing
                    byte => result.push(KeyPress::Character(byte.chars().next().unwrap())),
                }
            }
        }
        i += n;
    }
    (result, "".to_string())
}

async fn handle_receiving(reader: &mut OwnedReadHalf, logger: &ConnectionLogger) {
    let mut buffer = [0 as u8; 100];
    let mut bytes_received = 0 as usize;
    let mut text = "".to_string();
    loop {
        // Receive bytes. May be incomplete or invalid utf-8 characters, or
        // incomplete ansi escape sequence.
        match reader.read(&mut buffer[bytes_received..]).await {
            Ok(n) => {
                if n == 0 {
                    // clean disconnect
                    return;
                }
                bytes_received += n;
            }
            Err(e) => {
                logger.log(format!("can't recv: {}", e));
                return;
            }
        }

        // Parse as much as possible into utf-8 string
        let bytes_valid_utf8 =
            parse_as_much_utf8_as_possible(&buffer[0..bytes_received], &mut text);
        for i in bytes_valid_utf8..bytes_received {
            buffer[i - bytes_valid_utf8] = buffer[i];
        }
        bytes_received -= bytes_valid_utf8;

        // Parse as much characters and ANSI codes as possible
        let (key_presses, remaining_text) = parse_key_presses(text);
        text = remaining_text;

        for k in key_presses {
            println!("got key press: {:?}", k);
        }
    }
}

async fn handle_sending(
    mut writer: OwnedWriteHalf,
    mut need_render_receiver: watch::Receiver<()>,
    game: Arc<Mutex<game_logic::Game>>,
    logger: &ConnectionLogger,
) {
    let mut last_rendered = render::RenderBuffer::new();
    let mut currently_rendering = render::RenderBuffer::new();

    loop {
        currently_rendering.clear();
        game.lock().unwrap().render_to_buf(&mut currently_rendering);
        let to_send = currently_rendering.get_updates_as_ansi_codes(&mut last_rendered);
        if let Err(e) = writer.write_all(to_send.as_bytes()).await {
            logger.log(format!("can't send: {}", e));
            return;
        }
        currently_rendering.copy_into(&mut last_rendered);
        need_render_receiver.changed().await.unwrap();
    }
}

pub async fn handle_connection(
    socket: TcpStream,
    ip: IpAddr,
    need_render_receiver: watch::Receiver<()>,
    game: Arc<Mutex<game_logic::Game>>,
) {
    let logger = ConnectionLogger::new(ip);
    let (mut reader, writer) = socket.into_split();
    tokio::select! {
        _ = handle_receiving(&mut reader, &logger) => {},
        _ = handle_sending(writer, need_render_receiver, game, &logger) => {},
    }
}
