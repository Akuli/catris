use std::cmp::min;
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

use crate::ansi;
use crate::game_logic;
use crate::lobby;
use crate::render;
use crate::views;

// Even though you can create only one Client, it can be associated with multiple ClientLoggers
pub struct ClientLogger {
    pub client_id: u64,
}
impl ClientLogger {
    pub fn log(&self, message: String) {
        println!("[client {}] {}", self.client_id, message);
    }
}

struct Client {
    ip: IpAddr,
    id: u64,
    pub need_render_notify: Arc<Notify>,
    pub view: views::ViewRef,
    recv_buffer: [u8; 100], // keep small, receiving a single key press is O(recv buffer size)
    recv_buffer_size: usize,
    reader: OwnedReadHalf,
    lobby: Option<Arc<Mutex<lobby::Lobby>>>,
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

impl Client {
    fn new(ip: IpAddr, reader: OwnedReadHalf) -> Client {
        let result = Client {
            ip: ip,
            // https://stackoverflow.com/a/32936288
            id: ID_COUNTER.fetch_add(1, Ordering::SeqCst),
            need_render_notify: Arc::new(Notify::new()),
            view: Arc::new(Mutex::new(views::DummyView {})),
            recv_buffer: [0 as u8; 100],
            recv_buffer_size: 0,
            reader: reader,
            lobby: None,
        };
        // TODO: don't log all IPs
        result.logger().log(format!("New connection from {}", ip));
        result
    }

    pub fn logger(&self) -> ClientLogger {
        ClientLogger { client_id: self.id }
    }

    pub async fn receive_key_press(&mut self) -> Result<ansi::KeyPress, io::Error> {
        loop {
            match ansi::parse_key_press(&self.recv_buffer[..self.recv_buffer_size]) {
                Some((ansi::KeyPress::Quit, _)) => {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "received quit key press",
                    ));
                }
                Some((key, bytes_used)) => {
                    for i in bytes_used..self.recv_buffer_size {
                        self.recv_buffer[i - bytes_used] = self.recv_buffer[i];
                    }
                    self.recv_buffer_size -= bytes_used;
                    return Ok(key);
                }
                None => {
                    // Receive more data
                    let n = self
                        .reader
                        .read(&mut self.recv_buffer[self.recv_buffer_size..])
                        .await?;
                    if n == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "connection closed",
                        ));
                    }
                    self.recv_buffer_size += n;
                }
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(lobby) = &self.lobby {
            lobby.lock().unwrap().remove_client(self.id);
        }
    }
}

async fn handle_receiving(mut client: Client, lobbies: lobby::Lobbies) -> Result<(), io::Error> {
    loop {
        match client.receive_key_press().await? {
            ansi::KeyPress::Character('n') => {
                let mut lobbies = lobbies.lock().unwrap();
                let mut lobby = lobby::Lobby::new(&*lobbies);
                let id = lobby.id.clone();
                client.logger().log(format!("Created lobby: {}", id));
                lobby.add_client(
                    client.logger(),
                    "John".to_string(),
                    client.need_render_notify.clone(),
                    client.view.clone(),
                );
                let lobby = Arc::new(Mutex::new(lobby));
                lobbies.insert(id, lobby.clone());
                client.lobby = Some(lobby);
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
    let mut buffers = [render::RenderBuffer::new(), render::RenderBuffer::new()];
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
    let client = Client::new(ip, reader);
    let logger = client.logger();
    let view = client.view.clone();
    let notify = client.need_render_notify.clone();

    let error: Result<(), io::Error> = tokio::select! {
        e = handle_receiving(client, lobbies) => {e},
        e = handle_sending(writer, notify, view) => {e},
    };
    logger.log(format!("Disconnected: {:?}", error));
}
