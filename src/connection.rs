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

#[derive(Debug)]
enum KeyPress {
    Up,
    Down,
    Right,
    Left,
    BackSpace,
    Character(char),
}

// Returning None means need to receive more data.
// The usize is how many bytes were consumed.
fn parse_key_press(data: &[u8]) -> Option<(KeyPress, usize)> {
    match data {
        b"" => None,
        b"\x1b" => None,
        b"\x1b[" => None,
        b"\x1b[A" => Some((KeyPress::Up, 3)),
        b"\x1b[B" => Some((KeyPress::Down, 3)),
        b"\x1b[C" => Some((KeyPress::Right, 3)),
        b"\x1b[D" => Some((KeyPress::Left, 3)),
        b"\x7f" => Some((KeyPress::BackSpace, 1)), // linux/mac terminal
        b"\x08" => Some((KeyPress::BackSpace, 1)), // windows cmd.exe
        // utf-8 chars are never >4 bytes long
        _ => match std::str::from_utf8(&data[0..min(data.len(), 4)]) {
            Ok(s) => {
                let ch = s.chars().next().unwrap();
                Some((KeyPress::Character(ch), ch.to_string().len()))
            }
            Err(e) => {
                // see std::str::Utf8Error
                if e.valid_up_to() == 0 {
                    if e.error_len() == None {
                        // need more data
                        None
                    } else {
                        Some((KeyPress::Character(std::char::REPLACEMENT_CHARACTER), 1))
                    }
                } else {
                    let ch = std::str::from_utf8(&data[..e.valid_up_to()])
                        .unwrap()
                        .chars()
                        .next()
                        .unwrap();
                    Some((KeyPress::Character(ch), ch.to_string().len()))
                }
            }
        },
    }
}

async fn handle_receiving(reader: &mut OwnedReadHalf, logger: &ConnectionLogger) {
    let mut buffer = [0 as u8; 100];
    let mut bytes_received = 0 as usize;
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

        let mut consumed = 0 as usize;
        loop {
            match parse_key_press(&buffer[consumed..bytes_received]) {
                None => {
                    println!("need more data");
                    break;
                }
                Some((keypress, n)) => {
                    println!("got {:?}, skipping {} bytes", keypress, n);
                    consumed += n;
                }
            }
        }

        for i in consumed..bytes_received {
            buffer[i - consumed] = buffer[i];
        }
        bytes_received -= consumed;
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
