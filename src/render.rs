use std::io;
use std::io::Write;
use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio::time::sleep;
use weak_table::WeakValueHashMap;

use crate::ansi;

pub struct Buffer {
    pub width: usize,
    pub height: usize,
    chars: Vec<Vec<char>>,
    colors: Vec<Vec<ansi::Color>>,
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer {
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
                row.resize(width, ansi::DEFAULT_COLOR);
            }
        }

        if self.height != height {
            let mut blank_chars_row = vec![];
            let mut blank_colors_row = vec![];
            blank_chars_row.resize(width, ' ');
            blank_colors_row.resize(width, ansi::DEFAULT_COLOR);
            self.chars.resize(height, blank_chars_row);
            self.colors.resize(height, blank_colors_row);
        }

        self.width = width;
        self.height = height;
    }

    pub fn set_char(&mut self, x: usize, y: usize, ch: char) {
        self.set_char_with_color(x, y, ch, ansi::DEFAULT_COLOR);
    }
    pub fn set_char_with_color(&mut self, x: usize, y: usize, ch: char, colors: ansi::Color) {
        self.chars[y][x] = ch;
        self.colors[y][x] = colors;
    }

    pub fn add_text(&mut self, x: usize, y: usize, text: &str) -> usize {
        self.add_text_with_color(x, y, text, ansi::DEFAULT_COLOR)
    }
    pub fn add_text_with_color(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        colors: ansi::Color,
    ) -> usize {
        let mut x = x;
        for ch in text.chars() {
            self.set_char_with_color(x, y, ch, colors);
            x += 1;
        }
        return x;
    }

    pub fn add_centered_text(&mut self, y: usize, text: &str) {
        self.add_centered_text_with_color(y, text, ansi::DEFAULT_COLOR);
    }
    pub fn add_centered_text_with_color(&mut self, y: usize, text: &str, colors: ansi::Color) {
        let n = text.chars().count();
        let x = self.width / 2 - n / 2;
        self.add_text_with_color(x, y, text, colors);
    }

    pub fn clear(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.set_char(x, y, ' ');
            }
        }
    }

    pub fn copy_into(&self, dest: &mut Buffer) {
        dest.resize(self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                dest.chars[y][x] = self.chars[y][x];
                dest.colors[y][x] = self.colors[y][x];
            }
        }
    }

    fn clear_and_render_entire_screen(&self) -> String {
        let mut current_color = ansi::DEFAULT_COLOR;
        let mut result = "".to_string();

        result.push_str(&ansi::resize_terminal(self.width, self.height));
        result.push_str(&ansi::CLEAR_SCREEN);
        for y in 0..self.height {
            result.push_str(&ansi::move_cursor(0, y));
            for x in 0..self.width {
                if self.colors[y][x] != current_color {
                    current_color = self.colors[y][x];
                    result.push_str(&current_color.escape_sequence());
                }
                result.push(self.chars[y][x]);
            }
        }
        if current_color != ansi::DEFAULT_COLOR {
            result.push_str(&ansi::RESET_COLORS);
        }
        result
    }

    fn get_updates_for_what_changed(
        &self,
        old: &Buffer,
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
            if self.chars[y] == old.chars[y]
                && self.colors[y] == old.colors[y]
                && y != cursor_y
            {
                continue;
            }

            // Use ansi::CLEAR_TO_END_OF_LINE instead of spaces when possible
            let mut end = self.width;
            while end > 0
                && self.chars[y][end - 1] == ' '
                && self.colors[y][end - 1] == ansi::DEFAULT_COLOR
            {
                end -= 1;
            }

            let mut current_color = ansi::DEFAULT_COLOR;
            let mut cursor_at_xy = false;
            for x in 0..end {
                if self.colors[y][x] == old.colors[y][x] && self.chars[y][x] == old.chars[y][x] {
                    // skip redrawing this charater
                    cursor_at_xy = false;
                } else {
                    if !cursor_at_xy {
                        result.push_str(&ansi::move_cursor(x, y));
                        cursor_at_xy = true;
                    }
                    if self.colors[y][x] != current_color {
                        result.push_str(&self.colors[y][x].escape_sequence());
                        current_color = self.colors[y][x];
                    }
                    result.push(self.chars[y][x]);
                }
            }
            if current_color != ansi::DEFAULT_COLOR {
                result.push_str(&ansi::RESET_COLORS);
            }
            if !cursor_at_xy {
                result.push_str(&ansi::move_cursor(end, y));
            }
            result.push_str(ansi::CLEAR_TO_END_OF_LINE);
        }
        result
    }

    pub fn get_updates_as_ansi_codes(
        &self,
        old: &Buffer,
        cursor_pos: Option<(usize, usize)>,
    ) -> String {
        let mut result;

        if self.width != old.width || self.height != old.height {
            result = self.clear_and_render_entire_screen();
        } else {
            result = self.get_updates_for_what_changed(old, cursor_pos);
        }

        match cursor_pos {
            None => {
                result.push_str(&ansi::move_cursor(0, self.height - 1));
                result.push_str(ansi::HIDE_CURSOR);
            }
            Some((x, y)) => {
                result.push_str(&ansi::move_cursor(x, y));
                result.push_str(ansi::SHOW_CURSOR);
            }
        }

        result
    }
}

pub struct RenderData {
    pub buffer: Buffer,
    pub cursor_pos: Option<(usize, usize)>,
    pub changed: Arc<Notify>,
}
