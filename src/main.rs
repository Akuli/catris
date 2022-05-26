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

mod ansi;
mod game_logic;
mod render;

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

        // Parse valid utf-8 parts. Can still contain incomplete ANSI escape sequences.
        let parsed_start;
        let parsed_end;
        {
            let mut to_parse = &buffer[0..bytes_received];
            while to_parse.len() > 0 {
                match std::str::from_utf8(to_parse) {
                    Ok(s) => {
                        text.push_str(&s);
                        to_parse = &to_parse[to_parse.len()..];
                    }
                    Err(e) => {
                        let (valid, after_valid) = to_parse.split_at(e.valid_up_to());
                        text.push_str(std::str::from_utf8(valid).unwrap());
                        if let Some(n) = e.error_len() {
                            // not enough data to get any valid characters
                            if n <= 0 {
                                panic!("wat");
                            }
                            to_parse = &after_valid[n..];
                        } else {
                            // not enough data to continue parsing
                            break;
                        }
                    }
                }
            }
            parsed_start = to_parse.as_ptr() as usize - buffer.as_ptr() as usize;
            parsed_end = to_parse.as_ptr() as usize - buffer.as_ptr() as usize;
        }
        for i in parsed_start..parsed_end {
            buffer[i - parsed_start] = buffer[i];
        }
        bytes_received = parsed_end - parsed_start;

        println!("got text: {}", text);
        text = "".to_string();
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

async fn handle_connection(
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

async fn move_blocks_down_task(
    game: Arc<Mutex<game_logic::Game>>,
    need_render_sender: watch::Sender<()>,
) {
    loop {
        game.lock().unwrap().move_blocks_down();
        need_render_sender.send(()).unwrap();
        sleep(Duration::from_millis(400)).await;
    }
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:12345").await.unwrap();

    let game = Arc::new(Mutex::new(game_logic::Game::new("Foo".to_string())));

    let (need_render_sender, need_render_receiver) = watch::channel(());
    tokio::spawn(move_blocks_down_task(game.clone(), need_render_sender));

    loop {
        let (socket, sockaddr) = listener.accept().await.unwrap();
        {
            let game = game.clone();
            let need_render_receiver = need_render_receiver.clone();
            tokio::spawn(async move {
                handle_connection(socket, sockaddr.ip(), need_render_receiver, game).await;
            });
        }
    }
}
