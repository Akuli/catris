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
mod connection;
mod game_logic;
mod render;

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
                connection::handle_connection(socket, sockaddr.ip(), need_render_receiver, game)
                    .await;
            });
        }
    }
}
