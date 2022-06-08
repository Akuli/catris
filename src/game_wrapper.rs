use crate::ansi::Color;
use crate::game_logic::Game;
use crate::high_scores::add_result_and_get_high_scores;
use crate::high_scores::GameResult;
use crate::player::WorldPoint;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;
use std::time::Instant;
use tokio;
use tokio::sync::watch;
use tokio::time::timeout;

#[derive(Debug)]
pub enum GameStatus {
    Playing,
    Paused(Instant),
    HighScoresLoading,
    HighScoresLoaded {
        this_game_result: GameResult,
        top_results: Vec<GameResult>,
        this_game_index: Option<usize>, // where is this_game_result in top_results?
    },
    HighScoresError,
}

pub struct GameWrapper {
    pub game: Mutex<Game>,
    pub previous_pauses: Mutex<Duration>,

    // when game state has changed, the Playing status is sent again unchanged
    status_sender: watch::Sender<GameStatus>,
    pub status_receiver: watch::Receiver<GameStatus>,

    // Prevents blocks from falling down while a bomb or cleared row flashes.
    // This is here because of how it affects gameplay, not because of safety
    flashing_mutex: tokio::sync::Mutex<()>,
}

impl GameWrapper {
    pub fn new(game: Game) -> Self {
        let (status_sender, status_receiver) = watch::channel(GameStatus::Playing);
        GameWrapper {
            game: Mutex::new(game),
            previous_pauses: Mutex::new(Duration::ZERO),
            status_sender,
            status_receiver,
            flashing_mutex: tokio::sync::Mutex::new(()),
        }
    }

    pub fn mark_changed(&self) {
        self.status_sender.send_modify(|_| {});
    }

    // None means toggle
    pub fn set_paused(&self, want_paused: Option<bool>) {
        self.status_sender.send_modify(|value| match *value {
            GameStatus::Playing if want_paused != Some(false) => {
                *value = GameStatus::Paused(Instant::now());
            }
            GameStatus::Paused(pause_start) if want_paused != Some(true) => {
                *self.previous_pauses.lock().unwrap() += pause_start.elapsed();
                *value = GameStatus::Playing;
            }
            _ => {}
        });
    }

    fn get_duration(&self) -> Duration {
        let (game_start_time, game_end_time) = {
            let game = self.game.lock().unwrap();
            (game.start_time, game.end_time)
        };

        let including_previous_pauses = if let Some(t) = game_end_time {
            // Game is over
            t - game_start_time
        } else {
            match *self.status_receiver.borrow() {
                GameStatus::Paused(pause_start) => pause_start - game_start_time,
                GameStatus::Playing => game_start_time.elapsed(),
                _ => panic!(), // we shouldn't get here if game is over
            }
        };

        including_previous_pauses - *self.previous_pauses.lock().unwrap()
    }

    // can be called only if the game is over
    pub fn get_game_result(&self) -> GameResult {
        let (mode, score, players) = {
            let game = self.game.lock().unwrap();
            let player_names = game
                .players
                .iter()
                .map(|p| p.borrow().name.clone())
                .collect();
            (game.mode(), game.get_score(), player_names)
        };
        GameResult {
            mode,
            score,
            players,
            duration: self.get_duration(),
        }
    }
}

// returns true if can keep going, false if game is ending
async fn pause_aware_sleep(weak_wrapper: Weak<GameWrapper>, mut duration: Duration) -> bool {
    let mut receiver = match weak_wrapper.upgrade() {
        // subscribe() needed because it marks previous messages as seen
        // if you instead clone the receiver, the first few calls to receiver.changed() will return immediately
        Some(w) => w.status_sender.subscribe(),
        None => return false, // game ended already, before we can do anything
    };

    loop {
        let is_paused = match *receiver.borrow() {
            GameStatus::Paused(_) => true,
            GameStatus::Playing => false,
            _ => return false, // game over
        };
        if is_paused {
            // wait for unpause, without consuming remaining time
            if receiver.changed().await.is_err() {
                // game ended while waiting
                return false;
            }
        } else {
            // wait for game to pause or end, by at most the given sleep time
            let start = Instant::now();
            match timeout(duration, receiver.changed()).await {
                Err(_) => {
                    // timed out: we successfully slept the whole duration
                    return true;
                }
                Ok(Err(_)) => {
                    // receiver.changed() failed: sender no longer exists, game ended
                    return false;
                }
                Ok(Ok(())) => {
                    // pause was toggled
                    let successfully_slept = start.elapsed();
                    duration = duration
                        .checked_sub(successfully_slept)
                        .unwrap_or(Duration::ZERO);
                    if duration.is_zero() {
                        return true;
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
        wrapper.mark_changed();
        if !pause_aware_sleep(Arc::downgrade(&wrapper), Duration::from_millis(100)).await {
            return;
        }
    }
    for p in points {
        wrapper.game.lock().unwrap().flashing_points.remove(p);
    }
}

async fn move_blocks_down(weak_wrapper: Weak<GameWrapper>, fast: bool) {
    loop {
        let sleep_duration = if fast {
            Duration::from_millis(25)
        } else if let Some(wrapper) = weak_wrapper.upgrade() {
            let minutes = wrapper.get_duration().as_secs_f32() / 60.0;
            // TODO: should speed up more if you play badly
            let moves_per_second = 2.0 * (1.07 as f32).powf(minutes);
            Duration::from_secs_f32(1. / moves_per_second)
        } else {
            return;
        };
        if !pause_aware_sleep(weak_wrapper.clone(), sleep_duration).await {
            return;
        }

        match weak_wrapper.upgrade() {
            Some(wrapper) => {
                let mut _lock = wrapper.flashing_mutex.lock().await;
                let (moved, full) = {
                    let mut game = wrapper.game.lock().unwrap();
                    if game.players.is_empty() {
                        // can happen when the game ends, although it no longer matters what happens to game state
                        // avoid panics though:
                        //    - empty rows are considered full (no blocks missing)
                        //    - full rows increment score
                        //    - score calculation assumes at least 1 player
                        return;
                    }
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
    while pause_aware_sleep(weak_wrapper.clone(), Duration::from_secs(1)).await {
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

async fn handle_game_over(status_sender: &watch::Sender<GameStatus>, this_game_result: GameResult) {
    // .send() fails when there are no receivers
    // we don't really care if everyone disconnects while high scores are loading
    _ = status_sender.send(GameStatus::HighScoresLoading);
    match add_result_and_get_high_scores(this_game_result.clone()).await {
        Ok((top_results, this_game_index)) => {
            _ = status_sender.send(GameStatus::HighScoresLoaded {
                this_game_result,
                top_results,
                this_game_index,
            });
        }
        Err(e) => {
            eprintln!("ERROR: saving game result to high scores file failed");
            eprintln!("  game result = {:?}", this_game_result);
            eprintln!("  error = {:?}", e);
            _ = status_sender.send(GameStatus::HighScoresError);
        }
    }
}

async fn start_please_wait_counters_as_needed(
    weak_wrapper: Weak<GameWrapper>,
    mut receiver: watch::Receiver<GameStatus>,
) {
    loop {
        {
            let wrapper = weak_wrapper.upgrade();
            if wrapper.is_none() {
                return;
            }
            let wrapper = wrapper.unwrap();

            // nothing else should get a game out of playing/paused status
            assert!(matches!(
                *receiver.borrow(),
                GameStatus::Playing | GameStatus::Paused(_)
            ));

            let ids_if_not_game_over = wrapper
                .game
                .lock()
                .unwrap()
                .start_pending_please_wait_counters();
            if let Some(ids) = ids_if_not_game_over {
                for client_id in &ids {
                    tokio::spawn(tick_please_wait_counter(
                        Arc::downgrade(&wrapper),
                        *client_id,
                    ));
                }
                if !ids.is_empty() {
                    wrapper.mark_changed();
                }
            } else {
                // game over
                handle_game_over(&wrapper.status_sender, wrapper.get_game_result()).await;
                return;
            }
        }

        // Can fail, if game no longer exists (we only have a weak reference)
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

pub fn start_tasks(wrapper: Arc<GameWrapper>) {
    tokio::spawn(move_blocks_down(Arc::downgrade(&wrapper), true));
    tokio::spawn(move_blocks_down(Arc::downgrade(&wrapper), false));
    tokio::spawn(start_please_wait_counters_as_needed(
        Arc::downgrade(&wrapper),
        wrapper.status_receiver.clone(),
    ));
}
