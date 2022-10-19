#[macro_use(lazy_static)]
extern crate lazy_static;

use crate::client::Client;
use crate::client::ClientLogger;
use crate::connection::get_websocket_proxy_ip;
use crate::connection::initialize_connection;
use crate::connection::Receiver;
use crate::connection::Sender;
use crate::escapes::KeyPress;
use crate::escapes::TerminalType;
use crate::ip_tracker::IpTracker;
use crate::render::RenderBuffer;
use std::collections::HashSet;
use std::io;
use std::io::ErrorKind;
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

mod client;
mod connection;
mod escapes;
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
    terminal_type: TerminalType,
) -> Result<(), io::Error> {
    let mut last_render = RenderBuffer::new(terminal_type);
    let mut current_render = RenderBuffer::new(terminal_type); // Please get rid of this if copying turns out to be slow
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
                current_render.get_updates_as_escape_codes(&last_render, cursor_pos, force_redraw);
            sender.send(to_send.as_bytes()).await?;
            current_render.copy_into(&mut last_render);
        }
    }
}

pub async fn detect_terminal_type(
    sender: &mut Sender,
    receiver: &mut Receiver,
) -> Result<TerminalType, io::Error> {
    let message = concat!(
        "\r\n",
        "Detecting the type of your terminal. If it doesn't happen automatically:\r\n",
        "\r\n",
        "  * If you use e.g. Linux or Mac, make sure you ran \"stty raw\"\r\n",
        "    before connecting.\r\n",
        "  * Press a if your terminal supports ANSI escape sequences.\r\n",
        "  * Press v if you use a VT52 compatible terminal.\r\n",
        "  * Create an issue at https://github.com/Akuli/catris/ if you have\r\n",
        "    trouble connecting.\r\n",
        "\r\n",
        // Send DSR (Device Status Report, aka query cursor location) for ansi terminals.
        // Send ident (aka identify terminal type) for VT52 terminals.
        // Both types of terminals respond without user input.
        "\x1b[6n\x1bZ",
    );
    sender.send(message.as_bytes()).await?;

    match receiver.receive_key_press().await? {
        KeyPress::Character('a') => return Ok(TerminalType::ANSI),
        KeyPress::Character('v') => return Ok(TerminalType::VT52),
        KeyPress::Character('\x1b') => {
            // Escape character, probably in response to ANSI DSR or VT52 ident
            match receiver.receive_key_press().await? {
                KeyPress::Character('/') => {
                    // VT5* ident. Next character distinguishes, VT50, VT52 etc
                    if matches!(
                        receiver.receive_key_press().await?,
                        KeyPress::Character('K')
                            | KeyPress::Character('L')
                            | KeyPress::Character('Z')
                    ) {
                        return Ok(TerminalType::VT52);
                    }
                }
                KeyPress::Character('[') => {
                    // ANSI terminal. Response to DSR ends with letter R.
                    // Length limit so client can't send a lot of garbage to consume server CPU.
                    let mut n = 0;
                    while n < 10 {
                        if matches!(
                            receiver.receive_key_press().await?,
                            KeyPress::Character('R')
                        ) {
                            return Ok(TerminalType::ANSI);
                        }
                        n += 1;
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }

    return Err(io::Error::new(
        ErrorKind::ConnectionAborted,
        "unable to detect terminal type",
    ));
}

async fn handle_connection_until_error(
    logger: ClientLogger,
    socket: TcpStream,
    source_ip: IpAddr,
    lobbies: lobby::Lobbies,
    used_names: Arc<Mutex<HashSet<String>>>,
    ip_tracker: Arc<Mutex<IpTracker>>,
    is_websocket: bool,
) -> Result<(), io::Error> {
    let (mut sender, mut receiver, _decrementer) =
        initialize_connection(ip_tracker, logger, socket, source_ip, is_websocket).await?;

    let terminal_type = timeout(
        Duration::from_secs(20),
        detect_terminal_type(&mut sender, &mut receiver),
    )
    .await??;
    logger.log(&format!("Terminal type detected: {:?}", terminal_type));

    let client = Client::new(logger.client_id, receiver, terminal_type);
    let render_data = client.render_data.clone();

    let result = tokio::select! {
        res = handle_receiving(client, lobbies, used_names) => res,
        res = handle_sending(&mut sender, render_data, terminal_type) => res,
    };

    // Try to leave the terminal in a sane state
    let cleanup = terminal_type.show_cursor().to_string()
        + terminal_type.move_cursor_to_leftmost_column()
        + terminal_type.clear_from_cursor_to_end_of_screen();
    _ = timeout(Duration::from_millis(500), sender.send(cleanup.as_bytes())).await??;

    assert!(result.is_err());
    result
}

static CLIENT_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

async fn handle_connection(
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

    let error = handle_connection_until_error(
        logger,
        socket,
        source_ip,
        lobbies,
        used_names,
        ip_tracker,
        is_websocket,
    )
    .await
    .unwrap_err();
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
    if let Some(proxy_ip) = get_websocket_proxy_ip() {
        // In production, avoid unnecessary listening.
        ws_listener = TcpListener::bind((proxy_ip, 54321)).await.unwrap();
        println!(
            "Listening for websocket connections on port 54321 (only from {})...",
            proxy_ip
        );
    } else {
        // Allow connections from anywhere. Needed for local-playing.md
        ws_listener = TcpListener::bind("0.0.0.0:54321").await.unwrap();
        println!("Listening for websocket connections on port 54321...");
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
