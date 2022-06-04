use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;
use tokio;
use tokio::sync::watch;
use tokio::time::sleep;

use crate::ansi::Color;
use crate::logic_base::WorldPoint;
use crate::modes::AnyGame;

pub struct GameWrapper {
    pub game: Mutex<AnyGame>,
    // change event triggers when re-rendering might be needed
    changed_sender: watch::Sender<()>,
    pub changed_receiver: watch::Receiver<()>,

    // Prevents blocks from falling down while a bomb or cleared row flashes.
    // This is here because of how it affects gameplay, not because of safety
    flashing_mutex: tokio::sync::Mutex<()>,
}

impl GameWrapper {
    pub fn new(game: AnyGame) -> Self {
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

pub async fn flash(wrapper: Arc<GameWrapper>, points: &[WorldPoint]) {
    // TODO: define and use constants
    for color in [Color::WHITE_BACKGROUND.bg, 0, Color::WHITE_BACKGROUND.bg, 0] {
        for p in points {
            wrapper
                .game
                .lock()
                .unwrap()
                .get_flashing_points_mut()
                .insert(*p, color);
        }
        wrapper.changed_sender.send(()).unwrap();
        sleep(Duration::from_millis(100)).await;
    }
    for p in points {
        wrapper
            .game
            .lock()
            .unwrap()
            .get_flashing_points_mut()
            .remove(p);
    }
}

pub async fn move_blocks_down(weak_wrapper: Weak<GameWrapper>, fast: bool) {
    loop {
        sleep(Duration::from_millis(if fast { 25 } else { 400 })).await;
        match weak_wrapper.upgrade() {
            Some(wrapper) => {
                {
                    let mut _lock = wrapper.flashing_mutex.lock().await;
                    let full = {
                        let mut game = wrapper.game.lock().unwrap();
                        game.move_blocks_down(fast);
                        game.find_full_rows()
                    };
                    if full.len() != 0 {
                        flash(wrapper.clone(), &full).await;
                        let mut game = wrapper.game.lock().unwrap();
                        game.remove_full_rows(&full);
                        // Moving landed squares can cause them to overlap moving squares
                        game.remove_overlapping_landed_squares();
                    }
                }

                wrapper.mark_changed();
            }
            None => return,
        }
    }
}
