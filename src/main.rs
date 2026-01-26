use auto_re::{
	AutoRE,
	AutoREResult,
};

fn main() -> AutoREResult<()> {
	let mut terminal = ratatui::init();
	let mut autore = AutoRE::new();
	autore.run( &mut terminal )?;
	ratatui::restore();
	Ok(())
}
