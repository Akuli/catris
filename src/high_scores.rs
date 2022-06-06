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

#[derive(Debug)]
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

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn log(message: &str) {
    println!("[high scores] {}", message);
}

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
            log(&format!("Created {}", filename));
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

async fn save_high_score(filename: &str, hs: &HighScore) -> Result<(), io::Error> {
    log(&format!("Appending to {}: {:?}", filename, hs));
    let mut file = OpenOptions::new().append(true).open(filename).await?;
    /*
    TODO: change format, current format is pretty shit.
        - Get rid of lobby id (here always "-")
        - Add timestamps
    */
    file.write_all(
        format!(
            "{}\t-\t{}\t{}\t{}\n",
            mode_to_string(hs.mode),
            hs.score,
            hs.duration.as_secs_f64(),
            &hs.players.join("\t")
        )
        .as_bytes(),
    )
    .await?;
    Ok(())
}

// Prevent multiple games writing their high scores at once.
// File name stored here so I won't forget to use this
lazy_static! {
    static ref FILE_LOCK: Mutex<&'static str> = Mutex::new("catris_high_scores.txt");
}

async fn read_top_matching_high_scores(
    filename: &str,
    mode: Mode,
    multiplayer: bool,
) -> Result<Vec<HighScore>, io::Error> {
    let mut file = OpenOptions::new().read(true).open(filename).await?;
    let mut lines = BufReader::new(&mut file).lines();

    let first_line = lines.next_line().await?;
    if first_line != Some(format!("catris high scores file v{}", VERSION)) {
        return Err(io::Error::new(
            ErrorKind::Other,
            format!(
                "unexpected first line in high scores file: {:?}",
                first_line
            ),
        ));
    }

    let mut result = vec![];

    while let Some(line) = lines.next_line().await? {
        if line.trim().len() == 0 || line.trim().starts_with('#') {
            continue;
        }

        let mut parts = line.split('\t');
        let mode_name = parts.next().unwrap();
        let _ = parts.next().unwrap(); // lobby id
        let score_string = parts.next().unwrap();
        let duration_secs_string = parts.next().unwrap();
        let players: Vec<String> = parts.map(|s| s.to_string()).collect();
        assert!(players.len() != 0);

        if mode_name == mode_to_string(mode) && (players.len() >= 2) == multiplayer {
            // TODO: take only the best 5 high scores
            result.push(HighScore {
                mode,
                players,
                score: score_string.parse::<usize>().unwrap(),
                duration: Duration::from_secs_f64(duration_secs_string.parse::<f64>().unwrap()),
            });
        }
    }

    Ok(result)
}

pub async fn add_high_score(hs: &HighScore) -> Result<(), io::Error> {
    let filename_handle = FILE_LOCK.lock().unwrap();
    ensure_file_exists(*filename_handle).await?;
    save_high_score(*filename_handle, &hs).await?;
    println!(
        "from file: {:?}",
        read_top_matching_high_scores(*filename_handle, Mode::Ring, true).await?
    );
    Ok(())
}
