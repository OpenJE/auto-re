pub mod state;

use ratatui::{
	Terminal,
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

#[derive( Error, Debug )]
pub enum TuiError {
	#[error( "IO error: {0}" )]
	StdIoError( #[from] io::Error ),
}

pub type TuiResult<T> = Result<T, TuiError>;

pub struct Tui {
	terminal: Terminal<Backend<std::io::Stderr>>,
	event_rx: UnboundedReceiver<Event>,
	event_tx: UnboundedSender<Event>,
}

impl Tui {
	pub fn new(  ) -> Self {
		let ( event_tx, event_rx ) = mpsc::unbounded_channel();
		Self {
			terminal: ratatui::init(),
			event_tx,
			event_rx,
		}
	}

	async fn update( &self ) -> TuiResult<()> {

	}

	async fn render( &self ) {
		self.terminal.draw( |frame| self.render( frame ) )?;
	}
}

impl Drop for Tui {
	fn drop( &mut self ) {
		ratatui::restore();
	}
}
