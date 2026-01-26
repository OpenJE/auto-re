pub mod state;

use ratatui::{
	DefaultTerminal,
	Frame,
	widgets::{
		Block,
		Wrap,
		Paragraph,
	},
	style::{
		Style,
	},
	layout::{
		Alignment,
	},
};

use state::State;

pub struct Tui {
	current_state: State,
}

impl Tui {
	pub fn new() -> Self {
		Self {
			current_state: State::Blank,
		}
	}

	fn render( &self, frame: &mut Frame ) {

	}
}
