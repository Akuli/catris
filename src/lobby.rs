use crate::client;
use crate::client::ClientLogger;
use crate::game::Game;
use crate::game::Mode;
use crate::game_wrapper;
use crate::game_wrapper::GameWrapper;
use rand;
use rand::Rng;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use tokio;
use tokio::sync::watch;
use weak_table::WeakValueHashMap;

pub struct ClientInfo {
    pub client_id: u64,
    pub logger: ClientLogger,
    pub name: String,
    pub color: u8,
}

pub struct Lobby {
    pub id: String,
    pub clients: Vec<ClientInfo>,
    // change triggers when people join/leave the lobby or a game, and ui must refresh
    changed_sender: watch::Sender<()>,
    pub changed_receiver: watch::Receiver<()>,
    game_wrappers: WeakValueHashMap<Mode, Weak<GameWrapper>>,
}

pub const MAX_CLIENTS_PER_LOBBY: usize = 6;
const ALL_COLORS: [u8; MAX_CLIENTS_PER_LOBBY] = [31, 32, 33, 34, 35, 36];

impl Lobby {
    pub fn new(id: &str) -> Lobby {
        let (sender, receiver) = watch::channel(());
        Lobby {
            id: id.to_string(),
            clients: vec![],
            changed_sender: sender,
            changed_receiver: receiver,
            game_wrappers: WeakValueHashMap::new(),
        }
    }

    pub fn get_player_count(&self, mode: Mode) -> usize {
        match self.game_wrappers.get(&mode) {
            Some(wrapper) => {
                let n = wrapper.game.lock().unwrap().get_player_count();
                assert!(n > 0);
                n
            }
            None => 0,
        }
    }

    pub fn lobby_is_full(&self) -> bool {
        self.clients.len() == MAX_CLIENTS_PER_LOBBY
    }

    pub fn game_is_full(&self, mode: Mode) -> bool {
        self.get_player_count(mode) == mode.max_players()
    }

    fn mark_changed(&self) {
        self.changed_sender.send(()).unwrap();
    }

    pub fn add_client(&mut self, logger: client::ClientLogger, name: &str) {
        assert!(!self.lobby_is_full());
        logger.log(&format!(
            "Joining lobby with {} existing clients: {}",
            self.clients.len(),
            self.id
        ));
        let used_colors: Vec<u8> = self.clients.iter().map(|c| c.color).collect();
        let unused_color = *ALL_COLORS
            .iter()
            .filter(|color| !used_colors.contains(*color))
            .next()
            .unwrap();
        self.clients.push(ClientInfo {
            client_id: logger.client_id,
            logger,
            name: name.to_string(),
            color: unused_color,
        });
        self.mark_changed();
    }

    pub fn remove_client(&mut self, client_id: u64) {
        for wrapper in self.game_wrappers.values() {
            wrapper
                .game
                .lock()
                .unwrap()
                .remove_player_if_exists(client_id);
            wrapper.mark_changed();
        }

        let i = self
            .clients
            .iter()
            .position(|c| c.client_id == client_id)
            .unwrap();
        self.clients[i]
            .logger
            .log(&format!("Leaving lobby: {}", self.id));
        self.clients.remove(i);
        self.mark_changed();
    }

    pub fn join_game(&mut self, client_id: u64, mode: Mode) -> Arc<GameWrapper> {
        let client_info = self
            .clients
            .iter()
            .find(|info| info.client_id == client_id)
            .unwrap();

        let wrapper = if let Some(wrapper) = self.game_wrappers.get(&mode) {
            wrapper.game.lock().unwrap().add_player(&client_info);
            wrapper.mark_changed();
            wrapper
        } else {
            let mut game = Game::new(mode);
            game.add_player(&client_info);
            let wrapper = Arc::new(GameWrapper::new(game));
            tokio::spawn(game_wrapper::move_blocks_down(
                Arc::downgrade(&wrapper),
                true,
            ));
            tokio::spawn(game_wrapper::move_blocks_down(
                Arc::downgrade(&wrapper),
                false,
            ));
            self.game_wrappers.insert(mode, wrapper.clone());
            wrapper
        };

        self.mark_changed();
        wrapper
    }
}

// TODO: remove this eventually once i trust that it works
impl Drop for Lobby {
    fn drop(&mut self) {
        println!("[lobby {}] Destroying lobby", self.id);
    }
}

pub type Lobbies = Arc<Mutex<WeakValueHashMap<String, Weak<Mutex<Lobby>>>>>;

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

pub fn looks_like_lobby_id(string: &str) -> bool {
    return string.len() == 6 && string.chars().all(|ch| ID_ALPHABET.contains(&ch));
}

pub fn generate_unused_id(
    existing_lobbies: &WeakValueHashMap<String, Weak<Mutex<Lobby>>>,
) -> String {
    loop {
        let id = (0..6)
            .into_iter()
            .map(|_| ID_ALPHABET[rand::thread_rng().gen_range(0..ID_ALPHABET.len())])
            .collect::<String>();
        if !existing_lobbies.contains_key(&id) {
            return id;
        }
    }
}
