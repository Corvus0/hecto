use crate::Position;
use std::io::{self, stdout, Write};
use termion::event::Key;
use termion::input::{Keys, TermRead};
use termion::raw::{IntoRawMode, RawTerminal};
use termion::{async_stdin, color, AsyncReader};

pub struct Size {
    pub width: u16,
    pub height: u16,
}
pub struct Terminal {
    size: Size,
    _stdout: RawTerminal<std::io::Stdout>,
    _stdin: Keys<AsyncReader>,
}

impl Terminal {
    pub fn default() -> Result<Self, std::io::Error> {
        let size = termion::terminal_size()?;
        Ok(Self {
            size: Size {
                width: size.0.saturating_sub(5),
                height: size.1.saturating_sub(2),
            },
            _stdout: stdout().into_raw_mode()?,
            _stdin: async_stdin().keys(),
        })
    }

    pub fn size(&self) -> &Size {
        &self.size
    }

    pub fn update_size(&mut self) -> Result<(), std::io::Error> {
        let size = termion::terminal_size()?;
        self.size = Size {
            width: size.0.saturating_sub(5),
            height: size.1.saturating_sub(2),
        };
        Ok(())
    }

    pub fn clear_screen() {
        print!("{}", termion::clear::All);
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn cursor_position(position: &Position, is_row: bool) {
        let Position { mut x, mut y, .. } = position;
        if is_row {
            x = x.saturating_add(6);
        } else {
            x = x.saturating_add(1);
        }
        y = y.saturating_add(1);
        let x = x as u16;
        let y = y as u16;
        print!("{}", termion::cursor::Goto(x, y));
    }

    pub fn flush() -> Result<(), std::io::Error> {
        io::stdout().flush()
    }

    pub fn read_key(&mut self) -> Option<Result<Key, std::io::Error>> {
        self._stdin.next()
    }

    pub fn cursor_hide() {
        print!("{}", termion::cursor::Hide);
    }

    pub fn cursor_show() {
        print!("{}", termion::cursor::Show);
    }

    pub fn cursor_normal() {
        print! {"{}", termion::cursor::SteadyBlock}
    }

    pub fn cursor_edit() {
        print! {"{}", termion::cursor::SteadyUnderline}
    }

    pub fn clear_current_line() {
        print!("{}", termion::clear::CurrentLine);
    }

    pub fn set_bg_color(color: color::Rgb) {
        print!("{}", color::Bg(color));
    }

    pub fn reset_bg_color() {
        print!("{}", color::Bg(color::Reset));
    }

    pub fn set_fg_color(color: color::Rgb) {
        print!("{}", color::Fg(color));
    }

    pub fn reset_fg_color() {
        print!("{}", color::Fg(color::Reset));
    }
}
