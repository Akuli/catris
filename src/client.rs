use crate::ansi::KeyPress;
use crate::connection::Receiver;
use crate::lobby;
use crate::lobby::Lobbies;
use crate::lobby::Lobby;
use crate::render::RenderBuffer;
use crate::render::RenderData;
use std::collections::HashSet;
use std::io;
use std::io::ErrorKind;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::Notify;

// Even though you can create only one Client, it can be associated with multiple ClientLoggers
#[derive(Copy, Clone)]
pub struct ClientLogger {
    pub client_id: u64,
}
impl ClientLogger {
    pub fn log(&self, message: &str) {
        println!("[client {}] {}", self.client_id, message);
    }
}

pub struct Client {
    pub id: u64,
    pub render_data: Arc<Mutex<RenderData>>,
    receiver: Receiver,
    pub lobby: Option<Arc<Mutex<Lobby>>>,
    pub lobby_id_hidden: bool,
    pub prefer_rotating_counter_clockwise: bool,
    remove_name_on_disconnect_data: Option<(String, Arc<Mutex<HashSet<String>>>)>,
}
impl Client {
    pub fn new(id: u64, receiver: Receiver) -> Client {
        Client {
            id,
            render_data: Arc::new(Mutex::new(RenderData {
                buffer: RenderBuffer::new(),
                cursor_pos: None,
                changed: Arc::new(Notify::new()),
                force_redraw: false,
            })),
            receiver,
            lobby: None,
            lobby_id_hidden: false,
            prefer_rotating_counter_clockwise: false,
            remove_name_on_disconnect_data: None,
        }
    }

    pub fn is_connected_with_websocket(&self) -> bool {
        match self.receiver {
            Receiver::WebSocket { .. } => true,
            Receiver::RawTcp { .. } => false,
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

    pub async fn receive_key_press(&mut self) -> Result<KeyPress, io::Error> {
        loop {
            match self.receiver.receive_key_press().await? {
                KeyPress::Quit => {
                    return Err(io::Error::new(
                        ErrorKind::ConnectionAborted,
                        "received quit key press",
                    ));
                }
                KeyPress::RefreshRequest => {
                    let mut render_data = self.render_data.lock().unwrap();
                    render_data.force_redraw = true;
                    render_data.changed.notify_one();
                }
                key => {
                    return Ok(key);
                }
            }
        }
    }

    pub fn make_lobby(&mut self, lobbies: Lobbies) {
        let mut lobbies = lobbies.lock().unwrap();
        let id = lobby::generate_unused_id(&*lobbies);
        let mut lobby = Lobby::new(&id);
        self.logger().log(&format!("Created lobby: {}", id));
        lobby.add_client(self.logger(), self.get_name());

        let lobby = Arc::new(Mutex::new(lobby));
        lobbies.insert(id, lobby.clone());

        assert!(self.lobby.is_none());
        self.lobby = Some(lobby);
    }

    pub fn join_lobby(&mut self, lobby: Arc<Mutex<Lobby>>) -> bool {
        {
            let mut lobby = lobby.lock().unwrap();
            if lobby.lobby_is_full() {
                return false;
            }
            lobby.add_client(self.logger(), self.get_name());
        }
        assert!(self.lobby.is_none());
        self.lobby = Some(lobby);
        true
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
