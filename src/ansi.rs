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
