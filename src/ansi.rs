use std::cmp::min;

pub const CLEAR_SCREEN: &str = "\x1b[2J";
pub const RESET_COLORS: &str = "\x1b[0m";

pub fn resize_terminal(width: usize, height: usize) -> String {
    // https://apple.stackexchange.com/a/47841
    format!("\x1b[8;{};{}t", height, width)
}

pub fn move_cursor(x: usize, y: usize) -> String {
    format!("\x1b[{};{}H", y + 1, x + 1)
}

#[derive(Clone, Copy, PartialEq)]
pub struct Colors {
    // 0 means use default color
    pub fg: u8,
    pub bg: u8,
}

impl Colors {
    pub fn escape_sequence(self) -> String {
        let mut result = RESET_COLORS.to_string();
        if self.fg != 0 {
            result.push_str(&format!("\x1b[1;{}m", self.fg));
        }
        if self.bg != 0 {
            result.push_str(&format!("\x1b[1;{}m", self.bg));
        }
        return result;
    }
}

#[derive(Debug)]
pub enum KeyPress {
    Up,
    Down,
    Right,
    Left,
    BackSpace,
    Quit,
    RefreshRequest,
    Character(char),
}

const NORMAL_BACKSPACE: &[u8] = b"\x7f";
const WINDOWS_BACKSPACE: &[u8] = b"\x08";

const CTRL_C: &[u8] = b"\x03";
const CTRL_D: &[u8] = b"\x04";
const CTRL_Q: &[u8] = b"\x11";
const CTRL_R: &[u8] = b"\x12";

// Returning None means need to receive more data.
// The usize is how many bytes were consumed.
pub fn parse_key_press(data: &[u8]) -> Option<(KeyPress, usize)> {
    match data {
        b"" | b"\x1b" | b"\x1b[" => None,
        b"\x1b[A" => Some((KeyPress::Up, 3)),
        b"\x1b[B" => Some((KeyPress::Down, 3)),
        b"\x1b[C" => Some((KeyPress::Right, 3)),
        b"\x1b[D" => Some((KeyPress::Left, 3)),
        NORMAL_BACKSPACE | WINDOWS_BACKSPACE => Some((KeyPress::BackSpace, 1)),
        CTRL_C | CTRL_D | CTRL_Q => Some((KeyPress::Quit, 1)),
        CTRL_R => Some((KeyPress::RefreshRequest, 1)),
        // utf-8 chars are never >4 bytes long
        _ => match std::str::from_utf8(&data[0..min(data.len(), 4)]) {
            Ok(s) => {
                let ch = s.chars().next().unwrap();
                Some((KeyPress::Character(ch), ch.to_string().len()))
            }
            // error_len() == None means unexpected end of input, i.e. need more data
            Err(e) if e.valid_up_to() == 0 && e.error_len() == None => None,
            Err(e) if e.valid_up_to() == 0 => {
                Some((KeyPress::Character(std::char::REPLACEMENT_CHARACTER), 1))
            }
            Err(e) => {
                let ch = std::str::from_utf8(&data[..e.valid_up_to()])
                    .unwrap()
                    .chars()
                    .next()
                    .unwrap();
                Some((KeyPress::Character(ch), ch.to_string().len()))
            }
        },
    }
}
