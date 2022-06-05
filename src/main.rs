#[macro_use(lazy_static)]
extern crate lazy_static;

use crate::game_logic::Mode;
use std::io;
use std::time::Duration;

mod ansi;
mod blocks;
mod client;
mod game_logic;
mod game_wrapper;
mod high_scores;
mod ingame_ui;
mod lobby;
mod player;
mod render;
mod views;

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    crate::high_scores::add_high_score(&crate::high_scores::HighScore {
        mode: Mode::Ring,
        score: 123,
        duration: Duration::from_millis(678901),
        players: vec!["John".to_string(), "Bob".to_string(), "Mary".to_string()],
    })
    .await?;
    Ok(())
}
