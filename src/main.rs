#[macro_use(lazy_static)]
extern crate lazy_static;

use crate::client::Client;
use crate::client::ClientLogger;
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
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::time::timeout;
use weak_table::WeakValueHashMap;

mod ansi;
mod blocks;
mod client;
mod game_logic;
mod game_wrapper;
mod high_scores;
mod ingame_ui;
mod lobby;
mod player;
mod render;
mod views;
/*

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
    let mut buffers = [RenderBuffer::new(), RenderBuffer::new()];
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
    logger: ClientLogger,
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

pub async fn handle_connection(
    socket: TcpStream,
    ip: IpAddr,
    lobbies: lobby::Lobbies,
    used_names: Arc<Mutex<HashSet<String>>>,
    recent_ips: Arc<Mutex<VecDeque<(Instant, IpAddr)>>>,
    client_counter: Arc<AtomicU64>,
) {
    // TODO: max concurrent connections from same ip?
    let (reader, mut writer) = socket.into_split();

    let client = Client::new(reader);
    let logger = client.logger();
    logger.log("New connection");

    // not sure what ordering to use, so choosing the one with most niceness guarantees
    client_counter.fetch_add(1, Ordering::SeqCst);
    let _decrementer = DecrementClientCoundOnDrop {
        client_counter,
        logger,
    };

    log_ip_if_connects_a_lot(logger, ip, recent_ips);
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
    let used_names = Arc::new(Mutex::new(HashSet::new()));
    let lobbies: lobby::Lobbies = Arc::new(Mutex::new(WeakValueHashMap::new()));
    let recent_ips = Arc::new(Mutex::new(VecDeque::new()));
    let client_counter = Arc::new(AtomicU64::new(0));

    let listener = TcpListener::bind("0.0.0.0:12345").await.unwrap();
    println!("Listening on port 12345...");

    loop {
        let (socket, sockaddr) = listener.accept().await.unwrap();
        let lobbies = lobbies.clone();
        tokio::spawn(handle_connection(
            socket,
            sockaddr.ip(),
            lobbies.clone(),
            used_names.clone(),
            recent_ips.clone(),
            client_counter.clone(),
        ));
    }
}
*/

use tokio_tungstenite::tungstenite::Message;

use std::error::Error;
use futures_util::SinkExt;
use futures_util::{future, StreamExt, TryStreamExt};
use futures_util::stream::SplitStream;
use tokio_tungstenite::WebSocketStream;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let listener = TcpListener::bind("127.0.0.1:54321").await?;
    println!("Listening");

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream));
    }

    Ok(())
}

enum Receiver {
    WebSocket{stream: SplitStream<WebSocketStream<TcpStream>>, pings: mpsc::Sender<Vec<u8>>},
    RawTcp{stream: TcpStream},
}
impl Receiver {
    async fn recv(&mut self, target: &mut [u8]) -> Result<usize, Box<dyn Error>> {
        match self {
            Self::WebSocket{stream, pings} =>{
                loop {
                    let item = stream.next().await;
                    if item.is_none() {
                        return Ok(0);  // connection closed
                    }
                    match item.unwrap()? {
                        Message::Binary(bytes) => {
                            // TODO: what if client send 1GB message of bytes at once?
                            // would be already fucked up, because Vec<u8> of 1GB was allocated
                            if bytes.len() > target.len() {
                                Err(format!("too long websocket message: {} > {}", bytes.len(), target.len()))?;
                            }
                            for i in 0..bytes.len() {
                                target[i] = bytes[i];
                            }
                            return Ok(bytes.len());
                        }
                        Message::Close(_) => {
                            return Ok(0);
                        }
                        Message::Text(bytes) => {
                            // everything should be done with Binary messages
                            Err("unexpected websocket text data received")?
                        }
                        Message::Ping(bytes) => {
                            // TODO: rate limit
                            pings.send(bytes).await?;
                        }
                        Message::Pong(_) => {
                            // we never send ping, so client should never send pong
                            Err("unexpected websocket pong")?
                        }
                        Message::Frame(_) => {
                            panic!("this is impossible according to docs");
                        }
                    }
                }
            }
            Self::RawTcp{..} => unimplemented!(),
        }
    }
}

async fn foo(mut receiver: Receiver) -> Result<(), Box<dyn Error>> {
    let mut buf = [0 as u8; 100];
    let n = receiver.recv(&mut buf).await?;
    println!("{} {:?}", n, buf);
    Ok(())
}

async fn accept_connection(stream: TcpStream) {
    let addr = stream.peer_addr().unwrap();
    println!("Peer address: {}", addr);

    let mut ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");

    println!("New WebSocket connection: {}", addr);

    let (mut ws_writer, mut ws_reader) = ws_stream.split();
    let (ping_sender, ping_receiver) = mpsc::channel(5);
    let mut receiver = Receiver::WebSocket{stream: ws_reader, pings: ping_sender};
    _ = ws_writer.send(Message::binary(vec![b'h',b'e',b'l',b'l',b'o'])).await;
    _ = foo(receiver).await;
}
