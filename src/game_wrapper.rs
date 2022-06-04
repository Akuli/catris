use crate::ansi::Color;
use crate::game::Game;
use crate::player::WorldPoint;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;
use tokio;
use tokio::sync::watch;
use tokio::time::sleep;

pub struct GameWrapper {
    pub game: Mutex<Game>,
    // change event triggers when re-rendering might be needed
    changed_sender: watch::Sender<()>,
    pub changed_receiver: watch::Receiver<()>,

    // Prevents blocks from falling down while a bomb or cleared row flashes.
    // This is here because of how it affects gameplay, not because of safety
    flashing_mutex: tokio::sync::Mutex<()>,
}

impl GameWrapper {
    pub fn new(game: Game) -> Self {
        let (sender, receiver) = watch::channel(());
        GameWrapper {
            game: Mutex::new(game),
            changed_sender: sender,
            changed_receiver: receiver,
            flashing_mutex: tokio::sync::Mutex::new(()),
        }
    }

    pub fn mark_changed(&self) {
        self.changed_sender.send(()).unwrap();
    }
}

async fn flash(wrapper: Arc<GameWrapper>, points: &[WorldPoint]) {
    // TODO: define and use constants
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
        sleep(Duration::from_millis(100)).await;
    }
    for p in points {
        wrapper.game.lock().unwrap().flashing_points.remove(p);
    }
}

async fn move_blocks_down(weak_wrapper: Weak<GameWrapper>, fast: bool) {
    loop {
        sleep(Duration::from_millis(if fast { 25 } else { 400 })).await;
        match weak_wrapper.upgrade() {
            Some(wrapper) => {
                let mut _lock = wrapper.flashing_mutex.lock().await;
                let (moved, full) = {
                    let mut game = wrapper.game.lock().unwrap();
                    let moved = game.move_blocks_down(fast);
                    (moved, game.find_full_rows())
                };
                if !full.is_empty() {
                    flash(wrapper.clone(), &full).await;
                    let mut game = wrapper.game.lock().unwrap();
                    game.remove_full_rows(&full);
                }
                if moved || !full.is_empty() {
                    wrapper.mark_changed();
                }
            }
            None => return,
        }
    }
}

async fn tick_please_wait_counter(weak_wrapper: Weak<GameWrapper>, client_id: u64) {
    loop {
        sleep(Duration::from_secs(1)).await;
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
