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
        {
            let game = game.lock().unwrap();
            game.render_to_buf(&mut currently_rendering);
        }
        // TODO: socket error handling
        socket
            .write(
                currently_rendering
                    .get_updates_as_ansi_codes(&mut last_rendered)
                    .as_bytes(),
            )
            .await
            .unwrap();
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

    // TODO: possible to clone the receiver we get from here?
    let (need_render_sender, mut need_render_receiver) = watch::channel(());
    tokio::spawn(move_blocks_down_task(game.clone(), need_render_sender));

    let (mut socket, sockaddr) = listener.accept().await.unwrap();
    tokio::spawn(async move {
        handle_connection(
            &mut socket,
            sockaddr.ip(),
            &mut need_render_receiver,
            game.clone(),
        )
        .await;
    });
    loop {
        sleep(Duration::from_millis(400)).await;
    }
}
