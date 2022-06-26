use crate::game_logic::game::Mode;
use serde::Deserialize;
use serde::Serialize;
use std::fs;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::ErrorKind;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::time::Duration;

// https://users.rust-lang.org/t/convert-box-dyn-error-to-box-dyn-error-send/48856/8
type AnyErrorThreadSafe = Box<dyn std::error::Error + Send + Sync>;

fn mode_to_string(mode: Mode) -> &'static str {
    match mode {
        Mode::Traditional => "traditional",
        Mode::Bottle => "bottle",
        Mode::Ring => "ring",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameResult {
    pub mode: Mode,
    pub score: usize,
    pub duration: Duration,
    pub players: Vec<String>,
}

// high scores file format v4 and older didn't use json
#[derive(Serialize, Deserialize)]
struct GameResultInFileV5 {
    mode: String,
    score: usize,
    duration_sec: f64,
    players: Vec<String>,
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
        Err(e) => Err(e)?,
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

fn update_from_v4_or_older(filename: &str) -> Result<(), AnyErrorThreadSafe> {
    let temp_file = tempfile::tempfile()?;
    let mut writer = BufWriter::new(temp_file);

    writer.write_all(HEADER_PREFIX.as_bytes())?;
    writer.write_all(VERSION.as_bytes())?;
    writer.write_all(b"\n")?;

    let mut old_file = fs::OpenOptions::new().read(true).open(filename)?;
    let mut lines = BufReader::new(&mut old_file).lines();
    lines.next().ok_or("high scores file is empty")??;

    let mut lineno = 1;
    for line in lines {
        lineno += 1;

        let line = line?;
        if line.trim().starts_with('#') {
            writer.write_all(line.trim().as_bytes())?;
        } else if !line.trim().is_empty() {
            let split_error = || {
                format!(
                    "not enough tab-separated parts on line {} of high scores file",
                    lineno
                )
            };

            let mut parts = line.split('\t');
            let mode_name = parts.next().ok_or_else(split_error)?;
            let _ = parts.next().ok_or_else(split_error)?; // lobby id
            let score_string = parts.next().ok_or_else(split_error)?;
            let duration_sec_string = parts.next().ok_or_else(split_error)?;
            let players: Vec<String> = parts.map(|s| s.to_string()).collect();
            if players.is_empty() {
                Err(split_error())?;
            }

            assert!(VERSION == "5");
            let raw_game_result = GameResultInFileV5 {
                mode: mode_name.to_string(),
                score: score_string.parse()?,
                duration_sec: duration_sec_string.parse()?,
                players,
            };
            writer.write_all(serde_json::to_string(&raw_game_result)?.as_bytes())?;
        }
        writer.write_all(b"\n")?;
    }

    writer.flush()?;
    let temp_file = writer.get_mut();

    temp_file.seek(SeekFrom::Start(0))?;

    let mut new_file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(filename)?;
    io::copy(temp_file, &mut new_file)?;
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
            "1" | "2" | "3" | "4" => {
                append_update_comment(filename, old_version)?;
                update_from_v4_or_older(filename)?;
                Ok(())
            }
            VERSION => Ok(()),
            _ => Err(format!("unknown version: {}", old_version))?,
        }
    } else {
        Err(format!(
            "unexpected first line in high scores file: {:?}",
            first_line
        ))?
    }
}

fn append_result_to_file(filename: &str, result: &GameResult) -> Result<(), AnyErrorThreadSafe> {
    log(&format!("Appending to {}: {:?}", filename, result));
    let mut file = fs::OpenOptions::new().append(true).open(filename)?;
    let raw_game_result = GameResultInFileV5 {
        mode: mode_to_string(result.mode).to_string(),
        score: result.score,
        duration_sec: result.duration.as_secs_f64(),
        players: result.players.clone(),
    };
    file.write_all(serde_json::to_string(&raw_game_result)?.as_bytes())?;
    file.write_all(b"\n")?;
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

fn read_matching_high_scores(
    filename: &str,
    mode: Mode,
    multiplayer: bool,
) -> Result<Vec<GameResult>, AnyErrorThreadSafe> {
    let mut file = fs::OpenOptions::new().read(true).open(filename)?;
    let mut lines = BufReader::new(&mut file).lines();
    lines.next().ok_or("high scores file is empty")??;

    let mut result = vec![];

    for line in lines {
        let line = line?;
        if line.trim().is_empty() || line.trim().starts_with('#') {
            continue;
        }

        let raw_result: GameResultInFileV5 = serde_json::from_str(&line)?;
        if raw_result.mode == mode_to_string(mode) && (raw_result.players.len() >= 2) == multiplayer
        {
            add_game_result_if_high_score(
                &mut result,
                GameResult {
                    mode,
                    players: raw_result.players,
                    score: raw_result.score,
                    duration: Duration::from_secs_f64(raw_result.duration_sec),
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
    fn test_upgrading_from_v4_or_older() {
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
                "catris high scores file v3\n",
                "traditional\t-\t11\t22.75\tSinglePlayer\n",
                "traditional\t-\t33\t44\tAlice\tBob\tCharlie\n",
                // Comments don't conflict with hashtags in player names.
                // Only a hashtag in the beginning of a line is treated as a comment.
                "traditional\tABC123\t55\t66\t#HashTag#\n",
                "traditional\tABC123\t4000\t123\tGood Player\n",
                "   # comment line \n",
                "  ",
                "",
                "#traditional\t-\t55\t66\tThis is skipped\n",
                "# --- upgraded from v2 to v3 ---\n",
                "bottle\t-\t77\t88\tBottleFoo\n",
            ),
        )
        .unwrap();

        upgrade_if_needed(&filename).unwrap();

        assert_eq!(
            read_file(&filename),
            concat!(
                "catris high scores file v5\n",
                "{\"mode\":\"traditional\",\"score\":11,\"duration_sec\":22.75,\"players\":[\"SinglePlayer\"]}\n",
                "{\"mode\":\"traditional\",\"score\":33,\"duration_sec\":44.0,\"players\":[\"Alice\",\"Bob\",\"Charlie\"]}\n",
                "{\"mode\":\"traditional\",\"score\":55,\"duration_sec\":66.0,\"players\":[\"#HashTag#\"]}\n",
                "{\"mode\":\"traditional\",\"score\":4000,\"duration_sec\":123.0,\"players\":[\"Good Player\"]}\n",
                "# comment line\n",
                "#traditional\t-\t55\t66\tThis is skipped\n",
                "# --- upgraded from v2 to v3 ---\n",
                "{\"mode\":\"bottle\",\"score\":77,\"duration_sec\":88.0,\"players\":[\"BottleFoo\"]}\n",
                "# --- upgraded from v3 to v5 ---\n",
            )
        );

        // Make sure it's readable
        read_matching_high_scores(&filename, Mode::Traditional, false).unwrap();
    }

    #[test]
    fn test_reading_and_writing() {
        let tempdir = tempfile::tempdir().unwrap();
        let filename = tempdir
            .path()
            .join("high_scores.txt")
            .to_str()
            .unwrap()
            .to_string();

        ensure_file_exists(&filename).unwrap();
        #[rustfmt::skip] append_result_to_file(&filename, &GameResult { mode: Mode::Traditional, score: 10,   duration: Duration::from_secs_f32(22.75), players: vec!["SinglePlayer".to_string()] }).unwrap();
        #[rustfmt::skip] append_result_to_file(&filename, &GameResult { mode: Mode::Traditional, score: 30,   duration: Duration::from_secs_f32(44.0),  players: vec!["Alice".to_string(), "Bob".to_string(), "Charlie".to_string()] }).unwrap();
        #[rustfmt::skip] append_result_to_file(&filename, &GameResult { mode: Mode::Traditional, score: 50,   duration: Duration::from_secs_f32(66.0),  players: vec!["#HashTag#".to_string()] }).unwrap();
        #[rustfmt::skip] append_result_to_file(&filename, &GameResult { mode: Mode::Traditional, score: 4000, duration: Duration::from_secs_f32(123.0), players: vec!["Good Player".to_string()] }).unwrap();

        // These should be ignored, but not overwritten
        {
            let mut file = fs::OpenOptions::new().append(true).open(&filename).unwrap();
            file.write_all(b"    # comment line \n").unwrap();
            file.write_all(b"  \n").unwrap();
            file.write_all(b"\n").unwrap();
        }

        #[rustfmt::skip] append_result_to_file(&filename, &GameResult { mode: Mode::Bottle, score: 70, duration: Duration::from_secs_f32(88.0), players: vec!["BottleFoo".to_string()] }).unwrap();
        assert!(read_file(&filename).contains("# comment line"));

        let mut result = read_matching_high_scores(&filename, Mode::Traditional, false).unwrap();
        assert_eq!(
            result,
            vec![
                // Better results come first
                GameResult {
                    mode: Mode::Traditional,
                    score: 4000,
                    duration: Duration::from_secs(123),
                    players: vec!["Good Player".to_string()]
                },
                GameResult {
                    mode: Mode::Traditional,
                    score: 50,
                    duration: Duration::from_secs(66),
                    players: vec!["#HashTag#".to_string()]
                },
                GameResult {
                    mode: Mode::Traditional,
                    score: 10,
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
        assert_eq!(
            read_matching_high_scores(&filename, Mode::Traditional, true).unwrap(),
            vec![GameResult {
                mode: Mode::Traditional,
                score: 30,
                duration: Duration::from_secs(44),
                players: vec![
                    "Alice".to_string(),
                    "Bob".to_string(),
                    "Charlie".to_string()
                ]
            }]
        );

        // Different game mode
        assert_eq!(
            read_matching_high_scores(&filename, Mode::Bottle, false).unwrap(),
            vec![GameResult {
                mode: Mode::Bottle,
                score: 70,
                duration: Duration::from_secs(88),
                players: vec!["BottleFoo".to_string()]
            }]
        );
    }
}
