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
    let s = views::prompt(&mut client, "Name: ".to_string(), vec![]).await?;
    println!("run() -> {}", s);
    Ok(())
}

async fn handle_sending(
    mut writer: OwnedWriteHalf,
    render_data: Arc<Mutex<render::RenderData>>,
) -> Result<(), io::Error> {
    // pseudo optimization: double buffering to prevent copying between buffers
    let mut buffers = [render::Buffer::new(), render::Buffer::new()];
    let mut next_idx = 0;
    let mut cursor_showing: Option<bool> = None; // None = unknown

    loop {
        let cursor_pos;
        {
            let render_data = render_data.lock().unwrap();
            render_data.buffer.copy_into(&mut buffers[next_idx]);
            cursor_pos = render_data.cursor_pos;
        }

        // In the beginning of a connection, the buffer isn't ready yet
        if buffers[next_idx].width != 0 && buffers[next_idx].height != 0 {
            let mut to_send =
                buffers[next_idx].get_updates_as_ansi_codes(&buffers[1 - next_idx], cursor_pos);
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
        }
        next_idx = 1 - next_idx;
        let change_notify = render_data.lock().unwrap().changed.clone();
        change_notify.notified().await;
    }
}

pub async fn handle_connection(socket: TcpStream, ip: IpAddr, lobbies: lobby::Lobbies) {
    let (reader, writer) = socket.into_split();
    let client = client::Client::new(ip, reader);
    let logger = client.logger();
    let render_data = client.render_data.clone();

    let error: Result<(), io::Error> = tokio::select! {
        e = handle_receiving(client, lobbies) => e,
        e = handle_sending(writer, render_data) => e,
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
