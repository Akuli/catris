use crate::game_logic::Mode;
use std::error::Error;
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
pub struct GameResult {
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
            log(&format!("Creating {}", filename));
            file.write_all(format!("catris high scores file v{}\n", VERSION).as_bytes())
                .await?;
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

async fn append_result_to_file(filename: &str, result: &GameResult) -> Result<(), io::Error> {
    log(&format!("Appending to {}: {:?}", filename, result));
    let mut file = OpenOptions::new().append(true).open(filename).await?;
    /*
    TODO: change format, current format is kinda shit.
        - Get rid of lobby id (here always "-")
        - Add timestamps
    */
    file.write_all(
        format!(
            "{}\t-\t{}\t{}\t{}\n",
            mode_to_string(result.mode),
            result.score,
            result.duration.as_secs_f64(),
            &result.players.join("\t")
        )
        .as_bytes(),
    )
    .await?;
    Ok(())
}

// returns Some(i) when high_scores[i] is the newly added game result
fn add_game_result_if_high_score(
    high_scores: &mut Vec<GameResult>,
    result: GameResult,
) -> Option<usize> {
    // i is location in high scores list, initially top
    // Bring it down until list remains sorted
    let mut i = 0;
    while i < high_scores.len() && result.score < high_scores[i].score {
        i += 1;
    }
    high_scores.insert(i, result);
    high_scores.truncate(5);

    if i < high_scores.len() {
        Some(i)
    } else {
        None
    }
}

// Prevent multiple games writing their high scores at once.
// File name stored here so I won't forget to use this
lazy_static! {
    static ref FILE_LOCK: Mutex<&'static str> = Mutex::new("catris_high_scores.txt");
}

async fn read_matching_high_scores(
    filename: &str,
    mode: Mode,
    multiplayer: bool,
) -> Result<Vec<GameResult>, Box<dyn Error>> {
    let mut file = OpenOptions::new().read(true).open(filename).await?;
    let mut lines = BufReader::new(&mut file).lines();

    let first_line = lines.next_line().await?;
    if first_line != Some(format!("catris high scores file v{}", VERSION)) {
        return Err(Box::new(io::Error::new(
            ErrorKind::Other,
            format!(
                "unexpected first line in high scores file: {:?}",
                first_line
            ),
        )));
    }

    let mut result = vec![];

    let mut lineno = 1;
    while let Some(line) = lines.next_line().await? {
        lineno += 1;

        if line.trim().len() == 0 || line.trim().starts_with('#') {
            continue;
        }

        let split_error = || {
            io::Error::new(
                ErrorKind::Other,
                format!(
                    "not enough tab-separated parts on line {} of high scores file",
                    lineno
                ),
            )
        };

        let mut parts = line.split('\t');
        let mode_name = parts.next().ok_or_else(split_error)?;
        let _ = parts.next().ok_or_else(split_error)?; // lobby id
        let score_string = parts.next().ok_or_else(split_error)?;
        let duration_secs_string = parts.next().ok_or_else(split_error)?;

        let players: Vec<String> = parts.map(|s| s.to_string()).collect();
        assert!(players.len() != 0);

        if mode_name == mode_to_string(mode) && (players.len() >= 2) == multiplayer {
            add_game_result_if_high_score(
                &mut result,
                GameResult {
                    mode,
                    players,
                    score: score_string.parse()?,
                    duration: Duration::from_secs_f64(duration_secs_string.parse()?),
                },
            );
        }
    }

    Ok(result)
}

pub async fn add_result_and_get_high_scores(result: GameResult) -> Result<(Vec<GameResult>, Option<usize>), Box<dyn Error>> {
    let filename_handle = FILE_LOCK.lock().unwrap();
    ensure_file_exists(*filename_handle).await?;

    let mut high_scores =
        read_matching_high_scores(*filename_handle, result.mode, result.players.len() >= 2).await?;

    append_result_to_file(*filename_handle, &result).await?;
    let hs_index = add_game_result_if_high_score(&mut high_scores, result);

    Ok((high_scores, hs_index))
}
