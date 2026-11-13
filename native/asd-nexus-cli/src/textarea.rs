//! Minimal multi-line text editor (used by the routing config editor).

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextArea {
    pub lines: Vec<String>,
    pub row: usize,
    /// Cursor column in chars.
    pub col: usize,
}

impl TextArea {
    pub fn from_text(s: &str) -> Self {
        let mut lines: Vec<String> = s.lines().map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self { lines, row: 0, col: 0 }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn byte_idx(&self) -> usize {
        let l = &self.lines[self.row];
        l.char_indices().nth(self.col).map(|(i, _)| i).unwrap_or(l.len())
    }

    fn line_len(&self, r: usize) -> usize {
        self.lines[r].chars().count()
    }

    pub fn insert(&mut self, c: char) {
        let i = self.byte_idx();
        self.lines[self.row].insert(i, c);
        self.col += 1;
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
}
