use crate::FileType;
use crate::Position;
use crate::Row;
use crate::SearchDirection;
use std::fs;
use std::io::{Error, Write};
use std::path::Path;

#[derive(Default)]
pub struct Document {
    rows: Vec<Row>,
    pub file_name: Option<String>,
    dirty: bool,
    file_type: FileType,
}

impl Clone for Document {
    fn clone(&self) -> Self {
        Self {
            rows: self.rows.clone(),
            file_name: self.file_name.clone(),
            dirty: false,
            file_type: self.file_type.clone(),
        }
    }
}

impl Document {
    pub fn open(filename: &str) -> Result<Self, std::io::Error> {
        let contents = fs::read_to_string(filename)?;
        let file_type = FileType::from(filename);
        let mut rows = Vec::new();
        for value in contents.lines() {
            rows.push(Row::from(value));
        }
        Ok(Self {
            rows,
            file_name: Some(filename.to_string()),
            dirty: false,
            file_type,
        })
    }

    pub fn file_type(&self) -> String {
        self.file_type.name()
    }

    pub fn row(&self, index: usize) -> Option<&Row> {
        self.rows.get(index)
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    fn insert_newline(&mut self, at: &Position) {
        if at.y > self.rows.len() {
            return;
        }
        if at.y == self.rows.len() {
            self.rows.push(Row::default());
            return;
        }
        self.dirty = true;
        #[allow(clippy::indexing_slicing)]
        let current_row = &mut self.rows[at.y];
        let new_row = current_row.split(at.x);
        #[allow(clippy::integer_arithmetic)]
        self.rows.insert(at.y + 1, new_row);
    }

    pub fn insert(&mut self, at: &Position, c: char) {
        if at.y > self.rows.len() {
            return;
        }
        self.dirty = true;
        if c == '\n' {
            self.insert_newline(at);
        } else if at.y == self.rows.len() {
            let mut row = Row::default();
            row.insert(0, c);
            self.rows.push(row);
        } else {
            #[allow(clippy::indexing_slicing)]
            let row = &mut self.rows[at.y];
            row.insert(at.x, c);
        }
        self.unhighlight_rows(at.y);
    }

    pub fn insert_line(&mut self, y: usize, line: &str) {
        if y > self.rows.len() {
            return;
        }
        self.dirty = true;
        self.rows.splice(y..y, vec![line.into()]);
        self.unhighlight_rows(y);
    }

    pub fn replace(&mut self, at: &Position, c: char) {
        self.delete(at);
        self.insert(at, c);
    }

    fn unhighlight_rows(&mut self, start: usize) {
        let start = start.saturating_sub(1);
        for row in self.rows.iter_mut().skip(start) {
            row.is_highlighted = false;
        }
    }

    #[allow(clippy::integer_arithmetic, clippy::indexing_slicing)]
    pub fn delete(&mut self, at: &Position) -> usize {
        let mut deleted = 0;
        let len = self.rows.len();
        if at.y >= len {
            return deleted;
        }
        self.dirty = true;
        if at.x == self.rows[at.y].len() && at.y + 1 < len {
            let next_row = self.rows.remove(at.y + 1);
            let row = &mut self.rows[at.y];
            row.append(&next_row);
        } else {
            let row = &mut self.rows[at.y];
            deleted = row.delete(at.x);
        }
        self.unhighlight_rows(at.y);
        deleted
    }

    pub fn delete_line(&mut self, y: usize) {
        if y > self.rows.len() {
            return;
        }
        self.dirty = true;
        self.rows.splice(y..=y, vec![]);
        self.unhighlight_rows(y);
    }

    pub fn save(&mut self) -> Result<usize, Error> {
        let mut bytes_written = 0;
        if let Some(file_name) = &self.file_name {
            let file_exists = Path::new(file_name).exists();
            let mut new_file_name = file_name.clone();
            if file_exists {
                new_file_name.push_str(".new")
            }
            let mut file = fs::File::create(&new_file_name)?;
            self.file_type = FileType::from(file_name);
            for row in &mut self.rows {
                let row_bytes = row.as_bytes();
                file.write_all(row_bytes)?;
                file.write_all(b"\n")?;
                bytes_written += row_bytes.len() + 1;
                row.save();
            }
            if file_exists {
                let mut old_file_name = file_name.clone();
                old_file_name.push_str(".old");
                fs::rename(file_name, &old_file_name)?;
                fs::rename(&new_file_name, file_name)?;
                fs::remove_file(&old_file_name)?;
            }
            self.dirty = false;
        }
        Ok(bytes_written)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[allow(clippy::indexing_slicing)]
    pub fn find(&self, query: &str, at: &Position, direction: SearchDirection) -> Option<Position> {
        if at.y >= self.rows.len() {
            return None;
        }
        let mut position = Position { x: at.x, y: at.y };

        let start = if direction == SearchDirection::Forward {
            at.y
        } else {
            0
        };
        let end = if direction == SearchDirection::Forward {
            self.rows.len()
        } else {
            at.y.saturating_add(1)
        };
        for _ in start..end {
            if let Some(row) = self.rows.get(position.y) {
                if let Some(x) = row.find(query, position.x, direction) {
                    position.x = x;
                    return Some(position);
                }
                if direction == SearchDirection::Forward {
                    position.y = position.y.saturating_add(1);
                    position.x = 0;
                } else {
                    position.y = position.y.saturating_sub(1);
                    position.x = self.rows[position.y].len();
                }
            } else {
                return None;
            }
        }
        None
    }

    pub fn highlight(&mut self, word: &Option<String>, until: Option<usize>) {
        let mut start_with_comment = false;
        let until = if let Some(until) = until {
            if until.saturating_add(1) < self.rows.len() {
                until.saturating_add(1)
            } else {
                self.rows.len()
            }
        } else {
            self.rows.len()
        };
        #[allow(clippy::indexing_slicing)]
        for row in &mut self.rows[..until] {
            start_with_comment = row.highlight(
                self.file_type.highlighting_options(),
                word,
                start_with_comment,
            );
        }
    }
}
