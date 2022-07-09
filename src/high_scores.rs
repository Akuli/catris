use crate::game_logic::game::Mode;
use chrono::DateTime;
use chrono::Utc;
use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::time::Duration;

// https://users.rust-lang.org/t/convert-box-dyn-error-to-box-dyn-error-send/48856/8
type AnyErrorThreadSafe = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub struct GameResult {
    pub mode: Mode,
    pub score: usize,
    pub duration: Duration,
    pub players: Vec<String>,
    pub timestamp: Option<DateTime<Utc>>,
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

fn ensure_file_exists(filename: &str) -> Result<(), AnyErrorThreadSafe> {
    match fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(filename)
    {
        Ok(mut file) => {
            log(&format!("Creating {}", filename));
            file.write_all(format!("{}{}\n", HEADER_PREFIX, VERSION).as_bytes())?;
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn append_update_comment(filename: &str, old_version: &str) -> Result<(), AnyErrorThreadSafe> {
    log(&format!(
        "upgrading {} from v{} to v{}",
        filename, old_version, VERSION
    ));
    let mut file = fs::OpenOptions::new().append(true).open(filename)?;
    file.write_all(
        format!("# --- upgraded from v{} to v{} ---\n", old_version, VERSION).as_bytes(),
    )?;
    Ok(())
}

fn update_version_number(filename: &str) -> Result<(), AnyErrorThreadSafe> {
    let mut file = fs::OpenOptions::new().write(true).open(filename)?;
    file.seek(SeekFrom::Start(HEADER_PREFIX.len() as u64))?;
    file.write_all(VERSION.as_bytes())?;
    Ok(())
}

fn upgrade_if_needed(filename: &str) -> Result<(), AnyErrorThreadSafe> {
    let first_line = {
        let mut file = fs::OpenOptions::new().read(true).open(filename)?;
        BufReader::new(&mut file)
            .lines()
            .next()
            .ok_or("high scores file is empty")??
    };

    if let Some(old_version) = first_line.strip_prefix(HEADER_PREFIX) {
        match old_version {
            "1" | "2" | "3" if VERSION == "4" => {
                // Previous formats are compatible with v4
                append_update_comment(filename, old_version)?;
                update_version_number(filename)?;
                Ok(())
            }
            VERSION => Ok(()),
            _ => Err(format!("unknown version: {}", old_version).into()),
        }
    } else {
        Err(format!(
            "unexpected first line in high scores file: {:?}",
            first_line
        )
        .into())
    }
}

fn append_result_to_file(filename: &str, result: &GameResult) -> Result<(), AnyErrorThreadSafe> {
    log(&format!("Appending to {}: {:?}", filename, result));
    let mut file = fs::OpenOptions::new().append(true).open(filename)?;
    file.write_all(
        format!(
            "{}\t{}\t{}\t{}\t{}\n",
            mode_to_string(result.mode),
            // timestamp can't be None in new high scores, that's a legacy thing
            result.timestamp.unwrap().to_rfc3339(),
            result.score,
            result.duration.as_secs_f64(),
            &result.players.join("\t")
        )
        .as_bytes(),
    )?;
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

fn looks_like_lobby_id(value: &str) -> bool {
    value.chars().count() == 6 && value.chars().all(|c| matches!(c, 'A'..='Z' | '0'..='9'))
}

fn parse_timestamp_field(value: &str) -> Result<Option<DateTime<Utc>>, AnyErrorThreadSafe> {
    // saving lobby IDs to files was a bad idea, just ignore them
    if value == "-" || looks_like_lobby_id(value) {
        Ok(None)
    } else {
        let result = DateTime::parse_from_rfc3339(value)?;
        Ok(Some(result.into()))
    }
}

fn read_matching_high_scores(
    filename: &str,
    mode: Mode,
    multiplayer: bool,
) -> Result<Vec<GameResult>, AnyErrorThreadSafe> {
    let mut file = fs::OpenOptions::new().read(true).open(filename)?;
    let mut lines = BufReader::new(&mut file).lines();
    lines.next().ok_or("high scores file is empty")??;

    let mut result = vec![];

    let mut lineno = 1;
    for line in lines {
        lineno += 1;

        let line = line?;
        if line.trim().is_empty() || line.trim().starts_with('#') {
            continue;
        }

        let split_error = || {
            format!(
                "not enough tab-separated parts on line {} of high scores file",
                lineno
            )
        };

        let mut parts = line.split('\t');
        let mode_name = parts.next().ok_or_else(split_error)?;
        let timestamp_string = parts.next().ok_or_else(split_error)?;
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
                    timestamp: parse_timestamp_field(timestamp_string)?,
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
) -> Result<(Vec<GameResult>, Option<usize>), AnyErrorThreadSafe> {
    let filename_handle = FILE_LOCK.lock().await;

    // Not using tokio's file io because it's easy to forget to flush after writing
    // https://github.com/tokio-rs/tokio/issues/4296
    tokio::task::spawn_blocking(move || {
        ensure_file_exists(*filename_handle)?;
        upgrade_if_needed(*filename_handle)?;

        let mut high_scores =
            read_matching_high_scores(*filename_handle, result.mode, result.players.len() >= 2)?;

        append_result_to_file(*filename_handle, &result)?;
        let hs_index = add_game_result_if_high_score(&mut high_scores, result);
        Ok((high_scores, hs_index))
    })
    .await?
}

#[cfg(test)]
mod test {
    use super::*;

    fn read_file(filename: &str) -> String {
        String::from_utf8(fs::read(&filename).unwrap()).unwrap()
    }

    #[test]
    fn test_upgrading_from_v1() {
        let tempdir = tempfile::tempdir().unwrap();
        let filename = tempdir
            .path()
            .join("high_scores.txt")
            .to_str()
            .unwrap()
            .to_string();

        fs::write(
            &filename,
            concat!(
                "catris high scores file v1\n",
                "traditional\t-\t11\t22.75\tSinglePlayer\n",
                "traditional\tABZ019\t33\t44\tPlayer 1\tPlayer 2\n",
            ),
        )
        .unwrap();

        upgrade_if_needed(&filename).unwrap();

        // Comments don't conflict with hashtags in player names.
        // Only a hashtag in the beginning of a line is treated as a comment.
        assert_eq!(
            read_file(&filename),
            concat!(
                "catris high scores file v4\n",
                "traditional\t-\t11\t22.75\tSinglePlayer\n",
                "traditional\tABZ019\t33\t44\tPlayer 1\tPlayer 2\n",
                "# --- upgraded from v1 to v4 ---\n",
            )
        );

        // Make sure it's readable
        read_matching_high_scores(&filename, Mode::Traditional, false).unwrap();
    }

    #[test]
    fn test_reading() {
        let tempdir = tempfile::tempdir().unwrap();
        let filename = tempdir
            .path()
            .join("high_scores.txt")
            .to_str()
            .unwrap()
            .to_string();

        fs::write(
            &filename,
            concat!(
                "catris high scores file v4\n",
                "traditional\t-\t33\t44\tAlice\tBob\tCharlie\n",
                "traditional\tABZ019\t55\t66\t#HashTag#\n",
                "traditional\tABZ019\t4000\t123\tGood player\n",
                "   # comment line \n",
                "  ",
                "",
                "#traditional\t-\t55\t66\tThis is skipped\n",
                "# --- upgraded from v3 to v4 ---\n",
                "bottle\t-\t77\t88\tBottleFoo\n",
                "traditional\t2022-07-02T23:57:22+00:00\t11\t22.75\tSinglePlayer\n",
            ),
        )
        .unwrap();

        let mut result = read_matching_high_scores(&filename, Mode::Traditional, false).unwrap();
        assert_eq!(
            result,
            vec![
                // Better results come first
                GameResult {
                    mode: Mode::Traditional,
                    score: 4000,
                    duration: Duration::from_secs(123),
                    players: vec!["Good player".to_string()],
                    timestamp: None,
                },
                GameResult {
                    mode: Mode::Traditional,
                    score: 55,
                    duration: Duration::from_secs(66),
                    players: vec!["#HashTag#".to_string()],
                    timestamp: None,
                },
                GameResult {
                    mode: Mode::Traditional,
                    score: 11,
                    duration: Duration::from_secs_f32(22.75),
                    players: vec!["SinglePlayer".to_string()],
                    timestamp: Some(
                        DateTime::parse_from_rfc3339("2022-07-02T23:57:22+00:00")
                            .unwrap()
                            .into()
                    ),
                }
            ]
        );

        let second_place_result = GameResult {
            mode: Mode::Traditional,
            score: 3000,
            duration: Duration::from_secs_f32(123.45),
            players: vec!["Second Place".to_string()],
            timestamp: None,
        };
        let index = add_game_result_if_high_score(&mut result, second_place_result.clone());
        assert_eq!(result.len(), 4);
        assert_eq!(result[1], second_place_result);
        assert_eq!(index, Some(1));

        // Multiplayer
        let result = read_matching_high_scores(&filename, Mode::Traditional, true).unwrap();
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
                ],
                timestamp: None,
            }]
        );
    }

    #[test]
    fn test_writing() {
        let tempdir = tempfile::tempdir().unwrap();
        let filename = tempdir
            .path()
            .join("high_scores.txt")
            .to_str()
            .unwrap()
            .to_string();
        ensure_file_exists(&filename).unwrap();

        let sample_result = GameResult {
            mode: Mode::Ring,
            score: 7000,
            duration: Duration::from_secs(123),
            players: vec!["Foo".to_string(), "Bar".to_string()],
            timestamp: Some(Utc::now()),
        };

        append_result_to_file(&filename, &sample_result).unwrap();
        let from_file = read_matching_high_scores(&filename, Mode::Ring, true).unwrap();
        assert_eq!(from_file, [sample_result]);
    }
}
