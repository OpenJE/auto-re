mod engine;
mod store;
mod tui;

use std::{
	io,
	result::Result,
};
use thiserror::Error;
use crossterm::{
	event::{
		self,
		Event,
		KeyEvent,
		KeyEventKind,
		KeyCode,
	},
};
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
use idalib::{
	IDB,
	IDAError,
};

use tui::{
	state::State as TuiState,
};

#[derive( Error, Debug )]
pub enum AutoREError {
	#[error( "IO error: {0}" )]
	StdIoError( #[from] io::Error ),
	#[error( "IDA error: {0}" )]
	IdaError( #[from] IDAError ),
}

pub type AutoREResult<T> = Result<T, AutoREError>;

pub struct AutoRE {
	exit: bool,
	tui_state: TuiState,
	idb: Option<IDB>,
}

impl AutoRE {
	pub fn new() -> AutoRE {
		AutoRE {
			exit: false,
			tui_state: TuiState::Blank,
			idb: None,
		}
	}

	pub fn run( &mut self, terminal: &mut DefaultTerminal ) -> AutoREResult<()> {
		self.idb = match IDB::open( "../openvb/F3.exe.i64" ) {
			Ok( idb ) => Some( idb ),
			Err( error ) => return Err( AutoREError::IdaError( error ) ),
		};

		while !self.exit {
			terminal.draw( |frame| self.render( frame ) )?;
			match event::read()? {
				Event::Key( key_event ) => self.handle_key_event( key_event )?,
				_ => {}
			}
		}

		Ok(())
	}

	fn render( &self, frame: &mut Frame ) {
		frame.render_widget(
			Paragraph::new(
				match &self.idb {
					Some( idb ) => match idb.function_at( 0x56b810 ) {
						Some( function ) => idb.decompile( &function )
							.unwrap()
							.pseudocode(),
						None => "No Pseudocode".into(),
					},
					None => "No Pseudocode".into(),
				}
			)
				.block( Block::bordered().title( "Pseudocode" ) ),
				//.style( Style::new().white().on_black() ),
				//.alignment( Alignment::Center )
				//.wrap( Wrap { trim: true } ),

			frame.area()
		);
	}

	fn handle_key_event( &mut self, key_event: KeyEvent ) -> io::Result<()> {
		match key_event.kind {
			KeyEventKind::Press => {
				match key_event.code {
					KeyCode::Char('q') => {
						self.exit = true;
					}
					_ => {}
				}
			}
			_ => {}
		}

		Ok(())
	}
}
