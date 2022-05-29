use std::cmp::min;
use std::io;
use std::io::Write;
use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
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

pub struct Client {
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
    pub fn new(ip: IpAddr, reader: OwnedReadHalf) -> Client {
        let result = Client {
            ip: ip,
            // https://stackoverflow.com/a/32936288
            id: ID_COUNTER.fetch_add(1, Ordering::SeqCst),
            need_render_notify: Arc::new(Notify::new()),
            view: Arc::new(Mutex::new(Arc::new(Mutex::new(views::DummyView {})))),
            recv_buffer: [0 as u8; 100],
            recv_buffer_size: 0,
            reader: reader,
            lobby: None,
        };
        // TODO: don't log all IPs
        result.logger().log(format!("New connection from {}", ip));
        result
    }

    pub fn set_view(&mut self, view: impl views::View + 'static) {
        *self.view.lock().unwrap() = Arc::new(Mutex::new(view));
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

    pub fn make_lobby(&mut self, lobbies: lobby::Lobbies) {
        let mut lobbies = lobbies.lock().unwrap();
        let mut lobby = lobby::Lobby::new(&*lobbies);
        let id = lobby.id.clone();
        self.logger().log(format!("Created lobby: {}", id));
        lobby.add_client(
            self.logger(),
            "John".to_string(),
            self.need_render_notify.clone(),
            self.view.clone(),
        );
        let lobby = Arc::new(Mutex::new(lobby));
        lobbies.insert(id, lobby.clone());
        self.lobby = Some(lobby);
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(lobby) = &self.lobby {
            lobby.lock().unwrap().remove_client(self.id);
        }
    }
}
