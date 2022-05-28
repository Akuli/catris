use std::io;
use std::io::Write;
use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use tokio::sync::Notify;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::time::sleep;
use weak_table::WeakValueHashMap;

mod ansi;
mod client;
mod game_logic;
mod lobby;
mod render;
mod views;

async fn handle_receiving(mut client: client::Client, lobbies: lobby::Lobbies) -> Result<(), io::Error> {
    loop {
        match client.receive_key_press().await? {
            ansi::KeyPress::Character('n') => {
                client.make_lobby(lobbies.clone());
            }
            key => println!("{:?}", key),
        }
    }
}

async fn handle_sending(
    mut writer: OwnedWriteHalf,
    need_render_notify: Arc<Notify>,
    view: views::ViewRef,
) -> Result<(), io::Error> {
    // pseudo optimization: double buffering to prevent copying between buffers
    let mut buffers = [render::Buffer::new(), render::Buffer::new()];
    let mut next_idx = 0;

    loop {
        view.lock().unwrap().render(&mut buffers[next_idx]);
        let to_send = buffers[1 - next_idx].get_updates_as_ansi_codes(&buffers[next_idx]);
        writer.write_all(to_send.as_bytes()).await?;
        next_idx = 1 - next_idx;
        need_render_notify.notified().await;
    }
}

pub async fn handle_connection(socket: TcpStream, ip: IpAddr, lobbies: lobby::Lobbies) {
    let (reader, writer) = socket.into_split();
    let client = client::Client::new(ip, reader);
    let logger = client.logger();
    let view = client.view.clone();
    let notify = client.need_render_notify.clone();

    let error: Result<(), io::Error> = tokio::select! {
        e = handle_receiving(client, lobbies) => {e},
        e = handle_sending(writer, notify, view) => {e},
    };
    logger.log(format!("Disconnected: {:?}", error));
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:12345").await.unwrap();

    let lobbies: lobby::Lobbies = Arc::new(Mutex::new(WeakValueHashMap::new()));

    loop {
        let (socket, sockaddr) = listener.accept().await.unwrap();
        let lobbies = lobbies.clone();
        tokio::spawn(handle_connection(socket, sockaddr.ip(), lobbies.clone()));
    }
}
