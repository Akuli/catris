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

mod ansi;
mod client;
mod game_logic;
mod lobby;
mod render;
mod views;

async fn handle_receiving(
    mut client: client::Client,
    lobbies: lobby::Lobbies,
) -> Result<(), io::Error> {
    let view = Arc::new(Mutex::new(views::TextEntryView::new(
        "Name: ".to_string(),
        vec![
            "a".to_string(),
            "b".to_string(),
            "".to_string(),
            "c".to_string(),
            "d".to_string(),
        ],
    )));
    *client.view.lock().unwrap() = view.clone();
    loop {
        client.need_render_notify.notify_one();
        let future = view.lock().unwrap().run(&mut client);
        let s = future.await?;
        println!("run() -> {}", s);
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
    let mut cursor_showing: Option<bool> = None; // None = unknown

    loop {
        println!("render");
        let cursor_pos;
        {
            let view = view.lock().unwrap();
            let view = view.lock().unwrap();
            view.render(&mut buffers[next_idx]);
            cursor_pos = view.get_cursor_pos();
        }

        let mut to_send = buffers[1 - next_idx].get_updates_as_ansi_codes(&buffers[next_idx]);
        match cursor_pos {
            None => {
                to_send.push_str(&ansi::move_cursor(0, buffers[next_idx].height - 1));
                if cursor_showing != Some(false) {
                    to_send.push_str(ansi::HIDE_CURSOR);
                    cursor_showing = Some(false);
                }
            }
            Some((x, y)) => {
                to_send.push_str(&ansi::move_cursor(x, y));
                if cursor_showing != Some(true) {
                    to_send.push_str(ansi::SHOW_CURSOR);
                    cursor_showing = Some(true);
                }
            }
        }

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
