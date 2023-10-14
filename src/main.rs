#![warn(clippy::all, clippy::pedantic, clippy::restriction)]
#![allow(
    clippy::missing_docs_in_private_items,
    clippy::implicit_return,
    clippy::shadow_reuse,
    clippy::print_stdout,
    clippy::wildcard_enum_match_arm,
    clippy::else_if_without_else
)]
mod document;
mod editor;
mod filetype;
mod highlighting;
mod row;
mod terminal;
use anyhow::{Error, Result};
pub use document::Document;
use editor::Editor;
pub use editor::Position;
pub use editor::SearchDirection;
pub use filetype::FileType;
pub use filetype::HighlightingOptions;
pub use row::Row;
pub use terminal::Terminal;

fn main() {
    if let Err(error) = run() {
        die(&error);
    }
}

fn run() -> Result<()> {
    let mut editor = Editor::default();
    editor.run()?;
    Ok(())
}

fn die(error: &Error) -> ! {
    Terminal::clear_screen();
    let _ = Terminal::flush_static();
    panic!("{}", error);
}
