use crate::game_logic::game::Mode;
use std::error::Error;
use std::io;
use std::io::ErrorKind;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::SeekFrom;

#[derive(Debug, Clone, PartialEq)]
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

// if format changes, please add auto-upgrading code and update version in Cargo.toml
const VERSION: &str = env!("CARGO_PKG_VERSION_MAJOR");

fn log(message: &str) {
    println!("[high scores] {}", message);
}

const HEADER_PREFIX: &str = "catris high scores file v";

async fn ensure_file_exists(filename: &str) -> Result<(), io::Error> {
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(filename)
        .await
    {
        Ok(mut file) => {
            log(&format!("Creating {}", filename));
            file.write_all(format!("{}{}\n", HEADER_PREFIX, VERSION).as_bytes())
                .await?;
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

async fn append_update_comment(filename: &str, old_version: &str) -> Result<(), io::Error> {
    log(&format!(
        "upgrading {} from v{} to v{}",
        filename, old_version, VERSION
    ));
    let mut file = OpenOptions::new().append(true).open(filename).await?;
    file.write_all(
        format!("# --- upgraded from v{} to v{} ---\n", old_version, VERSION).as_bytes(),
    )
    .await?;
    Ok(())
}

async fn update_version_number(filename: &str) -> Result<(), io::Error> {
    let mut file = OpenOptions::new().write(true).open(filename).await?;
    file.seek(SeekFrom::Start(HEADER_PREFIX.len() as u64))
        .await?;
    file.write_all(VERSION.as_bytes()).await?;
    Ok(())
}

async fn upgrade_if_needed(filename: &str) -> Result<(), Box<dyn Error>> {
    let first_line = {
        let mut file = OpenOptions::new().read(true).open(filename).await?;
        BufReader::new(&mut file)
            .lines()
            .next_line()
            .await?
            .ok_or("high scores file is empty")?
    };

    if let Some(old_version) = first_line.strip_prefix(HEADER_PREFIX) {
        match old_version {
            "1" | "2" | "3" if VERSION == "4" => {
                // Previous formats are compatible with v4
                append_update_comment(filename, old_version).await?;
                update_version_number(filename).await?;
                Ok(())
            }
            VERSION => Ok(()),
            _ => Err(format!("unknown version: {}", old_version).into()),
        }
    } else {
        let message = format!(
            "unexpected first line in high scores file: {:?}",
            first_line
        );
        Err(message.into())
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

async fn read_matching_high_scores(
    filename: &str,
    mode: Mode,
    multiplayer: bool,
) -> Result<Vec<GameResult>, Box<dyn Error>> {
    let mut file = OpenOptions::new().read(true).open(filename).await?;
    let mut lines = BufReader::new(&mut file).lines();
    lines
        .next_line()
        .await?
        .ok_or("high scores file is empty")?;

    let mut result = vec![];

    let mut lineno = 1;
    while let Some(line) = lines.next_line().await? {
        lineno += 1;

        if line.trim().is_empty() || line.trim().starts_with('#') {
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
        assert!(!players.is_empty());

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

// Prevent multiple games writing their high scores at once.
// File name stored here so I won't forget to use this
lazy_static! {
    static ref FILE_LOCK: tokio::sync::Mutex<&'static str> =
        tokio::sync::Mutex::new("catris_high_scores.txt");
}

pub async fn add_result_and_get_high_scores(
    result: GameResult,
) -> Result<(Vec<GameResult>, Option<usize>), Box<dyn Error>> {
    let filename_handle = FILE_LOCK.lock().await;
    ensure_file_exists(*filename_handle).await?;
    upgrade_if_needed(*filename_handle).await?;

    let mut high_scores =
        read_matching_high_scores(*filename_handle, result.mode, result.players.len() >= 2).await?;

    append_result_to_file(*filename_handle, &result).await?;
    let hs_index = add_game_result_if_high_score(&mut high_scores, result);

    Ok((high_scores, hs_index))
}

#[cfg(test)]
mod test {
    use super::*;
    use std::path::Path;

    async fn read_file(filename: &str) -> String {
        String::from_utf8(tokio::fs::read(Path::new(&filename)).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn test_upgrading_from_v1() {
        let tempdir = tempfile::tempdir().unwrap();
        let filename = tempdir
            .path()
            .join("high_scores.txt")
            .to_str()
            .unwrap()
            .to_string();

        // TODO: don't convert between paths and strings so much
        tokio::fs::write(
            Path::new(&filename),
            concat!(
                "catris high scores file v1\n",
                "traditional\t-\t11\t22.75\tSinglePlayer\n",
                "traditional\t-\t33\t44\tPlayer 1\tPlayer 2\n",
            ),
        )
        .await
        .unwrap();

        upgrade_if_needed(&filename).await.unwrap();

        // Comments don't conflict with hashtags in player names.
        // Only a hashtag in the beginning of a line is treated as a comment.
        assert_eq!(
            read_file(&filename).await,
            concat!(
                "catris high scores file v4\n",
                "traditional\t-\t11\t22.75\tSinglePlayer\n",
                "traditional\t-\t33\t44\tPlayer 1\tPlayer 2\n",
                "# --- upgraded from v1 to v4 ---\n",
            )
        );

        // Make sure it's readable
        read_matching_high_scores(&filename, Mode::Traditional, false)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_reading() {
        let tempdir = tempfile::tempdir().unwrap();
        let filename = tempdir
            .path()
            .join("high_scores.txt")
            .to_str()
            .unwrap()
            .to_string();

        tokio::fs::write(
            Path::new(&filename),
            concat!(
                "catris high scores file v4\n",
                "traditional\t-\t11\t22.75\tSinglePlayer\n",
                "traditional\t-\t33\t44\tAlice\tBob\tCharlie\n",
                "traditional\tABC123\t55\t66\t#HashTag#\n",
                "traditional\tABC123\t4000\t123\tGood player\n",
                "   # comment line \n",
                "  ",
                "",
                "#traditional\t-\t55\t66\tThis is skipped\n",
                "# --- upgraded from v3 to v4 ---\n",
                "bottle\t-\t77\t88\tBottleFoo\n",
            ),
        )
        .await
        .unwrap();

        let mut result = read_matching_high_scores(&filename, Mode::Traditional, false)
            .await
            .unwrap();
        assert_eq!(
            result,
            vec![
                // Better results come first
                GameResult {
                    mode: Mode::Traditional,
                    score: 4000,
                    duration: Duration::from_secs(123),
                    players: vec!["Good player".to_string()]
                },
                GameResult {
                    mode: Mode::Traditional,
                    score: 55,
                    duration: Duration::from_secs(66),
                    players: vec!["#HashTag#".to_string()]
                },
                GameResult {
                    mode: Mode::Traditional,
                    score: 11,
                    duration: Duration::from_secs_f32(22.75),
                    players: vec!["SinglePlayer".to_string()]
                }
            ]
        );

        let second_place_result = GameResult {
            mode: Mode::Traditional,
            score: 3000,
            duration: Duration::from_secs_f32(123.45),
            players: vec!["Second Place".to_string()],
        };
        let index = add_game_result_if_high_score(&mut result, second_place_result.clone());
        assert_eq!(result.len(), 4);
        assert_eq!(result[1], second_place_result);
        assert_eq!(index, Some(1));

        // Multiplayer
        let result = read_matching_high_scores(&filename, Mode::Traditional, true)
            .await
            .unwrap();
        assert_eq!(
            result,
            vec![GameResult {
                mode: Mode::Traditional,
                score: 33,
                duration: Duration::from_secs(44),
                players: vec![
                    "Alice".to_string(),
                    "Bob".to_string(),
                    "Charlie".to_string()
                ]
            }]
        );
    }

    #[tokio::test]
    async fn test_writing() {
        let tempdir = tempfile::tempdir().unwrap();
        let filename = tempdir
            .path()
            .join("high_scores.txt")
            .to_str()
            .unwrap()
            .to_string();
        ensure_file_exists(&filename).await.unwrap();

        let sample_result = GameResult {
            mode: Mode::Ring,
            score: 7000,
            duration: Duration::from_secs(123),
            players: vec!["Foo".to_string(), "Bar".to_string()],
        };

        append_result_to_file(&filename, &sample_result)
            .await
            .unwrap();
        let from_file = read_matching_high_scores(&filename, Mode::Ring, true)
            .await
            .unwrap();
        assert_eq!(from_file, [sample_result]);
    }
}
