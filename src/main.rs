#[macro_use(lazy_static)]
extern crate lazy_static;

use crate::client::Client;
use crate::client::ClientLogger;
use crate::connection::initialize_connection;
use crate::connection::Sender;
use crate::render::RenderBuffer;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::io;
use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::time::timeout;
use weak_table::WeakValueHashMap;

mod ansi;
mod client;
mod connection;
mod game_logic;
mod game_wrapper;
mod high_scores;
mod ingame_ui;
mod lobby;
mod render;
mod views;

async fn handle_receiving(
    mut client: Client,
    lobbies: lobby::Lobbies,
    used_names: Arc<Mutex<HashSet<String>>>,
) -> Result<(), io::Error> {
    views::ask_name(&mut client, used_names).await?;
    client
        .logger()
        .log(&format!("Name asking done: {}", client.get_name().unwrap()));

    let want_new_lobby = views::ask_if_new_lobby(&mut client).await?;
    if want_new_lobby {
        client.make_lobby(lobbies);
    } else {
        views::ask_lobby_id_and_join_lobby(&mut client, lobbies).await?;
    }

    let mut selected_index = 0;
    loop {
        let game_mode = views::show_mode_menu(&mut client, &mut selected_index).await?;
        match game_mode {
            views::ModeMenuChoice::PlayGame(mode) => views::play_game(&mut client, mode).await?,
            views::ModeMenuChoice::GameplayTips => views::show_gameplay_tips(&mut client).await?,
            views::ModeMenuChoice::ShowAllHighScores => {
                views::show_all_high_scores(&mut client).await?
            }
        }
    }
}

async fn handle_sending(
    sender: &mut Sender,
    render_data: Arc<Mutex<render::RenderData>>,
) -> Result<(), io::Error> {
    let mut last_render = RenderBuffer::new();
    let mut current_render = RenderBuffer::new(); // Please get rid of this if copying turns out to be slow
    let change_notify = render_data.lock().unwrap().changed.clone();

    loop {
        change_notify.notified().await;

        let cursor_pos;
        let force_redraw;
        {
            let mut render_data = render_data.lock().unwrap();
            render_data.buffer.copy_into(&mut current_render);
            cursor_pos = render_data.cursor_pos;
            force_redraw = render_data.force_redraw;
            render_data.force_redraw = false;
        }

        // In the beginning of a connection, the buffer isn't ready yet
        if current_render.width != 0 && current_render.height != 0 {
            let to_send =
                current_render.get_updates_as_ansi_codes(&last_render, cursor_pos, force_redraw);
            sender.send(to_send.as_bytes()).await?;
            current_render.copy_into(&mut last_render);
        }
    }
}

fn log_ip_if_connects_a_lot(
    logger: ClientLogger,
    ip: IpAddr,
    recent_ips: Arc<Mutex<VecDeque<(Instant, IpAddr)>>>,
) {
    let n;
    {
        let mut recent_ips = recent_ips.lock().unwrap();
        recent_ips.push_back((Instant::now(), ip));
        while !recent_ips.is_empty() && recent_ips[0].0.elapsed().as_secs_f32() > 60.0 {
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

fn log_total(logger: ClientLogger, counts: &HashMap<IpAddr, usize>) {
    let total: usize = counts.values().sum();
    logger.log(&format!("There are now {} connected clients", total));
}

struct DecrementClientCoundOnDrop {
    logger: ClientLogger,
    client_counts_by_ip: Arc<Mutex<HashMap<IpAddr, usize>>>,
    ip: IpAddr,
}
impl Drop for DecrementClientCoundOnDrop {
    fn drop(&mut self) {
        let mut counts = self.client_counts_by_ip.lock().unwrap();
        let n = *counts.get(&self.ip).unwrap();
        assert!(n > 0);
        if n == 1 {
            // last client
            _ = counts.remove(&self.ip).unwrap();
        } else {
            counts.insert(self.ip, n - 1);
        }
        log_total(self.logger, &counts);
    }
}

static CLIENT_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub async fn handle_connection(
    socket: TcpStream,
    ip: IpAddr,
    lobbies: lobby::Lobbies,
    used_names: Arc<Mutex<HashSet<String>>>,
    recent_ips: Arc<Mutex<VecDeque<(Instant, IpAddr)>>>,
    client_counts_by_ip: Arc<Mutex<HashMap<IpAddr, usize>>>,
    is_websocket: bool,
) {
    // https://stackoverflow.com/a/32936288
    // not sure what ordering to use, so choosing the one with most niceness guarantees
    let client_id = CLIENT_ID_COUNTER.fetch_add(1, Ordering::SeqCst);

    let logger = ClientLogger { client_id };
    if is_websocket {
        logger.log("New websocket connection");
    } else {
        logger.log("New raw TCP connection");
    }

    log_ip_if_connects_a_lot(logger, ip, recent_ips);

    let _decrementer = {
        let mut counts = client_counts_by_ip.lock().unwrap();
        let old_count = *counts.get(&ip).unwrap_or(&0);
        if old_count >= 5 {
            logger.log(&format!("Closing connection because there are already {} other connections from the same IP", old_count));
            return;
        }

        counts.insert(ip, old_count + 1);
        let decrementer = DecrementClientCoundOnDrop {
            logger,
            client_counts_by_ip: client_counts_by_ip.clone(),
            ip,
        };
        log_total(logger, &counts);
        decrementer
    };

    let error: io::Error = match initialize_connection(socket, is_websocket).await {
        Ok((mut sender, receiver)) => {
            let client = Client::new(client_id, receiver);
            let render_data = client.render_data.clone();
            let error = tokio::select! {
                res = handle_receiving(client, lobbies, used_names) => res.unwrap_err(),
                res = handle_sending(&mut sender, render_data) => res.unwrap_err(),
            };

            // Try to leave the terminal in a sane state
            let cleanup_ansi_codes = ansi::SHOW_CURSOR.to_owned()
                + &ansi::move_cursor_horizontally(0)
                + ansi::CLEAR_FROM_CURSOR_TO_END_OF_SCREEN;
            _ = timeout(
                Duration::from_millis(500),
                sender.send(cleanup_ansi_codes.as_bytes()),
            )
            .await;

            error
        }
        Err(e) => e,
    };

    logger.log(&format!("Disconnected: {}", error));
}

#[tokio::main]
async fn main() {
    let used_names = Arc::new(Mutex::new(HashSet::new()));
    let lobbies: lobby::Lobbies = Arc::new(Mutex::new(WeakValueHashMap::new()));
    let recent_ips = Arc::new(Mutex::new(VecDeque::new()));
    let client_counts_by_ip = Arc::new(Mutex::new(HashMap::new()));

    // TODO: Add an option to listen on 0.0.0.0, for local-playing.md
    let raw_listener = TcpListener::bind("127.0.0.1:12345").await.unwrap();
    println!("Listening for raw TCP connections on port 12345...");

    let ws_listener = TcpListener::bind("127.0.0.1:54321").await.unwrap();
    println!("Listening for websocket connections on port 54321...");

    loop {
        tokio::select! {
            result = raw_listener.accept() => {
                let (socket, sockaddr) = result.unwrap();
                tokio::spawn(handle_connection(
                    socket,
                    sockaddr.ip(),
                    lobbies.clone(),
                    used_names.clone(),
                    recent_ips.clone(),
                    client_counts_by_ip.clone(),
                    false,
                ));
            }
            result = ws_listener.accept() => {
                let (socket, sockaddr) = result.unwrap();
                tokio::spawn(handle_connection(
                    socket,
                    sockaddr.ip(),
                    lobbies.clone(),
                    used_names.clone(),
                    recent_ips.clone(),
                    client_counts_by_ip.clone(),
                    true,
                ));
            }
        }
    }
}
