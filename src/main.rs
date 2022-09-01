#[macro_use(lazy_static)]
extern crate lazy_static;

use crate::client::Client;
use crate::client::ClientLogger;
use crate::connection::initialize_connection;
use crate::connection::Sender;
use crate::ip_tracker::websocket_connections_come_from_a_proxy;
use crate::ip_tracker::IpTracker;
use crate::render::RenderBuffer;
use std::collections::HashSet;
use std::io;
use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
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
mod ip_tracker;
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

static CLIENT_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub async fn handle_connection(
    socket: TcpStream,
    source_ip: IpAddr,
    lobbies: lobby::Lobbies,
    used_names: Arc<Mutex<HashSet<String>>>,
    ip_tracker: Arc<Mutex<IpTracker>>,
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

    let error: io::Error =
        match initialize_connection(ip_tracker, logger, socket, source_ip, is_websocket).await {
            Ok((mut sender, receiver, _decrementer)) => {
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
    let ip_tracker = Arc::new(Mutex::new(IpTracker::new()));

    let raw_listener = TcpListener::bind("0.0.0.0:12345").await.unwrap();
    println!("Listening for raw TCP connections on port 12345...");

    let ws_listener;
    if websocket_connections_come_from_a_proxy() {
        // In production, avoid unnecessary non-localhost listening.
        ws_listener = TcpListener::bind("127.0.0.1:54321").await.unwrap();
        println!("Listening for websocket connections on port 54321 (only from localhost)...");
    } else {
        // Allow connections from anywhere. Needed for local-playing.md
        ws_listener = TcpListener::bind("0.0.0.0:54321").await.unwrap();
        println!("Listening for websocket connections on port 54321 (only from localhost)...");
    }

    loop {
        tokio::select! {
            result = raw_listener.accept() => {
                let (socket, sockaddr) = result.unwrap();
                tokio::spawn(handle_connection(
                    socket,
                    sockaddr.ip(),
                    lobbies.clone(),
                    used_names.clone(),
                    ip_tracker.clone(),
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
                    ip_tracker.clone(),
                    true,
                ));
            }
        }
    }
}
