use crate::ansi;

pub struct RenderBuffer {
    width: usize,
    height: usize,
    chars: Vec<Vec<char>>,
    colors: Vec<Vec<ansi::Colors>>,
}

impl RenderBuffer {
    pub fn new() -> RenderBuffer {
        RenderBuffer {
            width: 0,
            height: 0,
            chars: vec![],
            colors: vec![],
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
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

    pub fn set_text(
        &mut self,
        x: usize,
        y: usize,
        chars: &mut dyn Iterator<Item = char>,
        colors: ansi::Colors,
    ) {
        let mut x = x;
        for ch in chars {
            self.chars[y][x] = ch;
            self.colors[y][x] = colors.clone();
            x += 1;
        }
    }

    pub fn clear(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.set_text(x, y, &mut " ".chars(), ansi::Colors { fg: 0, bg: 0 });
            }
        }
    }

    pub fn get_updates_as_ansi_codes(&mut self, old: &RenderBuffer) -> String {
        let mut result = "".to_string();
        let mut current_color = ansi::Colors { fg: 0, bg: 0 };

        if self.width != old.width || self.height != old.height {
            // re-render everything bruh
            result.push_str(&ansi::CLEAR_SCREEN);
            result.push_str(&ansi::move_cursor(0, 0));
            for y in 0..self.height {
                for x in 0..self.width {
                    if self.colors[y][x] != current_color {
                        result.push_str(&current_color.escape_sequence());
                        current_color = self.colors[y][x];
                    }
                    result.push(self.chars[y][x]);
                }
                result.push_str("\r\n");
            }
        } else {
            // re-render changed part
            let mut cursor_at_xy = false;
            for y in 0..self.height {
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
                        }
                        result.push(self.chars[y][x]);
                    }
                }
            }
        }

        result.push_str(&ansi::RESET_COLORS);
        result
    }
}
