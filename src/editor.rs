use crate::Document;
use crate::Row;
use crate::Terminal;
use anyhow::Result;
use arboard::Clipboard;
use chrono;
use chrono::DateTime;
use chrono::Local;
use std::env;
use std::path::Path;
use std::thread;
use std::time;
use std::time::Duration;
use std::time::Instant;
use termion::color;
use termion::event::Event::{Key as KeyEvent, Mouse};
use termion::event::{Key, MouseButton, MouseEvent};
use unicode_segmentation::UnicodeSegmentation;

const STATUS_FG_COLOR: color::Rgb = color::Rgb(63, 63, 63);
const STATUS_BG_COLOR: color::Rgb = color::Rgb(239, 239, 239);
const VERSION: &str = env!("CARGO_PKG_VERSION");
const REFRESH_INTERVAL: u64 = 1000 / 120;

#[derive(PartialEq, Copy, Clone)]
pub enum SearchDirection {
    Forward,
    Backward,
}

#[derive(PartialEq, Copy, Clone)]
enum Mode {
    Insert,
    Normal,
    Replace,
    Visual,
}

#[derive(Default, Copy, Clone)]
struct CursorPosition {
    x: usize,
    y: usize,
    max_x: usize,
}

#[derive(Default, Copy, Clone)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

impl From<Position> for CursorPosition {
    fn from(item: Position) -> Self {
        Self {
            x: item.x,
            y: item.y,
            max_x: 0,
        }
    }
}

impl Into<Position> for CursorPosition {
    fn into(self) -> Position {
        Position {
            x: self.x,
            y: self.y,
        }
    }
}

struct StatusMessage {
    time: Instant,
    text: String,
}

#[derive(Default, Clone)]
struct Version {
    document: Document,
    position: CursorPosition,
    timestamp: DateTime<Local>,
}

impl StatusMessage {
    fn from(message: String) -> Self {
        Self {
            time: Instant::now(),
            text: message,
        }
    }
}

pub struct Editor {
    should_quit: bool,
    terminal: Terminal,
    cursor_position: CursorPosition,
    selection_start: Position,
    offset: Position,
    document: Document,
    status_message: StatusMessage,
    highlighted_word: Option<String>,
    clipboard: Option<String>,
    mode: Mode,
    versions: Vec<Version>,
    version_index: usize,
    has_saved: bool,
    query: Option<String>,
}

impl Editor {
    pub fn run(&mut self) -> Result<()> {
        loop {
            self.refresh_screen()?;
            if self.should_quit {
                break;
            }
            if let Some(res) = self.term_read_event() {
                res?;
                continue;
            }
            thread::sleep(time::Duration::from_millis(REFRESH_INTERVAL));
        }
        Ok(())
    }

    pub fn default() -> Self {
        let args: Vec<String> = env::args().collect();
        let mut initial_status = String::from(": for commands");
        let (document, versions) = if let Some(file_name) = args.get(1) {
            if let Some((doc, versions)) = Self::open_document(&file_name) {
                (doc, versions)
            } else {
                initial_status = format!("ERR: Could not open file: {}", file_name);
                (Document::default(), vec![Version::default()])
            }
        } else {
            (Document::default(), vec![Version::default()])
        };

        Self {
            should_quit: false,
            terminal: Terminal::default().expect("Failed to initialize terminal"),
            cursor_position: CursorPosition::default(),
            selection_start: Position::default(),
            offset: Position::default(),
            document,
            status_message: StatusMessage::from(initial_status),
            highlighted_word: None,
            clipboard: None,
            mode: Mode::Normal,
            versions,
            version_index: 0,
            has_saved: false,
            query: None,
        }
    }

    fn open_document(file_name: &str) -> Option<(Document, Vec<Version>)> {
        let doc = Document::open(&file_name);
        if let Ok(doc) = doc {
            let version = Version {
                document: doc.clone(),
                position: CursorPosition::default(),
                timestamp: chrono::offset::Local::now(),
            };
            Some((doc, vec![version]))
        } else {
            None
        }
    }

    fn refresh_screen(&mut self) -> Result<()> {
        self.terminal.update_size()?;
        Terminal::cursor_hide();
        Terminal::cursor_position(&Position::default(), false);
        if self.should_quit {
            Terminal::clear_screen();
        } else {
            self.document.highlight(
                &self.highlighted_word,
                Some(
                    self.offset
                        .y
                        .saturating_add(self.terminal.size().height as usize),
                ),
            );
            self.draw_rows();
            self.draw_status_bar();
            self.draw_message_bar();
            Terminal::cursor_position(
                &Position {
                    x: self.cursor_position.x.saturating_sub(self.offset.x),
                    y: self.cursor_position.y.saturating_sub(self.offset.y),
                },
                !self.document.row(self.cursor_position.y).is_none(),
            );
        }
        Terminal::cursor_show();
        self.terminal.flush()
    }

    fn version_status_message(
        &self,
        old_len: usize,
        old_changes: usize,
        index: usize,
        timestamp: &DateTime<Local>,
        redo: bool,
    ) -> String {
        let current_time = chrono::offset::Local::now();
        let diff = current_time - timestamp;
        let time = if diff.num_seconds() < 60 {
            let duration = if diff.num_seconds() == 1 {
                "second"
            } else {
                "seconds"
            };
            format!("{} {} ago", diff.num_seconds(), duration)
        } else if diff.num_minutes() < 10 {
            let duration = if diff.num_minutes() == 1 {
                "minute"
            } else {
                "minutes"
            };
            format!("{} {} ago", diff.num_minutes(), duration)
        } else {
            timestamp.format("%H:%M:%S").to_string()
        };
        let lines_added = self.document.len() as i64 - old_len as i64;
        let add_type = if lines_added.abs() == 1 {
            "line"
        } else {
            "lines"
        };
        let change_msg = if lines_added == -1 {
            format!("{} {} less", lines_added.abs(), add_type)
        } else if lines_added != 0 {
            let magnitude = if lines_added > 0 || redo {
                "more"
            } else {
                "fewer"
            };
            format!("{} {} {}", lines_added.abs(), magnitude, add_type)
        } else {
            let lines_changed = (self.document.lines_changed() as i64 - old_changes as i64).abs();
            let change_type = if lines_changed == 1 {
                "change"
            } else {
                "changes"
            };
            format!("{} {}", lines_changed.abs(), change_type)
        };
        format!("{}; before #{}  {}", change_msg, index, time)
    }

    fn undo(&mut self) -> Result<()> {
        if self.version_index == 0 {
            self.status_message = StatusMessage::from("Already at oldest change".to_string());
            return Ok(());
        }
        let prev_doc = &self.versions[self.version_index].document;
        let prev_len = prev_doc.len();
        let prev_changes = prev_doc.lines_changed();
        self.version_index = self.version_index.saturating_sub(1);
        let version = &self.versions[self.version_index];
        self.document = version.document.clone();
        self.cursor_position = version.position;
        let msg = self.version_status_message(
            prev_len,
            prev_changes,
            self.version_index.saturating_add(1),
            &version.timestamp,
            false,
        );
        self.status_message = StatusMessage::from(msg);
        self.readjust_cursor();
        self.refresh_screen()?;
        Ok(())
    }

    fn redo(&mut self) -> Result<()> {
        if self.version_index == self.versions.len() - 1 {
            self.status_message = StatusMessage::from("Already at newest change".to_string());
            return Ok(());
        }
        let Version {
            document: prev_doc,
            position,
            timestamp,
        } = &self.versions[self.version_index];
        let prev_len = prev_doc.len();
        let prev_changes = prev_doc.lines_changed();
        self.version_index = self.version_index.saturating_add(1);
        let version = &self.versions[self.version_index];
        self.document = version.document.clone();
        self.cursor_position = *position;
        let msg = self.version_status_message(
            prev_len,
            prev_changes,
            self.version_index,
            &timestamp,
            true,
        );
        self.status_message = StatusMessage::from(msg);
        self.readjust_cursor();
        self.refresh_screen()?;
        Ok(())
    }

    fn add_version(&mut self) {
        if self.version_index != self.versions.len() - 1 {
            self.versions.drain((self.version_index + 1)..);
        }
        let Version {
            document: _,
            position,
            timestamp,
        } = self.versions[self.version_index];
        let version = Version {
            document: self.document.clone(),
            position,
            timestamp,
        };
        self.document = self.document.clone();
        self.versions.push(version);
        self.version_index = self.versions.len() - 1;
        self.has_saved = false;
    }

    fn contains_changes(&self) -> bool {
        self.versions.len() != 1 && self.version_index != 0 && !self.has_saved
    }

    fn save_prev_cursor_position(&mut self) {
        let prev_version = &mut self.versions[self.version_index];
        prev_version.position = self.cursor_position;
        prev_version.timestamp = chrono::offset::Local::now();
    }

    fn save(&mut self) {
        if !self.contains_changes() {
            self.status_message = StatusMessage::from("No changes to write.".to_string());
            return;
        }

        if self.document.file_name.is_none() {
            let new_name = self.prompt("Save as: ", |_, _, _| {}).unwrap_or(None);
            if new_name.is_none() {
                self.status_message = StatusMessage::from("Save aborted.".to_string());
                return;
            }
            self.document.file_name = new_name;
        }

        let save_result = self.document.save();
        if let Ok(bytes_written) = save_result {
            self.status_message = StatusMessage::from(format!(
                "File saved successfully: {} bytes written.",
                bytes_written
            ));
            self.has_saved = true;
        } else {
            self.status_message = StatusMessage::from("Error writing file!".to_string());
        }
    }

    fn move_to_search_term(&mut self, direction: SearchDirection) {
        if let Some(query) = &self.query.clone() {
            let mut found = false;
            if direction == SearchDirection::Forward {
                self.move_cursor(Key::Right);
            }
            if let Some(position) =
                self.document
                    .find(query, &self.cursor_position.into(), direction)
            {
                self.cursor_position = position.into();
                self.scroll();
                found = true;
            }
            if !found && direction == SearchDirection::Forward {
                self.move_cursor(Key::Left);
            }
        }
    }

    fn search(&mut self) {
        let old_position = self.cursor_position;
        let mut direction = SearchDirection::Forward;
        self.query = self
            .prompt(
                "Search (ESC to cancel, Arrows to navigate): ",
                |editor, key, query| {
                    let mut moved = false;
                    match key {
                        Key::Right | Key::Down => {
                            direction = SearchDirection::Forward;
                            editor.move_cursor(Key::Right);
                            moved = true;
                        }
                        Key::Left | Key::Up => direction = SearchDirection::Backward,
                        _ => direction = SearchDirection::Forward,
                    }
                    if let Some(position) =
                        editor
                            .document
                            .find(query, &editor.cursor_position.into(), direction)
                    {
                        editor.cursor_position = position.into();
                        editor.scroll();
                    } else if moved {
                        editor.move_cursor(Key::Left);
                    }
                    editor.highlighted_word = Some(query.to_string());
                },
            )
            .unwrap_or(None);
        if self.query.is_none() {
            self.cursor_position = old_position;
            self.scroll();
        }
        self.highlighted_word = None;
    }

    fn show_cwd(&mut self) {
        if let Ok(cwd) = env::current_dir() {
            self.status_message = StatusMessage::from(format!("CWD is {}", cwd.display()))
        } else {
            self.status_message = StatusMessage::from(format!("ERR: CWD does not exist"))
        }
    }

    fn term_read_key_blocking(&mut self) -> Result<Key> {
        loop {
            self.refresh_screen()?;
            match self.terminal.read_event() {
                Some(event) => match event? {
                    KeyEvent(key) => return Ok(key),
                    _ => (),
                },
                None => (),
            }
            thread::sleep(time::Duration::from_millis(REFRESH_INTERVAL));
        }
    }

    fn term_read_event(&mut self) -> Option<Result<()>> {
        match self.terminal.read_event() {
            Some(event) => match event {
                Ok(event) => match event {
                    KeyEvent(key) => Some(self.process_keypress(key)),
                    Mouse(me) => Some(self.process_mouse_event(me)),
                    _ => None,
                },
                Err(error) => Some(Err(error)),
            },
            None => None,
        }
    }

    fn doc_edit<C>(&mut self, mut callback: C)
    where
        C: FnMut(&mut Self),
    {
        if self.mode != Mode::Insert {
            self.save_prev_cursor_position();
        }
        callback(self);
        if self.mode != Mode::Insert {
            self.add_version();
        }
    }

    fn doc_insert(&mut self, c: char) {
        self.doc_edit(|editor| {
            editor.document.insert(&editor.cursor_position.into(), c);
        })
    }

    fn doc_insert_line(&mut self, line: &str) {
        self.doc_edit(|editor| {
            editor.document.insert_line(editor.cursor_position.y, line);
        })
    }

    fn doc_paste_clipboard(&mut self) {
        if let Some(content) = &self.clipboard.clone() {
            self.doc_edit(|editor| {
                editor
                    .document
                    .insert_line(editor.cursor_position.y, &content);
            });
        }
    }

    fn doc_delete(&mut self) -> usize {
        let mut deleted = 0;
        self.doc_edit(|editor| {
            deleted = editor.document.delete(&editor.cursor_position.into());
        });
        deleted
    }

    fn doc_delete_line(&mut self) {
        self.doc_edit(|editor| {
            editor.document.delete_line(editor.cursor_position.y);
        });
    }

    fn doc_replace(&mut self, c: char) {
        self.doc_edit(|editor| {
            editor.document.replace(&editor.cursor_position.into(), c);
        });
    }

    // TODO: Improve command parsing
    // Split inputs by whitespace first and process entire words as commands
    // if no match and only one word then split commands by characters
    fn execute_command(&mut self) -> Result<()> {
        let input = self
            .prompt(":", |_, _, _| {})
            .unwrap_or(None)
            .unwrap_or("".to_string());
        let mut commands = input.chars().peekable();
        let first_char = *commands.peek().unwrap_or(&' ');
        if first_char == '+' || first_char == '-' {
            if let Ok(line_num) = input.parse::<i64>() {
                let mut y = self.cursor_position.y as i64 + line_num;
                if y < 0 {
                    y = 0;
                }
                self.cursor_position = CursorPosition {
                    x: 0,
                    y: y as usize,
                    max_x: 0,
                };
                self.readjust_cursor();
            } else {
                self.status_message = StatusMessage::from(format!("Invalid offset: {}", input));
            }
        } else if let Ok(line_num) = input.parse::<usize>() {
            self.cursor_position = CursorPosition {
                x: 0,
                y: line_num.saturating_sub(1),
                max_x: 0,
            };
            self.readjust_cursor();
        } else {
            while let Some(c) = commands.next() {
                match c {
                    'w' => self.save(),
                    'p' => self.show_cwd(),
                    'c' => {
                        if let Some(path) = input.split_whitespace().collect::<Vec<&str>>().get(1) {
                            let dir = Path::new(path);
                            if let Ok(_) = env::set_current_dir(dir) {
                                self.show_cwd();
                            } else {
                                self.status_message = StatusMessage::from(format!(
                                    "ERR: Path does not exist: {}",
                                    path
                                ));
                            };
                            return Ok(());
                        } else {
                            self.status_message =
                                StatusMessage::from(format!("ERR: No path entered"));
                        }
                    }
                    'q' | 'e' | 'u' => {
                        let force = input.contains("!");
                        let dirty = self.contains_changes();
                        if dirty && !force {
                            self.status_message = StatusMessage::from(
                                "WARNING! File has unsaved changes: add ! to override.".to_string(),
                            );
                            return Ok(());
                        } else {
                            match c {
                                'q' => self.should_quit = true,
                                'e' => {
                                    if let Some(path) =
                                        input.split_whitespace().collect::<Vec<&str>>().get(1)
                                    {
                                        if let Some((doc, versions)) = Self::open_document(&path) {
                                            self.document = doc;
                                            self.versions = versions;
                                            self.version_index = 0;
                                            self.cursor_position = CursorPosition::default();
                                            self.readjust_cursor();
                                        } else {
                                            self.status_message = StatusMessage::from(format!(
                                                "ERR: Could not open file: {}",
                                                path
                                            ));
                                        };
                                        return Ok(());
                                    } else {
                                        self.status_message =
                                            StatusMessage::from(format!("ERR: No path entered"));
                                    }
                                }
                                'u' => self.undo()?,
                                _ => (),
                            }
                        }
                    }
                    '!' => (),
                    _ => {
                        self.status_message =
                            StatusMessage::from(format!("Command not found: {}", c));
                    }
                }
            }
        }
        Ok(())
    }

    fn repeat_keypress(&mut self, n: u32) -> Result<()> {
        if n == 0 {
            return Ok(());
        }
        let mut number_message = n.to_string();
        self.status_message = StatusMessage::from(number_message.clone());
        self.refresh_screen()?;
        while let Key::Char(c) = self.term_read_key_blocking()? {
            if c.is_numeric() {
                number_message.push(c);
                self.status_message = StatusMessage::from(number_message.clone());
                self.refresh_screen()?;
            } else if c.is_alphabetic() {
                if c == 'r' || c == 's' {
                    break;
                }
                if let Ok(repeats) = number_message.parse() {
                    for _ in 0..repeats {
                        self.normal_mode(c)?;
                    }
                }
                break;
            } else {
                break;
            }
        }
        self.status_message = StatusMessage::from(String::new());
        self.refresh_screen()?;
        Ok(())
    }

    fn normal_mode(&mut self, c: char) -> Result<()> {
        match c {
            c if c.is_numeric() => {
                if let Some(n) = c.to_digit(10) {
                    self.repeat_keypress(n)?
                }
            }
            'h' => self.move_cursor(Key::Left),
            'j' => self.move_cursor(Key::Down),
            'k' => self.move_cursor(Key::Up),
            'l' => self.move_cursor(Key::Right),
            'g' | 'G' => {
                self.cursor_position.y = if c == 'g' { 0 } else { self.document.len() };
                self.cursor_position.x = 0;
                self.cursor_position.max_x = 0;
            }
            'a' | 'A' | 'i' | 'I' => {
                self.switch_mode(Mode::Insert);
                if c == 'a' {
                    self.move_cursor(Key::Right);
                } else if c == 'A' {
                    self.move_cursor(Key::End);
                } else if c == 'I' {
                    self.move_cursor(Key::Home);
                }
            }
            'o' | 'O' => {
                self.switch_mode(Mode::Insert);
                if c == 'o' {
                    self.move_cursor(Key::Down);
                }
                self.doc_insert_line("");
                if c == 'O' {
                    self.move_cursor(Key::Down);
                }
            }
            'r' => {
                let key = self.term_read_key_blocking()?;
                match key {
                    Key::Char(key) => {
                        self.doc_replace(key);
                    }
                    _ => return Ok(()),
                }
            }
            'x' | 's' => {
                if let Some(row) = self.document.row(self.cursor_position.y) {
                    let row_len = row.len();
                    if row_len > 0 {
                        self.doc_delete();
                    }
                    if c == 'x' && self.cursor_position.x == row_len.saturating_sub(1) {
                        self.move_cursor(Key::Left);
                    } else if c == 's' {
                        self.switch_mode(Mode::Insert);
                    }
                }
            }
            'd' | 'y' => {
                if let Some(row) = self.document.row(self.cursor_position.y) {
                    self.clipboard = Some(row.contents().trim().to_string());
                    if c == 'd' {
                        self.doc_delete_line();
                        self.move_cursor(Key::Home);
                    }
                }
            }
            'p' => {
                if self.clipboard.is_some() {
                    self.doc_paste_clipboard();
                    self.move_cursor(Key::Down);
                }
            }
            'R' => self.switch_mode(Mode::Replace),
            'v' => self.switch_mode(Mode::Visual),
            '/' => self.search(),
            ':' => self.execute_command()?,
            'u' => self.undo()?,
            'n' => self.move_to_search_term(SearchDirection::Forward),
            'N' => self.move_to_search_term(SearchDirection::Backward),
            _ => (),
        }
        if self.mode != Mode::Insert && self.document.is_dirty() {
            self.add_version();
        }
        Ok(())
    }

    fn insert_mode(&mut self, c: char) {
        match c {
            '\t' => {
                let spaces = 4 - self.cursor_position.x % 4;
                for _ in 0..spaces {
                    self.doc_insert(' ');
                    self.move_cursor(Key::Right);
                }
            }
            '\n' => {
                self.doc_insert(c);
                self.move_cursor(Key::Right);
                let mut spaces = 0;
                if let Some(row) = self.document.row(self.cursor_position.y) {
                    spaces = row.indentation();
                }
                for _ in 0..spaces.saturating_sub(self.cursor_position.x) {
                    self.move_cursor(Key::Right);
                }
            }
            '(' | '[' | '{' | '\'' | '"' => {
                self.doc_insert(c);
                self.move_cursor(Key::Right);
                let closing = if c == '(' {
                    ')'
                } else if c == '[' {
                    ']'
                } else if c == '{' {
                    '}'
                } else if c == '"' || self.document.file_type() != "Rust" {
                    c
                } else {
                    return;
                };
                self.doc_insert(closing);
            }
            _ => {
                self.doc_insert(c);
                self.move_cursor(Key::Right);
            }
        }
    }

    fn replace_mode(&mut self, c: char) {
        self.doc_replace(c);
        self.move_cursor(Key::Right);
    }

    // TODO: arrow keys select and highlight portions of document
    // from starting position, enter sets clipboard content to
    // selection and returns to normal mode
    // Store start and end selection cursor position.
    // If row.y == start.y, highlight all characters >= start.x
    // else if row.y == end.y, highlight all characters <= end.x
    // else highlight entire row
    fn visual_mode(&mut self, c: char) -> Result<()> {
        self.selection_start = self.cursor_position.into();
        self.switch_mode(Mode::Normal);
        match c {
            'h' | 'j' | 'k' | 'l' | 'c' | 'd' | 'y' => self.normal_mode(c)?,
            _ => (),
        }
        self.switch_mode(Mode::Visual);
        Ok(())
    }

    fn switch_mode(&mut self, mode: Mode) {
        use Mode::*;
        match mode {
            Normal => {
                let prev_mode = self.mode;
                if prev_mode != Mode::Normal {
                    self.mode = mode;
                    self.move_cursor(Key::Left);
                }
                if prev_mode == Mode::Insert && self.document.is_dirty() {
                    self.add_version();
                }
            }
            Insert => {
                let prev_version = &mut self.versions[self.version_index];
                prev_version.position = self.cursor_position;
                prev_version.timestamp = chrono::offset::Local::now();
            }
            _ => (),
        }
        self.mode = mode;
    }

    fn process_keypress(&mut self, pressed_key: Key) -> Result<()> {
        match pressed_key {
            Key::Char(c) => match self.mode {
                Mode::Insert => self.insert_mode(c),
                Mode::Normal => self.normal_mode(c)?,
                Mode::Replace => self.replace_mode(c),
                Mode::Visual => self.visual_mode(c)?,
            },
            Key::Esc => self.switch_mode(Mode::Normal),
            Key::Delete => {
                self.doc_delete();
            }
            Key::Backspace => {
                if self.cursor_position.x > 0 || self.cursor_position.y > 0 {
                    self.move_cursor(Key::Left);
                    if self.mode == Mode::Insert {
                        let deleted = self.doc_delete();
                        self.cursor_position.x = self.cursor_position.x.saturating_sub(deleted);
                        self.cursor_position.max_x = self.cursor_position.x;
                    }
                }
            }
            Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::PageUp
            | Key::PageDown
            | Key::End
            | Key::Home => self.move_cursor(pressed_key),
            Key::Ctrl('r') => {
                if self.mode == Mode::Normal {
                    self.redo()?
                }
            }
            _ => (),
        }
        self.scroll();
        Ok(())
    }

    fn process_mouse_event(&mut self, me: MouseEvent) -> Result<()> {
        use MouseButton::*;
        use MouseEvent::*;
        match me {
            Press(Left, x, y) => {
                let (mut x, mut y): (usize, usize) = (x.into(), y.into());
                if self.document.row(y).is_some() {
                    x = x.saturating_sub(5);
                }
                x = x.saturating_sub(1) + self.offset.x;
                y = y.saturating_sub(1) + self.offset.y;
                self.cursor_position = CursorPosition {
                    x,
                    y,
                    max_x: self.cursor_position.max_x,
                };
                self.readjust_cursor();
            }
            Press(Right, _, _) | Press(Middle, _, _) => {
                self.switch_mode(Mode::Insert);
                let content = Clipboard::new()?.get_text()?;
                for c in content.chars() {
                    self.doc_insert(c);
                }
            }
            // TODO: Scroll view without having to move cursor
            // Everything is already offset based so start there
            // Need to change how scrolling works with offsets
            // Need to reset offset when inserting or moving cursor with keyboard
            // Need to move cursor if left clicking
            Press(WheelDown, _, _) => {
                self.move_cursor(Key::Down);
                self.scroll();
            }
            Press(WheelUp, _, _) => {
                self.move_cursor(Key::Up);
                self.scroll();
            }
            Release(_x, _y) => (),
            Hold(_x, _y) => (),
        }
        Ok(())
    }

    fn scroll(&mut self) {
        let CursorPosition { x, y, max_x: _ } = self.cursor_position;
        let width = self.terminal.size().width as usize;
        let height = self.terminal.size().height as usize;
        let offset = &mut self.offset;
        let screen_x = x.saturating_sub(offset.x);
        let screen_y = y.saturating_sub(offset.y);
        let width_edge = width / 8;
        let height_edge = height / 5;
        if y < offset.y {
            offset.y = y.saturating_sub(height_edge);
        } else if screen_y < height_edge {
            offset.y = offset.y.saturating_sub(height_edge - screen_y);
        } else if y >= offset.y.saturating_add(height) {
            offset.y = y.saturating_sub(height).saturating_add(height_edge + 1);
        } else if screen_y >= height - height_edge {
            offset.y = offset.y.saturating_add(screen_y - (height - height_edge));
        }
        if x < offset.x {
            offset.x = x.saturating_sub(width_edge);
        } else if screen_x < width_edge {
            offset.x = offset.x.saturating_sub(width_edge - screen_x);
        } else if x >= offset.x.saturating_add(width) {
            offset.x = x.saturating_sub(width).saturating_add(width_edge);
        } else if screen_x >= width - width_edge {
            offset.x = offset.x.saturating_add(screen_x - (width - width_edge));
        }
    }

    fn readjust_cursor(&mut self) {
        let CursorPosition {
            mut x,
            mut y,
            max_x: _,
        } = self.cursor_position;
        let doc_len = self.document.len();
        if y > doc_len {
            y = doc_len;
        }
        x = if let Some(row) = self.document.row(y) {
            let row_len = row.len().saturating_sub(1);
            if x > row_len {
                row_len
            } else {
                x
            }
        } else {
            0
        };
        self.cursor_position = CursorPosition { x, y, max_x: x };
    }

    fn move_cursor(&mut self, key: Key) {
        let terminal_height = self.terminal.size().height as usize;
        let CursorPosition {
            mut x,
            mut y,
            mut max_x,
        } = self.cursor_position;
        let height = self.document.len();
        let mut width = if let Some(row) = self.document.row(y) {
            row.len()
        } else {
            0
        };
        if self.mode == Mode::Normal {
            width = width.saturating_sub(1);
        }
        match key {
            Key::Up => y = y.saturating_sub(1),
            Key::Down => {
                if y < height {
                    y = y.saturating_add(1);
                }
            }
            Key::Left => {
                if x > 0 {
                    x -= 1;
                } else if self.mode != Mode::Normal && y > 0 {
                    y -= 1;
                    if let Some(row) = self.document.row(y) {
                        x = row.len();
                    } else {
                        x = 0;
                    }
                }
                max_x = x;
            }
            Key::Right => {
                if x < width {
                    x += 1;
                } else if self.mode != Mode::Normal && y < height {
                    y += 1;
                    x = 0;
                }
                max_x = x;
            }
            Key::PageUp => {
                y = if y > terminal_height {
                    y.saturating_sub(terminal_height)
                } else {
                    0
                }
            }
            Key::PageDown => {
                y = if y.saturating_add(terminal_height) < height {
                    y.saturating_add(terminal_height)
                } else {
                    height
                }
            }
            Key::Home => {
                x = 0;
                max_x = x;
            }
            Key::End => {
                x = width;
                max_x = x;
            }
            _ => (),
        }
        width = if let Some(row) = self.document.row(y) {
            row.len()
        } else {
            0
        };
        if self.mode == Mode::Normal {
            width = width.saturating_sub(1);
        }
        x = max_x;
        if x > width {
            x = width;
        }

        self.cursor_position = CursorPosition { x, y, max_x }
    }

    fn draw_welcome_message(&self) {
        let mut welcome_message = format!("Hecto editor -- version {}\r", VERSION);
        let width = self.terminal.size().width as usize;
        let len = welcome_message.len();
        #[allow(clippy::integer_arithmetic, clippy::integer_division)]
        let padding = width.saturating_sub(len) / 2;
        let spaces = " ".repeat(padding.saturating_sub(1));
        welcome_message = format!("~{}{}", spaces, welcome_message);
        welcome_message.truncate(width);
        println!("{}\r", &welcome_message);
    }

    pub fn draw_row(&self, row: &Row, num: usize) {
        let width = self.terminal.size().width as usize;
        let start = self.offset.x;
        let end = self.offset.x.saturating_add(width);
        let render = row.render(start, end);
        let cursor_on_row = self.cursor_position.y == num.saturating_sub(1);
        if row.is_dirty() && !cursor_on_row {
            Terminal::set_fg_color(color::Rgb(128, 0, 0));
        } else if row.is_dirty() && cursor_on_row {
            Terminal::set_fg_color(color::Rgb(196, 72, 72));
        } else if !cursor_on_row {
            Terminal::set_fg_color(color::Rgb(85, 85, 85));
        }
        print!("{:>4} ", num);
        Terminal::reset_fg_color();
        println!("{}\r", render);
    }

    #[allow(clippy::integer_arithmetic, clippy::integer_division)]
    fn draw_rows(&self) {
        let height = self.terminal.size().height;
        for terminal_row in 0..height {
            Terminal::clear_current_line();
            let index = self.offset.y.saturating_add(terminal_row as usize);
            if let Some(row) = self.document.row(index) {
                let num = index.saturating_add(1);
                self.draw_row(row, num);
            } else if self.document.is_empty() && terminal_row == height / 3 {
                self.draw_welcome_message();
            } else {
                println!("~\r");
            }
        }
    }

    fn draw_status_bar(&self) {
        let mut status;
        let width = self.terminal.size().width.saturating_add(5) as usize;
        let modified_indicator = if self.contains_changes() {
            " (modified)"
        } else {
            ""
        };
        let mut file_name = "[No Name]".to_string();
        if let Some(name) = &self.document.file_name {
            file_name = name.clone();
            file_name.truncate(20);
        }
        let mode = match self.mode {
            Mode::Insert => "INSERT MODE",
            Mode::Normal => "NORMAL MODE",
            Mode::Replace => "REPLACE MODE",
            Mode::Visual => "VISUAL MODE",
        }
        .to_string();
        status = format!(
            "{} | {} - {} lines{}",
            mode,
            file_name,
            self.document.len(),
            modified_indicator
        );
        let progress = match self.offset.y {
            n if n == 0 => "Top".to_string(),
            n if n
                == self
                    .document
                    .len()
                    .saturating_sub(self.terminal.size().height.saturating_sub(1) as usize)
                && self.cursor_position.y >= n =>
            {
                "Bottom".to_string()
            }
            _ => format!(
                "%{}",
                ((self.cursor_position.y as f64 / self.document.len() as f64) * 100.0) as usize
            ),
        };
        let line_indicator = format!(
            "{} {} | {}:{} {}",
            chrono::offset::Local::now().format("%Y-%m-%d %H:%M:%S"),
            self.document.file_type(),
            self.cursor_position.y.saturating_add(1),
            self.cursor_position.x.saturating_add(1),
            progress
        );
        let len = status.len() + line_indicator.len();
        status.push_str(&" ".repeat(width.saturating_sub(len)));
        status = format!("{}{}", status, line_indicator);
        status.truncate(width);
        Terminal::set_bg_color(STATUS_BG_COLOR);
        Terminal::set_fg_color(STATUS_FG_COLOR);
        println!("{}\r", status);
        Terminal::reset_fg_color();
        Terminal::reset_bg_color();
    }

    fn draw_message_bar(&self) {
        Terminal::clear_current_line();
        let message = &self.status_message;
        if Instant::now() - message.time < Duration::new(5, 0) {
            let mut text = message.text.clone();
            text.truncate(self.terminal.size().width as usize);
            print!("{}", text);
        }
    }

    fn prompt<C>(&mut self, prompt: &str, mut callback: C) -> Result<Option<String>>
    where
        C: FnMut(&mut Self, Key, &String),
    {
        let mut result = String::new();
        loop {
            self.status_message = StatusMessage::from(format!("{}{}", prompt, result));
            self.refresh_screen()?;
            let key = self.term_read_key_blocking()?;
            match key {
                Key::Backspace => {
                    let graphemes_cnt = result.graphemes(true).count();
                    if graphemes_cnt == 0 {
                        break;
                    }
                    result = result
                        .graphemes(true)
                        .take(graphemes_cnt.saturating_sub(1))
                        .collect();
                }
                Key::Char('\n') => break,
                Key::Char(c) => {
                    if !c.is_control() {
                        result.push(c);
                    }
                }
                Key::Esc => {
                    result.truncate(0);
                    break;
                }
                _ => (),
            }
            callback(self, key, &result);
        }
        self.status_message = StatusMessage::from(String::new());
        if result.is_empty() {
            return Ok(None);
        }
        Ok(Some(result))
    }
}
