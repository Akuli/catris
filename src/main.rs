#[macro_use(lazy_static)]
extern crate lazy_static;

use crate::client::Client;
use crate::client::ClientLogger;
use crate::connection::initialize_connection;
use crate::connection::Sender;
use crate::render::RenderBuffer;
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
mod blocks;
mod client;
mod connection;
mod game_logic;
mod game_wrapper;
mod high_scores;
mod ingame_ui;
mod lobby;
mod player;
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
        .log(&format!("Name asking done: {}", client.get_name()));

    let want_new_lobby = views::ask_if_new_lobby(&mut client).await?;
    if want_new_lobby {
        client.make_lobby(lobbies);
    } else {
        views::ask_lobby_id_and_join_lobby(&mut client, lobbies).await?;
    }

    let mut selected_index = 0;
    loop {
        let game_mode = views::choose_game_mode(&mut client, &mut selected_index).await?;
        match game_mode {
            Some(mode) => views::play_game(&mut client, mode).await?,
            None => views::show_gameplay_tips(&mut client).await?,
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

struct DecrementClientCoundOnDrop {
    client_counter: Arc<AtomicU64>,
    logger: ClientLogger,
}
impl Drop for DecrementClientCoundOnDrop {
    fn drop(&mut self) {
        let old_value = self.client_counter.fetch_sub(1, Ordering::SeqCst);
        self.logger.log(&format!(
            "There are now {} connected clients",
            old_value - 1
        ));
    }
}

static CLIENT_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub async fn handle_connection(
    socket: TcpStream,
    ip: IpAddr,
    lobbies: lobby::Lobbies,
    used_names: Arc<Mutex<HashSet<String>>>,
    recent_ips: Arc<Mutex<VecDeque<(Instant, IpAddr)>>>,
    client_counter: Arc<AtomicU64>,
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

    // client counter decrements when client quits, id counter does not
    // TODO: is fetch_add() really needed, we only need to add not fetch?
    client_counter.fetch_add(1, Ordering::SeqCst);
    let _decrementer = DecrementClientCoundOnDrop {
        client_counter,
        logger,
    };

    // TODO: max concurrent connections from same ip?
    log_ip_if_connects_a_lot(logger, ip, recent_ips);

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
    let client_counter = Arc::new(AtomicU64::new(0));

    let raw_listener = TcpListener::bind("0.0.0.0:12345").await.unwrap();
    println!("Listening for raw TCP connections on port 12345...");

    let ws_listener = TcpListener::bind("0.0.0.0:54321").await.unwrap();
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
                    client_counter.clone(),
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
                    client_counter.clone(),
                    true,
                ));
            }
        }
    }
}
