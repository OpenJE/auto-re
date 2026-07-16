// Experimental TUI module — remote code, needs migration to spec-aligned architecture.
// This module is behind the `tui` feature and is not part of the M1 plan.

pub mod state;

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
	Frame,
	widgets::{Block, Paragraph},
};

use crate::event::Event as AppEvent;
use crate::tui::state::State;

#[derive(Debug, thiserror::Error)]
pub enum TuiError {
	#[error("io error: {0}")]
	StdIoError(#[from] io::Error),
}

pub type TuiResult<T> = std::result::Result<T, TuiError>;

pub struct Tui {
	terminal: ratatui::DefaultTerminal,
	_state: State,
}

impl Tui {
	pub fn new() -> Self {
		Self {
			terminal: ratatui::init(),
			_state: State::Blank,
		}
	}

	fn render(&self, frame: &mut Frame) {
		frame.render_widget(
			Paragraph::new("auto-re TUI (experimental)")
				.block(Block::bordered().title("auto-re")),
			frame.area(),
		);
	}

	fn handle_key_event(&mut self, key_event: KeyEvent) -> io::Result<bool> {
		match key_event.kind {
			KeyEventKind::Press => match key_event.code {
				KeyCode::Char('q') => Ok(true), // signal exit
				_ => Ok(false),
			},
			_ => Ok(false),
		}
	}
}

impl Drop for Tui {
	fn drop(&mut self) {
		ratatui::restore();
	}
}

/// Entry point for the TUI, called from main when the `tui` feature is enabled.
pub async fn run_tui() -> crate::Result<()> {
	let mut tui = Tui::new();

	loop {
		tui.terminal
			.draw(|frame| tui.render(frame))
			.map_err(|e| crate::Error::Io(e))?;

		match event::read().map_err(|e| crate::Error::Io(e))? {
			Event::Key(key_event) => {
				if tui
					.handle_key_event(key_event)
					.map_err(|e| crate::Error::Io(e))?
				{
					break;
				}
			}
			_ => {}
		}
	}

	Ok(())
}
