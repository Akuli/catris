use std::fmt::Write;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TerminalType {
    Ansi, // Modern terminals https://en.wikipedia.org/wiki/ANSI_escape_code
    VT52, // Older devices https://en.wikipedia.org/wiki/VT52
}

impl TerminalType {
    // Location of cursor after clearing depends on the terminal mode.
    pub fn clear(&self) -> &str {
        match self {
            Self::Ansi => "\x1b[2J",    // clear without moving cursor
            Self::VT52 => "\x1bH\x1bJ", // move cursor to top left + clear to end of screen
        }
    }

    pub fn clear_from_cursor_to_end_of_line(&self) -> &str {
        match self {
            Self::Ansi => "\x1b[0K", Self::VT52 => "\x1bK",
        }
    }

    pub fn clear_from_cursor_to_end_of_screen(&self) -> &str {
        match self {
            Self::Ansi => "\x1b[0J",
            Self::VT52 => "\x1bJ",
        }
    }

    pub fn show_cursor(&self) -> &str {
        match self {
            Self::Ansi => "\x1b[?25h",
            Self::VT52 => "\x1be", // extension, not all terminals support
        }
    }

    pub fn hide_cursor(&self) -> &str {
        match self {
            Self::Ansi => "\x1b[?25l",
            Self::VT52 => "\x1bf", // extension, not all terminals support
        }
    }

    pub fn resize(&self, width: usize, height: usize) -> String {
        match self {
            Self::Ansi => format!("\x1b[8;{};{}t", height, width), // https://apple.stackexchange.com/a/47841
            Self::VT52 => "".to_string(),
        }
    }

    pub fn move_cursor(&self, x: usize, y: usize) -> String {
        match self {
            Self::Ansi => format!("\x1b[{};{}H", y + 1, x + 1),
            Self::VT52 => {
                // Top left is "\x1bY  " where space is ascii character 32.
                // Other locations increment the ascii character values.
                if let Some(x_char) = char::from_u32((x as u32) + 32) {
                    if let Some(y_char) = char::from_u32((y as u32) + 32) {
                        return format!("\x1bY{}{}", y_char, x_char);
                    }
                }
                "".to_string()
            }
        }
    }

    pub fn move_cursor_to_leftmost_column(&self) -> &str {
        "\r"
    }

    pub fn has_color(&self) -> bool {
        match self {
            Self::Ansi => true,
            Self::VT52 => false,
        }
    }

    pub fn reset_colors(&self) -> &str {
        match self {
            Self::Ansi => "\x1b[0m",
            Self::VT52 => "", // no colors
        }
    }

    pub fn format_color(&self, color: Color) -> String {
        match self {
            Self::Ansi => {
                let mut result = self.reset_colors().to_string();
                if color.fg != 0 {
                    let _ = write!(result, "\x1b[1;{}m", color.fg);
                }
                if color.bg != 0 {
                    let _ = write!(result, "\x1b[1;{}m", color.bg);
                }
                result
            }
            Self::VT52 => "".to_string(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Color {
    pub fg: u8,
    pub bg: u8,
}
impl Color {
    pub const DEFAULT: Color = Color { fg: 0, bg: 0 };
    pub const BLACK_ON_WHITE: Color = Color { fg: 30, bg: 47 };
    pub const GRAY_FOREGROUND: Color = Color { fg: 90, bg: 0 };
    pub const GRAY_BACKGROUND: Color = Color { fg: 0, bg: 100 };

    pub const RED_FOREGROUND: Color = Color { fg: 31, bg: 0 };
    pub const GREEN_FOREGROUND: Color = Color { fg: 32, bg: 0 };
    pub const YELLOW_FOREGROUND: Color = Color { fg: 33, bg: 0 };
    pub const BLUE_FOREGROUND: Color = Color { fg: 34, bg: 0 };
    pub const MAGENTA_FOREGROUND: Color = Color { fg: 35, bg: 0 };
    pub const CYAN_FOREGROUND: Color = Color { fg: 36, bg: 0 };
    //pub const WHITE_FOREGROUND: Color = Color { fg: 37, bg: 0 };

    pub const RED_BACKGROUND: Color = Color { fg: 0, bg: 41 };
    pub const GREEN_BACKGROUND: Color = Color { fg: 0, bg: 42 };
    pub const YELLOW_BACKGROUND: Color = Color { fg: 0, bg: 43 };
    pub const BLUE_BACKGROUND: Color = Color { fg: 0, bg: 44 };
    pub const MAGENTA_BACKGROUND: Color = Color { fg: 0, bg: 45 };
    pub const CYAN_BACKGROUND: Color = Color { fg: 0, bg: 46 };
    pub const WHITE_BACKGROUND: Color = Color { fg: 0, bg: 47 };
}

#[derive(Debug, PartialEq)]
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

    // VT52 arrow keys: 2 bytes each
    if data.len() >= 2 {
        match &data[..2] {
            b"\x1bA" => return Some((KeyPress::Up, 2)),
            b"\x1bB" => return Some((KeyPress::Down, 2)),
            b"\x1bC" => return Some((KeyPress::Right, 2)),
            b"\x1bD" => return Some((KeyPress::Left, 2)),
            _ => {}
        }
    }

    // ANSI arrow keys: 3 bytes each
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
            Some((KeyPress::Character(ch), ch.len_utf8()))
        }
        Err(e) if e.valid_up_to() == 0 && e.error_len() == None => {
            // unexpected end of input, need more data to get valid utf-8
            None
        }
        Err(e) if e.valid_up_to() == 0 => {
            // data[0] can't possibly be the first byte of utf-8 character, skip it
            Some((KeyPress::Character(std::char::REPLACEMENT_CHARACTER), 1))
        }
        Err(e) => {
            let ch = std::str::from_utf8(&data[..e.valid_up_to()])
                .unwrap()
                .chars()
                .next()
                .unwrap();
            Some((KeyPress::Character(ch), ch.len_utf8()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_press() {
        // arrow keys
        assert_eq!(parse_key_press(b"\x1bBasd"), Some((KeyPress::Down, 2)));
        assert_eq!(parse_key_press(b"\x1b[Basd"), Some((KeyPress::Down, 3)));

        // arrow keys: incomplete/bad
        assert_eq!(parse_key_press(b""), None);
        assert_eq!(parse_key_press(b"\x1b"), None);
        assert_eq!(parse_key_press(b"\x1b["), None);
        assert_eq!(parse_key_press(b"\x1b[A"), Some((KeyPress::Up, 3)));
        assert_eq!(parse_key_press(b"\x1b[Axxx"), Some((KeyPress::Up, 3)));
        assert_eq!(
            parse_key_press(b"[Axxx"),
            Some((KeyPress::Character('['), 1))
        );

        // incomplete utf-8
        assert_eq!(parse_key_press(b"\xe2"), None);
        assert_eq!(parse_key_press(b"\xe2\x82"), None);
        assert_eq!(
            parse_key_press(b"\xe2\x82\xac"),
            Some((KeyPress::Character('€'), 3))
        );

        // invalid utf-8: consume first byte to allow retrying with the rest
        assert_eq!(
            parse_key_press(b"\xe2\xe2"),
            Some((KeyPress::Character(std::char::REPLACEMENT_CHARACTER), 1))
        );
        assert_eq!(
            parse_key_press(b"\x82\xac"),
            Some((KeyPress::Character(std::char::REPLACEMENT_CHARACTER), 1))
        );

        assert_eq!(
            parse_key_press(b"John"),
            Some((KeyPress::Character('J'), 1))
        );
        assert_eq!(
            parse_key_press("Örkki".as_bytes()),
            Some((KeyPress::Character('Ö'), 2))
        );
        assert_eq!(parse_key_press(b"\r"), Some((KeyPress::Enter, 1)));
    }
}
