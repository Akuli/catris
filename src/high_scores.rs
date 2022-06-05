use crate::game_logic::Mode;
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
    pub mode: Mode,
    pub score: usize,
    pub duration: Duration,
    pub players: Vec<String>,
}

fn mode_to_string(mode: Mode) -> &'static str {
    match mode {
        Mode::Traditional => "traditional",
        Mode::Bottle => "bottle",
        Mode::Ring => "ring",
    }
}

/*async fn read_best_high_scores(file: &mut File) -> Result<(), io::Error> {
    let mut lines = BufReader::new(file).lines();

    if lines.next_line().await? != Some("catris high scores file v{}\n".to_string()) {
        return Err(io::Error::new(ErrorKind::Other, "unexpected first line in high scores file"));
    }

    Ok(())
}*/

const VERSION: &str = env!("CARGO_PKG_VERSION");

async fn ensure_file_exists(filename: &str) -> Result<(), io::Error> {
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(filename)
        .await
    {
        Ok(mut file) => {
            file.write_all(format!("catris high scores file v{}\n", VERSION).as_bytes())
                .await?;
            println!("[high scores] Created {}", filename);
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

async fn save_high_score(filename: &str, hs: &HighScore) -> Result<(), io::Error> {
    let mut file = OpenOptions::new().append(true).open(filename).await?;
    // TODO: change format, current format is pretty shit.
    // Get rid of lobby id (here always "-"), add timestamps.
    file.write_all(format!(
        "{}\t-\t{}\t{}\t{}\n",
        mode_to_string(hs.mode),
        hs.score,
        hs.duration.as_secs_f64(),
        &hs.players.join("\t")
    ).as_bytes()).await?;
    Ok(())
}

// Prevent multiple games writing their high scores at once.
// File name stored here so I won't forget to use this
lazy_static! {
    static ref FILE_LOCK: Mutex<&'static str> = Mutex::new("catris_high_scores.txt");
}

pub async fn add_high_score(hs: &HighScore) -> Result<(), io::Error> {
    let filename_handle = FILE_LOCK.lock().unwrap();
    ensure_file_exists(*filename_handle).await?;
    save_high_score(*filename_handle, &hs).await?;
    Ok(())
}
