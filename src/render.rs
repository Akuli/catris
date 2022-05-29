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
    colors: Vec<Vec<ansi::Colors>>,
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
                row.resize(width, ansi::Colors { fg: 0, bg: 0 });
            }
        }

        if self.height != height {
            let mut blank_chars_row = vec![];
            let mut blank_colors_row = vec![];
            blank_chars_row.resize(width, ' ');
            blank_colors_row.resize(width, ansi::Colors { fg: 0, bg: 0 });
            self.chars.resize(height, blank_chars_row);
            self.colors.resize(height, blank_colors_row);
        }

        self.width = width;
        self.height = height;
    }

    pub fn set_char(&mut self, x: usize, y: usize, ch: char, colors: ansi::Colors) {
        self.chars[y][x] = ch;
        self.colors[y][x] = colors;
    }

    pub fn add_text(&mut self, x: usize, y: usize, text: String, colors: ansi::Colors) {
        let mut x = x;
        for ch in text.chars() {
            self.set_char(x, y, ch, colors.clone());
            x += 1;
        }
    }

    pub fn clear(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.add_text(x, y, " ".to_string(), ansi::Colors { fg: 0, bg: 0 });
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

    pub fn get_updates_as_ansi_codes(&self, old: &Buffer) -> String {
        let mut result = "".to_string();
        let mut current_color = ansi::Colors { fg: 0, bg: 0 };

        if self.width != old.width || self.height != old.height {
            // re-render everything
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
        } else {
            // re-render changed part
            for y in 0..self.height {
                let mut cursor_at_xy = false;
                for x in 0..self.width {
                    if self.colors[y][x] == old.colors[y][x] && self.chars[y][x] == old.chars[y][x]
                    {
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
            }
        }

        if current_color != (ansi::Colors { fg: 0, bg: 0 }) {
            result.push_str(&ansi::RESET_COLORS);
        }
        result
    }
}

pub struct RenderData {
    pub buffer: Buffer,
    pub cursor_pos: Option<(usize, usize)>,
    pub changed: Arc<Notify>,
}
