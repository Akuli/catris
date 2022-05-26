use std::io::Write;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::sleep;

mod ansi;
mod game_logic;
mod render;

use crate::render::RenderBuffer;

async fn handle_connection(
    socket: &mut TcpStream,
    ip: IpAddr,
    need_render_receiver: &mut watch::Receiver<()>,
    game: Arc<Mutex<game_logic::Game>>,
) {
    println!("Connection from {}", ip);

    let mut last_rendered = RenderBuffer::new();
    let mut currently_rendering = RenderBuffer::new();

    loop {
        currently_rendering.clear();
        game.lock().unwrap().render_to_buf(&mut currently_rendering);
        let to_send = currently_rendering.get_updates_as_ansi_codes(&mut last_rendered);
        if let Err(e) = socket.write_all(to_send.as_bytes()).await {
            println!("Disconnected: {} {}", ip, e);
            return;
        }
        currently_rendering.copy_into(&mut last_rendered);
        need_render_receiver.changed().await.unwrap();
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
    let listener = TcpListener::bind("127.0.0.1:12345").await.unwrap();

    let block = game_logic::MovingBlock {
        center_x: 5,
        center_y: -1,
        relative_coords: vec![(0, 0), (0, -1), (-1, 0), (-1, -1)],
    };
    let player = game_logic::Player {
        name: "Foo".to_string(),
        block: block,
    };
    let game = Arc::new(Mutex::new(game_logic::Game {
        players: vec![player],
    }));

    let (need_render_sender, need_render_receiver) = watch::channel(());
    tokio::spawn(move_blocks_down_task(game.clone(), need_render_sender));

    loop {
        let (mut socket, sockaddr) = listener.accept().await.unwrap();
        {
            let game = game.clone();
            let mut need_render_receiver = need_render_receiver.clone();
            tokio::spawn(async move {
                handle_connection(&mut socket, sockaddr.ip(), &mut need_render_receiver, game)
                    .await;
            });
        }
    }
}
