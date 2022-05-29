use rand;
use rand::Rng;
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
use crate::client;
use crate::game_logic;
use crate::render;
use crate::views;

struct ClientInfo {
    client_id: u64,
    logger: client::ClientLogger,
    name: String,
    color: u8,
    render_data: Arc<Mutex<render::RenderData>>,
}

pub struct Lobby {
    pub id: String,
    clients: Vec<ClientInfo>,
}

pub const MAX_CLIENTS_PER_LOBBY: usize = 6;
const ALL_COLORS: [u8; 6] = [31, 32, 33, 34, 35, 36];

/*
I started with A-Z0-9 and removed chars that look confusingly similar
in small font:

  A and 4
  B and 8
  C and G and 6
  E and F
  I and 1
  O and 0 and Q
  S and 5
  U and V
  Z and 2
*/
const ID_ALPHABET: [char; 16] = [
    'D', 'H', 'J', 'K', 'L', 'M', 'N', 'P', 'R', 'T', 'W', 'X', 'Y', '3', '7', '9',
];

impl Lobby {
    pub fn new(existing_lobbies: &WeakValueHashMap<String, Weak<Mutex<Lobby>>>) -> Lobby {
        loop {
            let id = (0..6)
                .into_iter()
                .map(|_| ID_ALPHABET[rand::thread_rng().gen_range(0..ID_ALPHABET.len())])
                .collect::<String>();
            if !existing_lobbies.contains_key(&id) {
                return Lobby {
                    id: id,
                    clients: vec![],
                };
            }
        }
    }

    pub fn add_client(
        &mut self,
        logger: client::ClientLogger,
        name: String,
        render_data: Arc<Mutex<render::RenderData>>,
    ) {
        let used_colors: Vec<u8> = self.clients.iter().map(|c| c.color).collect();
        let unused_color = *ALL_COLORS
            .iter()
            .filter(|color| !used_colors.contains(*color))
            .next()
            .unwrap();
        self.clients.push(ClientInfo {
            client_id: logger.client_id,
            logger: logger,
            name: name,
            color: unused_color,
            render_data: render_data,
        });
    }

    pub fn remove_client(&mut self, client_id: u64) {
        let i = self
            .clients
            .iter()
            .position(|c| c.client_id == client_id)
            .unwrap();
        self.clients[i]
            .logger
            .log(format!("Leaving lobby: {}", self.id));
        self.clients.remove(i);
    }
}

// TODO: remove this eventually once i trust that it works
impl Drop for Lobby {
    fn drop(&mut self) {
        println!("Destroying lobby: {}", self.id);
    }
}

pub type Lobbies = Arc<Mutex<WeakValueHashMap<String, Weak<Mutex<Lobby>>>>>;
