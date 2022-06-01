use std::collections::HashSet;
use std::collections::VecDeque;
use std::io;
use std::io::Write;
use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio::time::sleep;
use tokio::time::timeout;
use weak_table::WeakValueHashMap;

mod ansi;
mod client;
mod lobby;
mod logic_base;
mod modes;
mod render;
mod views;

async fn handle_receiving(
    mut client: client::Client,
    lobbies: lobby::Lobbies,
    used_names: Arc<Mutex<HashSet<String>>>,
) -> Result<(), io::Error> {
    views::ask_name(&mut client, used_names.clone()).await?;
    client
        .logger()
        .log(&format!("Name asking done: {}", client.get_name()));

    let want_new_lobby = views::ask_if_new_lobby(&mut client).await?;
    if want_new_lobby {
        client.make_lobby(lobbies);
    } else {
        views::ask_lobby_id_and_join_lobby(&mut client, lobbies).await?;
    }

    let mut selected_index = 0 as usize;
    loop {
        let game_mode = views::choose_game_mode(&mut client, &mut selected_index).await?;
        match game_mode {
            Some(mode) => views::play_game(&mut client, mode).await?,
            None => views::show_gameplay_tips(&mut client).await?,
        }
    }
}

async fn handle_sending(
    writer: &mut OwnedWriteHalf,
    render_data: Arc<Mutex<render::RenderData>>,
) -> Result<(), io::Error> {
    // pseudo optimization: double buffering to prevent copying between buffers
    let mut buffers = [render::Buffer::new(), render::Buffer::new()];
    let mut next_idx = 0;

    loop {
        let cursor_pos;
        {
            let render_data = render_data.lock().unwrap();
            render_data.buffer.copy_into(&mut buffers[next_idx]);
            cursor_pos = render_data.cursor_pos;
        }

        // In the beginning of a connection, the buffer isn't ready yet
        if buffers[next_idx].width != 0 && buffers[next_idx].height != 0 {
            let to_send =
                buffers[next_idx].get_updates_as_ansi_codes(&buffers[1 - next_idx], cursor_pos);
            writer.write_all(to_send.as_bytes()).await?;
        }

        next_idx = 1 - next_idx;
        let change_notify = render_data.lock().unwrap().changed.clone();
        change_notify.notified().await;
    }
}

fn log_ip_if_connects_a_lot(
    logger: &client::ClientLogger,
    ip: IpAddr,
    recent_ips: Arc<Mutex<VecDeque<(Instant, IpAddr)>>>,
) {
    let n;
    {
        let mut recent_ips = recent_ips.lock().unwrap();
        recent_ips.push_back((Instant::now(), ip));
        while recent_ips.len() != 0 && recent_ips[0].0.elapsed().as_secs_f32() > 60.0 {
            recent_ips.pop_front();
        }
        n = recent_ips
            .iter()
            .filter(|(_, recent_ip)| *recent_ip == ip)
            .count();
    }

    if n >= 5 {
        logger.log(&format!(
            "This is the {}th connection from IP address {} within the last minute",
            n, ip
        ));
    }
}

pub async fn handle_connection(
    socket: TcpStream,
    ip: IpAddr,
    lobbies: lobby::Lobbies,
    used_names: Arc<Mutex<HashSet<String>>>,
    recent_ips: Arc<Mutex<VecDeque<(Instant, IpAddr)>>>,
) {
    // TODO: max concurrent connections from same ip?
    let (reader, mut writer) = socket.into_split();

    let client = client::Client::new(ip, reader);
    let logger = client.logger();
    logger.log("New connection");
    log_ip_if_connects_a_lot(&logger, ip, recent_ips);
    let render_data = client.render_data.clone();

    let result: Result<(), io::Error> = tokio::select! {
        res = handle_receiving(client, lobbies, used_names) => res,
        res = handle_sending(&mut writer, render_data) => res,
    };

    // Try to leave the terminal in a sane state
    let cleanup_ansi_codes = ansi::SHOW_CURSOR.to_owned()
        + &ansi::move_cursor_horizontally(0)
        + ansi::CLEAR_FROM_CURSOR_TO_END_OF_SCREEN;
    _ = timeout(
        Duration::from_millis(500),
        writer.write_all(cleanup_ansi_codes.as_bytes()),
    )
    .await;

    logger.log(&format!("Disconnected: {}", result.unwrap_err()));
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:12345").await.unwrap();

    let used_names = Arc::new(Mutex::new(HashSet::new()));
    let lobbies: lobby::Lobbies = Arc::new(Mutex::new(WeakValueHashMap::new()));
    let recent_ips = Arc::new(Mutex::new(VecDeque::new()));

    loop {
        let (socket, sockaddr) = listener.accept().await.unwrap();
        let lobbies = lobbies.clone();
        tokio::spawn(handle_connection(
            socket,
            sockaddr.ip(),
            lobbies.clone(),
            used_names.clone(),
            recent_ips.clone(),
        ));
    }
}
