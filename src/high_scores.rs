use std::io;
use std::io::ErrorKind;
use std::sync::Mutex;
use std::time::Duration;
use tokio::fs::File;
use tokio::fs::OpenOptions;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;

pub struct HighScore {
    pub score: usize,
    pub duration: Duration,
    pub players: Vec<String>,
}

async fn read_best_high_scores(file: &mut File) -> Result<(), io::Error> {
    let mut lines = BufReader::new(file).lines();
    Ok(())
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

async fn ensure_file_exists(filename: &str) -> Result<(), io::Error> {
    println!("Asd");
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(filename)
        .await
    {
        Ok(mut file) => {
            file.write_all(format!("catris high scores file v{}\n", VERSION).as_bytes())
                .await?;
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

// Prevent multiple games writing their high scores at once.
// File name stored here so I won't forget to use this
lazy_static! {
    static ref FILE_LOCK: Mutex<&'static str> = Mutex::new("catris_high_scores.txt");
}

pub async fn add_high_score(hs: &HighScore) {
    let filename_handle = FILE_LOCK.lock().unwrap();
    ensure_file_exists(*filename_handle).await;
}
