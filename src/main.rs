use std::io::Write;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::sleep;

mod ansi;
mod game_logic;
mod render;

use crate::render::RenderBuffer;

async fn handle_receiving(reader: OwnedReadHalf) {
    loop {
        sleep(Duration::from_millis(400)).await;
    }
}

async fn handle_sending(
    mut writer: OwnedWriteHalf,
    mut need_render_receiver: watch::Receiver<()>,
    game: Arc<Mutex<game_logic::Game>>,
) {
    let mut last_rendered = RenderBuffer::new();
    let mut currently_rendering = RenderBuffer::new();

    loop {
        currently_rendering.clear();
        game.lock().unwrap().render_to_buf(&mut currently_rendering);
        let to_send = currently_rendering.get_updates_as_ansi_codes(&mut last_rendered);
        /*if let Err(e) = writer.write_all(to_send.as_bytes()).await {
            println!("send error: {}", e);
            return;
        }*/
        writer.write_all(to_send.as_bytes()).await.unwrap();
        currently_rendering.copy_into(&mut last_rendered);
        need_render_receiver.changed().await.unwrap();
    }
}

struct Connection {
    ip: IpAddr,
}

impl Connection {
    fn new(ip: IpAddr) -> Connection {
        println!("Connection from {}", ip);
        Connection { ip }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        println!("Disconnected: {}", self.ip);
    }
}

async fn handle_connection(
    socket: TcpStream,
    ip: IpAddr,
    need_render_receiver: watch::Receiver<()>,
    game: Arc<Mutex<game_logic::Game>>,
) {
    let _c = Connection::new(ip);
    let (reader, writer) = socket.into_split();
    tokio::select! {
        _ = handle_receiving(reader) => {},
        _ = handle_sending(writer, need_render_receiver, game) => {},
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
