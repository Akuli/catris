use crate::client;
use crate::client::ClientLogger;
use crate::game_logic::Game;
use crate::game_logic::Mode;
use crate::game_wrapper;
use crate::game_wrapper::GameWrapper;
use rand;
use rand::Rng;
use std::collections::HashMap;
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
    // change triggers when people join/leave the lobby or a game.
    // Lobby UI shows how many players are in each game, that must refresh
    changed_sender: watch::Sender<()>,
    pub changed_receiver: watch::Receiver<()>,
    // games get deleted when players leave them
    game_wrappers: HashMap<Mode, Arc<GameWrapper>>,
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
            game_wrappers: HashMap::new(),
        }
    }

    pub fn get_player_count(&self, mode: Mode) -> usize {
        match self.game_wrappers.get(&mode) {
            Some(wrapper) => {
                let n = wrapper.game.lock().unwrap().players.len();
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

    pub fn mark_changed(&self) {
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

    fn join_game(&mut self, client_id: u64, mode: Mode) -> Option<Arc<GameWrapper>> {
        let client_info = self
            .clients
            .iter()
            .find(|info| info.client_id == client_id)
            .unwrap();

        let wrapper = if let Some(wrapper) = self.game_wrappers.get(&mode) {
            if !wrapper.game.lock().unwrap().add_player(client_info) {
                return None;
            }
            wrapper.mark_changed();
            wrapper.clone()
        } else {
            let mut game = Game::new(mode);
            let ok = game.add_player(&client_info);
            assert!(ok);
            let wrapper = Arc::new(GameWrapper::new(game));
            game_wrapper::start_tasks(wrapper.clone());
            self.game_wrappers.insert(mode, wrapper.clone());
            wrapper
        };

        self.mark_changed();
        Some(wrapper)
    }

    pub fn leave_game(&mut self, client_id: u64, mode: Mode) {
        let last_player_removed = if let Some(wrapper) = self.game_wrappers.get(&mode) {
            let mut game = wrapper.game.lock().unwrap();
            game.remove_player_if_exists(client_id);
            wrapper.mark_changed();
            game.players.is_empty()
        } else {
            false
        };

        if last_player_removed {
            self.game_wrappers.remove(&mode);
        }
        self.mark_changed();
    }

    pub fn remove_game(&mut self, mode: Mode) {
        self.game_wrappers.remove(&mode);
        self.mark_changed();
    }
}

// Removes client from lobby automatically when game ends
pub struct PlayingToken {
    client_id: u64,
    mode: Mode,
    lobby: Arc<Mutex<Lobby>>,
}
impl Drop for PlayingToken {
    fn drop(&mut self) {
        self.lobby
            .lock()
            .unwrap()
            .leave_game(self.client_id, self.mode);
    }
}

pub fn join_game_in_a_lobby(
    lobby: Arc<Mutex<Lobby>>,
    client_id: u64,
    mode: Mode,
) -> Option<(Arc<GameWrapper>, PlayingToken)> {
    let game_wrapper_if_not_full = lobby.lock().unwrap().join_game(client_id, mode);
    game_wrapper_if_not_full.map(|game_wrapper| {
        (
            game_wrapper,
            PlayingToken {
                client_id,
                mode,
                lobby,
            },
        )
    })
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
