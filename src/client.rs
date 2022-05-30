use std::cmp::min;
use std::collections::HashSet;
use std::collections::VecDeque;
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
    pub fn log(&self, message: &str) {
        println!("[client {}] {}", self.client_id, message);
    }
}

pub struct Client {
    ip: IpAddr,
    id: u64,
    pub render_data: Arc<Mutex<render::RenderData>>,
    recv_buffer: [u8; 100], // keep small, receiving a single key press is O(recv buffer size)
    recv_buffer_size: usize,
    key_press_times: VecDeque<Instant>,
    reader: OwnedReadHalf,
    lobby: Option<Arc<Mutex<lobby::Lobby>>>,
    remove_name_on_disconnect_data: Option<(String, Arc<Mutex<HashSet<String>>>)>,
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

impl Client {
    pub fn new(ip: IpAddr, reader: OwnedReadHalf) -> Client {
        Client {
            ip: ip,
            // https://stackoverflow.com/a/32936288
            id: ID_COUNTER.fetch_add(1, Ordering::SeqCst),
            render_data: Arc::new(Mutex::new(render::RenderData {
                buffer: render::Buffer::new(),
                cursor_pos: None,
                changed: Arc::new(Notify::new()),
            })),
            recv_buffer: [0 as u8; 100],
            recv_buffer_size: 0,
            key_press_times: VecDeque::new(),
            reader: reader,
            lobby: None,
            remove_name_on_disconnect_data: None,
        }
    }

    pub fn logger(&self) -> ClientLogger {
        ClientLogger { client_id: self.id }
    }

    pub fn get_name(&self) -> &str {
        let (name, _) = self.remove_name_on_disconnect_data.as_ref().unwrap();
        name
    }

    // returns false if name is in use already
    pub fn set_name(&mut self, name: &str, used_names: Arc<Mutex<HashSet<String>>>) -> bool {
        {
            let mut used_names = used_names.lock().unwrap();
            if used_names.contains(name) {
                return false;
            }
            used_names.insert(name.to_string());
        }

        assert!(self.remove_name_on_disconnect_data.is_none());
        self.remove_name_on_disconnect_data = Some((name.to_string(), used_names));
        true
    }

    fn check_key_press_frequency(&mut self) -> Result<(), io::Error> {
        self.key_press_times.push_back(Instant::now());
        while self.key_press_times.len() != 0
            && self.key_press_times[0].elapsed().as_secs_f32() > 1.0
        {
            self.key_press_times.pop_front();
        }
        if self.key_press_times.len() > 100 {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "received more than 100 key presses / sec",
            ));
        }
        Ok(())
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
                    self.check_key_press_frequency()?;
                    for i in bytes_used..self.recv_buffer_size {
                        self.recv_buffer[i - bytes_used] = self.recv_buffer[i];
                    }
                    self.recv_buffer_size -= bytes_used;
                    return Ok(key);
                }
                None => {
                    // Receive more data
                    let read_target = &mut self.recv_buffer[self.recv_buffer_size..];
                    let n = timeout(Duration::from_secs(10 * 60), self.reader.read(read_target))
                        .await??;
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
        let id = lobby::generate_unused_id(&*lobbies);
        let mut lobby = lobby::Lobby::new(&id);
        self.logger().log(&format!("Created lobby: {}", id));
        lobby.add_client(self.logger(), "John", self.render_data.clone());

        let lobby = Arc::new(Mutex::new(lobby));
        lobbies.insert(id, lobby.clone());

        assert!(self.lobby.is_none());
        self.lobby = Some(lobby);
    }

    pub fn join_lobby(&mut self, lobby: Arc<Mutex<lobby::Lobby>>) {
        lobby
            .lock()
            .unwrap()
            .add_client(self.logger(), "John", self.render_data.clone());
        assert!(self.lobby.is_none());
        self.lobby = Some(lobby);
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(lobby) = &self.lobby {
            lobby.lock().unwrap().remove_client(self.id);
        }
        if let Some((name, name_set)) = &self.remove_name_on_disconnect_data {
            name_set.lock().unwrap().remove(name);
        }
    }
}
