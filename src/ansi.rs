use std::cmp::min;

pub const CLEAR_SCREEN: &str = "\x1b[2J";
pub const CLEAR_TO_END_OF_LINE: &str = "\x1b[0K";
pub const RESET_COLORS: &str = "\x1b[0m";
pub const SHOW_CURSOR: &str = "\x1b[?25h";
pub const HIDE_CURSOR: &str = "\x1b[?25l";

pub fn resize_terminal(width: usize, height: usize) -> String {
    // https://apple.stackexchange.com/a/47841
    format!("\x1b[8;{};{}t", height, width)
}

pub fn move_cursor(x: usize, y: usize) -> String {
    format!("\x1b[{};{}H", y + 1, x + 1)
}

#[derive(Clone, Copy, PartialEq)]
pub struct Color {
    pub fg: u8,
    pub bg: u8,
}
pub const DEFAULT_COLOR: Color = Color { fg: 0, bg: 0 };
pub const RED_FOREGROUND: Color = Color { fg: 31, bg: 0 };
pub const YELLOW_BACKGROUND: Color = Color { fg: 0, bg: 43 };
pub const BLACK_ON_WHITE: Color = Color { fg: 30, bg: 47 };

impl Color {
    pub fn escape_sequence(self) -> String {
        let mut result = RESET_COLORS.to_string();
        if self.fg != 0 {
            result.push_str(&format!("\x1b[1;{}m", self.fg));
        }
        if self.bg != 0 {
            result.push_str(&format!("\x1b[1;{}m", self.bg));
        }
        result
    }
}

#[derive(Debug)]
pub enum KeyPress {
    Up,
    Down,
    Right,
    Left,
    BackSpace,
    Enter,
    Quit,
    RefreshRequest,
    Character(char),
}

const NORMAL_BACKSPACE: u8 = b'\x7f';
const WINDOWS_BACKSPACE: u8 = b'\x08';

const CTRL_C: u8 = b'\x03';
const CTRL_D: u8 = b'\x04';
const CTRL_Q: u8 = b'\x11';
const CTRL_R: u8 = b'\x12';

// The usize is how many bytes were consumed.
pub fn parse_key_press(data: &[u8]) -> Option<(KeyPress, usize)> {
    if data == b"" || data == b"\x1b" || data == b"\x1b[" {
        // Incomplete data: need to receive more
        return None;
    }

    // Arrow keys are 3 bytes each
    if data.len() >= 3 {
        match &data[..3] {
            b"\x1b[A" => return Some((KeyPress::Up, 3)),
            b"\x1b[B" => return Some((KeyPress::Down, 3)),
            b"\x1b[C" => return Some((KeyPress::Right, 3)),
            b"\x1b[D" => return Some((KeyPress::Left, 3)),
            _ => {}
        }
    }

    // Other special things are 1 byte each
    match data[0] {
        b'\r' => return Some((KeyPress::Enter, 1)),
        NORMAL_BACKSPACE | WINDOWS_BACKSPACE => return Some((KeyPress::BackSpace, 1)),
        CTRL_C | CTRL_D | CTRL_Q => return Some((KeyPress::Quit, 1)),
        CTRL_R => return Some((KeyPress::RefreshRequest, 1)),
        _ => {}
    }

    match std::str::from_utf8(data) {
        Ok(s) => {
            let ch = s.chars().next().unwrap();
            return Some((KeyPress::Character(ch), ch.len_utf8()));
        }
        Err(e) if e.valid_up_to() == 0 && e.error_len() == None => {
            // unexpected end of input, need more data to get valid utf-8
            return None;
        }
        Err(e) if e.valid_up_to() == 0 => {
            // data[0] can't possibly be the first byte of utf-8 character, skip it
            return Some((KeyPress::Character(std::char::REPLACEMENT_CHARACTER), 1));
        }
        Err(e) => {
            let ch = std::str::from_utf8(&data[..e.valid_up_to()])
                .unwrap()
                .chars()
                .next()
                .unwrap();
            return Some((KeyPress::Character(ch), ch.len_utf8()));
        }
    }
}
