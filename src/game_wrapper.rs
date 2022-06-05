use crate::ansi::Color;
use crate::game_logic::Game;
use crate::lobby::ClientInfo;
use crate::player::WorldPoint;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;
use std::time::Instant;
use tokio;
use tokio::sync::watch;
use tokio::time::timeout;

pub struct GameWrapper {
    pub game: Mutex<Game>,
    // change event triggers when re-rendering might be needed
    changed_sender: watch::Sender<()>,
    pub changed_receiver: watch::Receiver<()>,

    paused_sender: watch::Sender<bool>,
    paused_receiver: watch::Receiver<bool>,

    // Prevents blocks from falling down while a bomb or cleared row flashes.
    // This is here because of how it affects gameplay, not because of safety
    flashing_mutex: tokio::sync::Mutex<()>,
}

impl GameWrapper {
    pub fn new(game: Game) -> Self {
        let (changed_sender, changed_receiver) = watch::channel(());
        let (paused_sender, paused_receiver) = watch::channel(false);
        GameWrapper {
            game: Mutex::new(game),
            changed_sender,
            changed_receiver,
            paused_sender,
            paused_receiver,
            flashing_mutex: tokio::sync::Mutex::new(()),
        }
    }

    pub fn mark_changed(&self) {
        // .send() fails when there are no receivers
        // shouldn't fail here, because this struct owns a receiver
        self.changed_sender.send(()).unwrap();
    }

    pub fn is_paused(&self) -> bool {
        *self.paused_receiver.borrow()
    }

    pub fn set_paused(&self, value: bool) {
        self.paused_sender.send(value).unwrap();
        self.mark_changed();
    }

    pub fn add_player(&self, client_info: &ClientInfo) {
        self.game.lock().unwrap().add_player(client_info);
        self.mark_changed();
    }

    pub fn remove_player_if_exists(&self, client_id: u64) {
        self.game.lock().unwrap().remove_player_if_exists(client_id);
        self.mark_changed();
    }
}

async fn pause_aware_sleep(wrapper: Weak<GameWrapper>, mut duration: Duration) {
    let mut receiver = match wrapper.upgrade() {
        // subscribe() needed because it marks previous messages as seen
        // if you instead clone the receiver, the first few calls to receiver.changed() will return immediately
        Some(w) => w.paused_sender.subscribe(),
        None => return, // game ended already, before we can do anything
    };

    loop {
        let paused = *receiver.borrow();
        if paused {
            // wait for unpause, without consuming remaining time
            if receiver.changed().await.is_err() {
                // game ended while waiting
                return;
            }
        } else {
            // wait for game to pause or end, by at most the given sleep time
            let start = Instant::now();
            match timeout(duration, receiver.changed()).await {
                Err(_) => {
                    // timed out: we successfully slept the whole duration
                    return;
                }
                Ok(Err(_)) => {
                    // receiver.changed() failed: sender no longer exists, game ended
                    return;
                }
                Ok(Ok(())) => {
                    // pause was toggled
                    let successfully_slept = start.elapsed();
                    duration = duration
                        .checked_sub(successfully_slept)
                        .unwrap_or(Duration::ZERO);
                    if duration.is_zero() {
                        return;
                    }
                }
            }
        }
    }
}

async fn flash(wrapper: Arc<GameWrapper>, points: &[WorldPoint]) {
    for color in [Color::WHITE_BACKGROUND.bg, 0, Color::WHITE_BACKGROUND.bg, 0] {
        for p in points {
            wrapper
                .game
                .lock()
                .unwrap()
                .flashing_points
                .insert(*p, color);
        }
        wrapper.changed_sender.send(()).unwrap();
        pause_aware_sleep(Arc::downgrade(&wrapper), Duration::from_millis(100)).await;
    }
    for p in points {
        wrapper.game.lock().unwrap().flashing_points.remove(p);
    }
}

async fn move_blocks_down(weak_wrapper: Weak<GameWrapper>, fast: bool) {
    let sleep_duration = Duration::from_millis(if fast { 25 } else { 400 });
    loop {
        pause_aware_sleep(weak_wrapper.clone(), sleep_duration).await;
        match weak_wrapper.upgrade() {
            Some(wrapper) => {
                let mut _lock = wrapper.flashing_mutex.lock().await;
                let (moved, full) = {
                    let mut game = wrapper.game.lock().unwrap();
                    let moved = game.move_blocks_down(fast);
                    (moved, game.find_full_rows_and_increment_score())
                };
                if !full.is_empty() {
                    flash(wrapper.clone(), &full).await;
                    let mut game = wrapper.game.lock().unwrap();
                    game.remove_full_rows(&full);
                    wrapper.mark_changed();
                }
                if moved {
                    wrapper.mark_changed();
                }
            }
            None => return,
        }
    }
}

async fn tick_please_wait_counter(weak_wrapper: Weak<GameWrapper>, client_id: u64) {
    loop {
        pause_aware_sleep(weak_wrapper.clone(), Duration::from_secs(1)).await;
        match weak_wrapper.upgrade() {
            Some(wrapper) => {
                let mut game = wrapper.game.lock().unwrap();
                let run_again = game.tick_please_wait_counter(client_id);
                wrapper.mark_changed();
                if !run_again {
                    return;
                }
            }
            None => return,
        }
    }
}

async fn start_please_wait_counters_as_needed(
    weak_wrapper: Weak<GameWrapper>,
    mut changed_receiver: watch::Receiver<()>,
) {
    loop {
        match weak_wrapper.upgrade() {
            Some(wrapper) => {
                let ids = wrapper
                    .game
                    .lock()
                    .unwrap()
                    .start_pending_please_wait_counters();
                for client_id in &ids {
                    tokio::spawn(tick_please_wait_counter(
                        Arc::downgrade(&wrapper),
                        *client_id,
                    ));
                }
                if !ids.is_empty() {
                    wrapper.mark_changed();
                }
            }
            None => return,
        }

        // Can fail, if game no longer exists (we only have a weak reference)
        if changed_receiver.changed().await.is_err() {
            return;
        }
    }
}

pub fn start_tasks(wrapper: Arc<GameWrapper>) {
    tokio::spawn(move_blocks_down(Arc::downgrade(&wrapper), true));
    tokio::spawn(move_blocks_down(Arc::downgrade(&wrapper), false));
    tokio::spawn(start_please_wait_counters_as_needed(
        Arc::downgrade(&wrapper),
        wrapper.changed_receiver.clone(),
    ));
}
