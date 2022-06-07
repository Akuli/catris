use crate::ansi::Color;
use crate::game_logic::Game;
use crate::high_scores::add_result_and_get_high_scores;
use crate::high_scores::GameResult;
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
                self.game
                    .lock()
                    .unwrap()
                    .add_paused_time(Instant::now() - pause_start);
                *value = GameStatus::Playing;
            }
            _ => {}
        });
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

// TODO: remove once i'm done debugging
impl Drop for GameWrapper {
    fn drop(&mut self) {
        println!("dropping Game Wrapper");
    }
}

async fn pause_aware_sleep(weak_wrapper: Weak<GameWrapper>, mut duration: Duration) {
    let mut receiver = match weak_wrapper.upgrade() {
        // subscribe() needed because it marks previous messages as seen
        // if you instead clone the receiver, the first few calls to receiver.changed() will return immediately
        Some(w) => w.status_sender.subscribe(),
        None => return, // game ended already, before we can do anything
    };

    loop {
        let is_paused = match *receiver.borrow() {
            GameStatus::Paused(_) => true,
            GameStatus::Playing => false,
            _ => return, // game over
        };
        if is_paused {
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
        wrapper.mark_changed();
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

            let game_result = {
                let mut game = wrapper.game.lock().unwrap();
                if let Some(ids) = game.start_pending_please_wait_counters() {
                    for client_id in &ids {
                        tokio::spawn(tick_please_wait_counter(
                            Arc::downgrade(&wrapper),
                            *client_id,
                        ));
                    }
                    if !ids.is_empty() {
                        wrapper.mark_changed();
                    }
                    None
                } else {
                    // game over, can't handle here because we're holding the game mutex
                    Some(game.get_result())
                }
            };

            if let Some(result) = game_result {
                handle_game_over(&wrapper.status_sender, result).await;
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
