pub mod state;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::widgets::{Block, Paragraph};

use crate::tui::state::State;

/// TUI application state. Terminal is managed separately in `run_tui`
/// so the borrow checker stays happy during `ratatui::Terminal::draw`.
pub struct Tui {
	_state: State,
}

impl Tui {
	#[must_use]
	pub fn new() -> Self {
		Self {
			_state: State::Blank,
		}
	}

	fn handle_key_event(&mut self, key_event: KeyEvent) -> io::Result<bool> {
		match key_event.kind {
			KeyEventKind::Press => match key_event.code {
				KeyCode::Char('q') => Ok(true),
				_ => Ok(false),
			},
			_ => Ok(false),
		}
	}
}

/// Entry point for the TUI, called from main when the `tui` feature is enabled.
/// Renders a simple bordered paragraph and exits on `q`.
pub async fn run_tui() -> crate::Result<()> {
	let mut terminal = ratatui::init();
	let mut app = Tui::new();

	loop {
		terminal
			.draw(|frame| {
				frame.render_widget(
					Paragraph::new("auto-re TUI — press q to quit")
						.block(Block::bordered().title("auto-re")),
					frame.area(),
				);
			})
			.map_err(crate::Error::Io)?;

		// Poll with 100 ms timeout so tokio can run other tasks.
		if event::poll(Duration::from_millis(100)).map_err(crate::Error::Io)? {
			if let Event::Key(key_event) = event::read().map_err(crate::Error::Io)? {
				if app.handle_key_event(key_event)? {
					break;
				}
			}
		}
	}

	ratatui::restore();
	Ok(())
}
