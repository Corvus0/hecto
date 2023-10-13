use crate::Position;
use anyhow::Result;
use std::io::{stdout, Stdout, Write};
use termion::cursor::{Goto, Hide, Show};
use termion::event::Event;
use termion::input::{Events, MouseTerminal, TermRead};
use termion::raw::{IntoRawMode, RawTerminal};
use termion::screen::{AlternateScreen, IntoAlternateScreen};
use termion::{async_stdin, color, AsyncReader};

pub struct Size {
    pub width: u16,
    pub height: u16,
}
pub struct Terminal {
    size: Size,
    stdin: Events<AsyncReader>,
    stdout: RawTerminal<AlternateScreen<MouseTerminal<Stdout>>>,
}

impl Terminal {
    pub fn default() -> Result<Self> {
        let size = termion::terminal_size()?;
        Ok(Self {
            size: Size {
                width: size.0.saturating_sub(5),
                height: size.1.saturating_sub(2),
            },
            stdin: async_stdin().events(),
            stdout: MouseTerminal::from(stdout())
                .into_alternate_screen()?
                .into_raw_mode()?,
        })
    }

    pub fn size(&self) -> &Size {
        &self.size
    }

    pub fn update_size(&mut self) -> Result<()> {
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
        print!("{}", Goto(x, y));
    }

    pub fn flush_static() -> Result<()> {
        stdout().flush().map_err(anyhow::Error::from)
    }

    pub fn flush(&mut self) -> Result<()> {
        self.stdout.flush().map_err(anyhow::Error::from)
    }

    pub fn read_event(&mut self) -> Option<Result<Event>> {
        self.stdin.next().map(|op| op.map_err(anyhow::Error::from))
    }

    pub fn cursor_hide() {
        print!("{Hide}");
    }

    pub fn cursor_show() {
        print!("{Show}");
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
