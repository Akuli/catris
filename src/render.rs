use crate::escapes::Color;
use crate::escapes::TerminalType;
use std::sync::Arc;
use tokio::sync::Notify;

pub struct RenderBuffer {
    pub width: usize,
    pub height: usize,
    chars: Vec<Vec<char>>,
    colors: Vec<Vec<Color>>,
}

impl RenderBuffer {
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            chars: vec![],
            colors: vec![],
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        assert!((width == 0 && height == 0) || (width >= 80 && height >= 24));

        if self.width != width {
            for row in &mut self.chars {
                row.resize(width, ' ');
            }
            for row in &mut self.colors {
                row.resize(width, Color::DEFAULT);
            }
        }

        if self.height != height {
            let mut blank_chars_row = vec![];
            let mut blank_colors_row = vec![];
            blank_chars_row.resize(width, ' ');
            blank_colors_row.resize(width, Color::DEFAULT);
            self.chars.resize(height, blank_chars_row);
            self.colors.resize(height, blank_colors_row);
        }

        self.width = width;
        self.height = height;
    }

    pub fn get_char(&self, x: usize, y: usize) -> char {
        self.chars[y][x]
    }

    #[cfg(test)]
    pub fn get_color(&self, x: usize, y: usize) -> Color {
        self.colors[y][x]
    }

    pub fn set_char(&mut self, x: usize, y: usize, ch: char) {
        self.set_char_with_color(x, y, ch, Color::DEFAULT);
    }
    pub fn set_char_with_color(&mut self, x: usize, y: usize, ch: char, colors: Color) {
        self.chars[y][x] = ch;
        self.colors[y][x] = colors;
    }

    pub fn add_text(&mut self, x: usize, y: usize, text: &str) -> usize {
        self.add_text_with_color(x, y, text, Color::DEFAULT)
    }
    pub fn add_text_with_color(&mut self, x: usize, y: usize, text: &str, color: Color) -> usize {
        let mut x = x;
        for ch in text.chars() {
            self.set_char_with_color(x, y, ch, color);
            x += 1;
        }
        x
    }

    // does not change background colors
    pub fn add_text_with_foreground_color(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        fg: u8,
    ) -> usize {
        let mut x = x;
        for ch in text.chars() {
            self.colors[y][x].fg = fg;
            self.chars[y][x] = ch;
            x += 1;
        }
        x
    }

    pub fn fill_row_with_char(&mut self, y: usize, ch: char) {
        for x in 0..self.width {
            self.chars[y][x] = ch;
        }
    }

    pub fn set_row_color(&mut self, y: usize, color: Color) {
        for x in 0..self.width {
            self.colors[y][x] = color;
        }
    }

    // returns start and end of range of x coordinates where text ended up
    pub fn add_centered_text(&mut self, y: usize, text: &str) -> (usize, usize) {
        self.add_centered_text_with_color(y, text, Color::DEFAULT)
    }
    pub fn add_centered_text_with_color(
        &mut self,
        y: usize,
        text: &str,
        colors: Color,
    ) -> (usize, usize) {
        let n = text.chars().count();
        let x = self.width / 2 - n / 2;
        self.add_text_with_color(x, y, text, colors);
        (x, x + n)
    }

    pub fn clear(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.set_char(x, y, ' ');
            }
        }
    }

    pub fn copy_into(&self, dest: &mut RenderBuffer) {
        dest.resize(self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                dest.chars[y][x] = self.chars[y][x];
                dest.colors[y][x] = self.colors[y][x];
            }
        }
    }

    fn clear_and_render_entire_screen(&self, terminal_type: TerminalType) -> String {
        let mut current_color = Color::DEFAULT;
        let mut result = "".to_string();

        result.push_str(&terminal_type.resize(self.width, self.height));
        result.push_str(terminal_type.clear());
        for y in 0..self.height {
            result.push_str(&terminal_type.move_cursor(0, y));
            for x in 0..self.width {
                if self.colors[y][x] != current_color && terminal_type.has_color() {
                    current_color = self.colors[y][x];
                    result.push_str(&terminal_type.format_color(current_color));
                }
                result.push(self.chars[y][x]);
            }
        }
        if current_color != Color::DEFAULT {
            result.push_str(terminal_type.reset_colors());
        }
        result
    }

    fn get_updates_for_changes_only(
        &self,
        terminal_type: TerminalType,
        old: &RenderBuffer,
        cursor_pos: Option<(usize, usize)>,
    ) -> String {
        let mut result = "".to_string();
        let cursor_y = match cursor_pos {
            Some((_, y)) => y,
            None => self.height - 1,
        };

        for y in 0..self.height {
            // Output nothing for unchanged lines, but consider cursor line potentially changed.
            // This way we wipe away the character typed by user.
            if self.chars[y] == old.chars[y] && self.colors[y] == old.colors[y] && y != cursor_y {
                continue;
            }

            // Use ansi::CLEAR_TO_END_OF_LINE instead of spaces when possible
            let mut end = self.width;
            while end > 0
                && self.chars[y][end - 1] == ' '
                && self.colors[y][end - 1] == Color::DEFAULT
            {
                end -= 1;
            }

            let mut current_color = Color::DEFAULT;
            let mut cursor_at_xy = false;
            for x in 0..end {
                if self.colors[y][x] == old.colors[y][x] && self.chars[y][x] == old.chars[y][x] {
                    // skip redrawing this charater
                    cursor_at_xy = false;
                } else {
                    if !cursor_at_xy {
                        result.push_str(&terminal_type.move_cursor(x, y));
                        cursor_at_xy = true;
                    }
                    if self.colors[y][x] != current_color && terminal_type.has_color() {
                        result.push_str(&terminal_type.format_color(self.colors[y][x]));
                        current_color = self.colors[y][x];
                    }
                    result.push(self.chars[y][x]);
                }
            }
            if current_color != Color::DEFAULT {
                result.push_str(&terminal_type.reset_colors());
            }
            if !cursor_at_xy {
                result.push_str(&terminal_type.move_cursor(end, y));
            }
            result.push_str(&terminal_type.clear_from_cursor_to_end_of_line());
        }
        result
    }

    pub fn get_updates_as_escape_codes(
        &self,
        terminal_type: TerminalType,
        old: &RenderBuffer,
        cursor_pos: Option<(usize, usize)>,
        force_redraw: bool,
    ) -> String {
        let mut result = if self.width != old.width || self.height != old.height || force_redraw {
            self.clear_and_render_entire_screen(terminal_type)
        } else {
            self.get_updates_for_changes_only(terminal_type, old, cursor_pos)
        };

        match cursor_pos {
            None => {
                result.push_str(&terminal_type.move_cursor(0, self.height - 1));
                result.push_str(terminal_type.hide_cursor());
            }
            Some((x, y)) => {
                result.push_str(&terminal_type.move_cursor(x, y));
                result.push_str(terminal_type.show_cursor());
            }
        }

        result
    }
}

pub struct RenderData {
    pub buffer: RenderBuffer,
    pub cursor_pos: Option<(usize, usize)>,
    pub changed: Arc<Notify>,
    pub force_redraw: bool,
}

impl RenderData {
    pub fn clear(&mut self, width: usize, height: usize) {
        self.buffer.clear();
        self.buffer.resize(width, height);
        self.cursor_pos = None;
    }
}
