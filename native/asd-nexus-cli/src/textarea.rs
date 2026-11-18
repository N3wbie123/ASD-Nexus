//! Multi-line text editor used by the prompt input box and routing config editor.

#[derive(Debug, Clone, PartialEq)]
pub struct TextArea {
    pub lines: Vec<String>,
    pub row: usize,
    /// Cursor column in chars.
    pub col: usize,
}

impl Default for TextArea {
    fn default() -> Self {
        Self::new()
    }
}

impl TextArea {
    pub fn new() -> Self {
        Self { lines: vec![String::new()], row: 0, col: 0 }
    }

    pub fn from_text(s: &str) -> Self {
        let mut lines: Vec<String> = if s.is_empty() {
            vec![String::new()]
        } else {
            s.split('\n').map(str::to_string).collect()
        };
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self { lines, row: 0, col: 0 }
    }

    pub fn set_text(&mut self, s: &str) {
        self.lines = if s.is_empty() {
            vec![String::new()]
        } else {
            s.split('\n').map(str::to_string).collect()
        };
        self.row = self.lines.len().saturating_sub(1);
        self.col = self.line_len(self.row);
    }

    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.row = 0;
        self.col = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn byte_idx_at(&self, row: usize, col: usize) -> usize {
        let l = &self.lines[row];
        l.char_indices().nth(col).map(|(i, _)| i).unwrap_or(l.len())
    }

    fn byte_idx(&self) -> usize {
        self.byte_idx_at(self.row, self.col)
    }

    pub fn line_len(&self, r: usize) -> usize {
        self.lines.get(r).map(|l| l.chars().count()).unwrap_or(0)
    }

    pub fn insert(&mut self, c: char) {
        if c == '\n' {
            self.newline();
            return;
        }
        let i = self.byte_idx();
        self.lines[self.row].insert(i, c);
        self.col += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert(c);
        }
    }

    pub fn newline(&mut self) {
        let i = self.byte_idx();
        let rest = self.lines[self.row].split_off(i);
        self.row += 1;
        self.lines.insert(self.row, rest);
        self.col = 0;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            let i = self.byte_idx();
            self.lines[self.row].remove(i);
        } else if self.row > 0 {
            let cur = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.line_len(self.row);
            self.lines[self.row].push_str(&cur);
        }
    }

    pub fn delete(&mut self) {
        let len = self.line_len(self.row);
        if self.col < len {
            let i = self.byte_idx();
            self.lines[self.row].remove(i);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    pub fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.line_len(self.row);
        }
    }

    pub fn right(&mut self) {
        if self.col < self.line_len(self.row) {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub fn up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.col = self.col.min(self.line_len(self.row));
        }
    }

    pub fn down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.col.min(self.line_len(self.row));
        }
    }

    pub fn home(&mut self) {
        self.col = 0;
    }

    pub fn end(&mut self) {
        self.col = self.line_len(self.row);
    }

    /// Detect `@<query>` token ending at the current cursor position.
    /// Returns `(at_char_col, cursor_char_col, query_without_at)`.
    pub fn active_at_token(&self) -> Option<(usize, usize, String)> {
        let line = self.lines.get(self.row)?;
        let chars: Vec<char> = line.chars().take(self.col).collect();
        let mut at_pos = None;
        for (idx, ch) in chars.iter().enumerate().rev() {
            if ch.is_whitespace() {
                break;
            }
            if *ch == '@' {
                if idx == 0 || chars[idx - 1].is_whitespace() {
                    at_pos = Some(idx);
                }
                break;
            }
        }
        let start = at_pos?;
        let query: String = chars[start + 1..].iter().collect();
        Some((start, self.col, query))
    }

    /// Replace char range `[start_col..end_col]` in current row with `replacement`.
    pub fn replace_current_line_span(&mut self, start_col: usize, end_col: usize, replacement: &str) {
        let start_b = self.byte_idx_at(self.row, start_col);
        let end_b = self.byte_idx_at(self.row, end_col);
        self.lines[self.row].replace_range(start_b..end_b, replacement);
        self.col = start_col + replacement.chars().count();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_roundtrip() {
        let mut t = TextArea::from_text("{\n}");
        t.end();
        t.newline();
        for c in "\"a\":1".chars() {
            t.insert(c);
        }
        assert_eq!(t.text(), "{\n\"a\":1\n}");
        t.home();
        t.backspace();
        assert_eq!(t.text(), "{\"a\":1\n}");
        t.right();
        t.down();
        t.left();
        assert_eq!((t.row, t.col), (1, 0));
        t.left();
        assert_eq!((t.row, t.col), (0, 6));
    }

    #[test]
    fn at_token_detection_and_replace() {
        let mut t = TextArea::new();
        t.insert_str("check @src/ma");
        assert_eq!(t.active_at_token(), Some((6, 13, "src/ma".into())));
        t.replace_current_line_span(6, 13, "@src/main.rs ");
        assert_eq!(t.text(), "check @src/main.rs ");
        assert_eq!(t.active_at_token(), None);
    }
}
