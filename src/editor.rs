use crate::Document;
use crate::Row;
use crate::Terminal;
use std::env;
use std::time::Duration;
use std::time::Instant;
use termion::color;
use termion::event::Key;
use unicode_segmentation::UnicodeSegmentation;

const STATUS_FG_COLOR: color::Rgb = color::Rgb(63, 63, 63);
const STATUS_BG_COLOR: color::Rgb = color::Rgb(239, 239, 239);
const VERSION: &str = env!("CARGO_PKG_VERSION");

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

#[derive(Default, Clone)]
pub struct Position {
    pub x: usize,
    pub y: usize,
    pub max_x: usize,
}

struct StatusMessage {
    time: Instant,
    text: String,
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
    cursor_position: Position,
    offset: Position,
    document: Document,
    status_message: StatusMessage,
    highlighted_word: Option<String>,
    clipboard: Option<String>,
    mode: Mode,
}

impl Editor {
    pub fn run(&mut self) {
        loop {
            if let Err(error) = self.refresh_screen() {
                die(&error);
            }
            if self.should_quit {
                break;
            }
            if let Err(error) = self.process_keypress() {
                die(&error);
            }
        }
    }

    pub fn default() -> Self {
        let args: Vec<String> = env::args().collect();
        let mut initial_status = String::from(": for commands");
        let document = if let Some(file_name) = args.get(1) {
            let doc = Document::open(&file_name);
            if let Ok(doc) = doc {
                doc
            } else {
                initial_status = format!("ERR: Could not open file: {}", file_name);
                Document::default()
            }
        } else {
            Document::default()
        };

        Self {
            should_quit: false,
            terminal: Terminal::default().expect("Failed to initialize terminal"),
            cursor_position: Position::default(),
            offset: Position::default(),
            document,
            status_message: StatusMessage::from(initial_status),
            highlighted_word: None,
            clipboard: None,
            mode: Mode::Normal,
        }
    }

    fn refresh_screen(&mut self) -> Result<(), std::io::Error> {
        Terminal::cursor_hide();
        Terminal::cursor_position(&Position::default(), false);
        if self.should_quit {
            Terminal::clear_screen();
            println!("Goodbye.\r");
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
                    max_x: self.cursor_position.max_x,
                },
                !self.document.row(self.cursor_position.y).is_none(),
            );
        }
        Terminal::cursor_show();
        Terminal::flush()
    }

    fn save(&mut self) {
        if !self.document.is_dirty() {
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
        } else {
            self.status_message = StatusMessage::from("Error writing file!".to_string());
        }
    }

    fn search(&mut self) {
        let old_position = self.cursor_position.clone();
        let mut direction = SearchDirection::Forward;
        let query = self
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
                            .find(query, &editor.cursor_position, direction)
                    {
                        editor.cursor_position = position;
                        editor.scroll();
                    } else if moved {
                        editor.move_cursor(Key::Left);
                    }
                    editor.highlighted_word = Some(query.to_string());
                },
            )
            .unwrap_or(None);
        if query.is_none() {
            self.cursor_position = old_position;
            self.scroll();
        }
        self.highlighted_word = None;
    }

    fn execute_command(&mut self) {
        let input = self
            .prompt(":", |_, _, _| {})
            .unwrap_or(None)
            .unwrap_or("".to_string());
        if let Ok(line_number) = input.parse::<usize>() {
            self.cursor_position.y = if line_number < self.document.len() {
                line_number.saturating_sub(1)
            } else {
                self.document.len()
            };
            return;
        }
        let mut commands = input.chars().peekable();
        while let Some(c) = commands.next() {
            match c {
                'w' => self.save(),
                'q' => {
                    if self.document.is_dirty() {
                        self.status_message =
                            StatusMessage::from(format!("WARNING! File has unsaved changes.",));
                        if let Some(next) = commands.peek() {
                            if *next == '!' {
                                commands.next();
                                self.should_quit = true;
                            }
                        }
                    } else {
                        self.should_quit = true;
                    }
                }
                _ => self.status_message = StatusMessage::from(format!("Command not found: {}", c)),
            }
        }
    }

    fn normal_mode(&mut self, c: char) {
        if let Some(n) = c.to_digit(10) {
            if let Ok(input) = self.prompt(&format!("{}", n)[..], |_, _, _| {}) {
                if let Some(mut input) = input {
                    input.insert_str(0, &n.to_string()[..]);
                    if let Some(command) = input.pop() {
                        if let Ok(repeats) = input.parse() {
                            for _ in 0..repeats {
                                self.normal_mode(command);
                            }
                        }
                    }
                }
            }
            return;
        }
        match c {
            'h' => self.move_cursor(Key::Left),
            'j' => self.move_cursor(Key::Down),
            'k' => self.move_cursor(Key::Up),
            'l' => self.move_cursor(Key::Right),
            'g' => {
                self.cursor_position.y = 0;
                self.cursor_position.x = 0;
            }
            'G' => {
                self.cursor_position.y = self.document.len();
                self.cursor_position.x = 0;
            }
            'a' => {
                self.move_cursor(Key::Right);
                self.mode = Mode::Insert;
            }
            'A' => {
                self.move_cursor(Key::End);
                self.mode = Mode::Insert;
            }
            'i' => self.mode = Mode::Insert,
            'I' => {
                self.move_cursor(Key::Home);
                self.mode = Mode::Insert;
            }
            'o' => {
                self.move_cursor(Key::End);
                self.mode = Mode::Insert;
                self.document.insert(&self.cursor_position, '\n');
                self.move_cursor(Key::Down);
            }
            'O' => {
                self.move_cursor(Key::Up);
                self.move_cursor(Key::End);
                self.mode = Mode::Insert;
                self.document.insert(&self.cursor_position, '\n');
                self.move_cursor(Key::Down);
            }
            's' => {
                self.mode = Mode::Insert;
                if let Err(error) = self.process_keypress() {
                    die(&error);
                }
                self.document.delete(&self.cursor_position);
                self.mode = Mode::Normal;
            }
            'x' => {
                self.document.delete(&self.cursor_position);
            }
            'y' => {
                if let Some(row) = self.document.row(self.cursor_position.y) {
                    self.clipboard = Some(row.contents().trim().to_string());
                }
            }
            'p' => {
                if self.clipboard.is_some() {
                    self.move_cursor(Key::End);
                    self.document.insert(&self.cursor_position, '\n');
                    self.move_cursor(Key::Down);
                    self.move_cursor(Key::End);
                    if let Some(contents) = &self.clipboard {
                        for c in contents.chars().rev() {
                            self.document.insert(&self.cursor_position, c);
                        }
                    }
                }
            }
            'd' => {
                self.move_cursor(Key::Home);
                if let Some(row) = self.document.row(self.cursor_position.y) {
                    self.clipboard = Some(row.contents().trim().to_string());
                    for _ in 0..=row.len() {
                        self.document.delete(&self.cursor_position);
                    }
                }
            }
            'r' => self.mode = Mode::Replace,
            'v' => self.mode = Mode::Visual,
            '/' => self.search(),
            ':' => self.execute_command(),
            _ => (),
        }
    }

    fn insert_mode(&mut self, c: char) {
        match c {
            '\t' => {
                let spaces = 4 - self.cursor_position.x % 4;
                for _ in 0..spaces {
                    self.document.insert(&self.cursor_position, ' ');
                }
                self.cursor_position.x = self.cursor_position.x.saturating_add(spaces);
            }
            '\n' => {
                self.document.insert(&self.cursor_position, c);
                self.move_cursor(Key::Right);
                let mut spaces = 0;
                if let Some(row) = self.document.row(self.cursor_position.y) {
                    spaces = row.indentation();
                }
                self.cursor_position.x = self.cursor_position.x.saturating_add(spaces);
            }
            '(' => {
                self.document.insert(&self.cursor_position, c);
                self.move_cursor(Key::Right);
                self.document.insert(&self.cursor_position, ')');
            }
            '[' => {
                self.document.insert(&self.cursor_position, c);
                self.move_cursor(Key::Right);
                self.document.insert(&self.cursor_position, ']');
            }
            '{' => {
                self.document.insert(&self.cursor_position, c);
                self.move_cursor(Key::Right);
                self.document.insert(&self.cursor_position, '}');
            }
            '\'' => {
                self.document.insert(&self.cursor_position, c);
                self.move_cursor(Key::Right);
                if self.document.file_type() != "Rust" {
                    self.document.insert(&self.cursor_position, c);
                }
            }
            '"' => {
                self.document.insert(&self.cursor_position, c);
                self.move_cursor(Key::Right);
                self.document.insert(&self.cursor_position, c);
            }
            _ => {
                self.document.insert(&self.cursor_position, c);
                self.move_cursor(Key::Right);
            }
        }
    }

    fn replace_mode(&mut self, c: char) {
        match c {
            '\t' | '\n' => self.insert_mode(c),
            _ => {
                self.document.delete(&self.cursor_position);
                self.document.insert(&self.cursor_position, c);
                self.move_cursor(Key::Right);
            }
        }
    }

    // TODO: arrow keys select and highlight portions of document
    // from starting position, enter sets clipboard content to
    // selection and returns to normal mode
    fn visual_mode(&mut self, c: char) {
        return;
        todo!();
    }

    fn process_keypress(&mut self) -> Result<(), std::io::Error> {
        let pressed_key = Terminal::read_key()?;
        match pressed_key {
            Key::Char(c) => match self.mode {
                Mode::Insert => self.insert_mode(c),
                Mode::Normal => self.normal_mode(c),
                Mode::Replace => self.replace_mode(c),
                Mode::Visual => self.visual_mode(c),
            },
            Key::Esc => self.mode = Mode::Normal,
            Key::Delete => {
                self.document.delete(&self.cursor_position);
            }
            Key::Backspace => {
                if self.cursor_position.x > 0 || self.cursor_position.y > 0 {
                    self.move_cursor(Key::Left);
                    if self.mode == Mode::Insert {
                        let deleted = self.document.delete(&self.cursor_position);
                        self.cursor_position.x = self.cursor_position.x.saturating_sub(deleted);
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
            _ => (),
        }
        self.scroll();
        Ok(())
    }

    fn scroll(&mut self) {
        let Position { x, y, max_x: _ } = self.cursor_position;
        let width = self.terminal.size().width as usize;
        let height = self.terminal.size().height as usize;
        let mut offset = &mut self.offset;
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

    fn move_cursor(&mut self, key: Key) {
        let terminal_height = self.terminal.size().height as usize;
        let Position {
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
                } else if y > 0 {
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
                } else if y < height {
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
        x = max_x;
        if x > width {
            x = width;
        }

        self.cursor_position = Position { x, y, max_x }
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
        let row = row.render(start, end);
        if self.cursor_position.y != num.saturating_sub(1) {
            Terminal::set_fg_color(color::Rgb(85, 87, 83));
        }
        print!("{:>4} ", num);
        Terminal::reset_fg_color();
        println!("{}\r", row);
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
        let modified_indicator = if self.document.is_dirty() {
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
            "{} | {}:{} {}",
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

    fn prompt<C>(&mut self, prompt: &str, mut callback: C) -> Result<Option<String>, std::io::Error>
    where
        C: FnMut(&mut Self, Key, &String),
    {
        let mut result = String::new();
        loop {
            self.status_message = StatusMessage::from(format!("{}{}", prompt, result));
            self.refresh_screen()?;
            let key = Terminal::read_key()?;
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

fn die(e: &std::io::Error) {
    Terminal::clear_screen();
    panic!("{}", e);
}
