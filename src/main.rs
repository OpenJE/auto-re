use auto_re::Result;

#[cfg(feature = "ida")]
use idax::database;

fn main() -> Result<()> {
	#[cfg(feature = "ida")]
	{
		database::init()?;
		println!("IDA initialized via idax");
	}

	#[cfg(feature = "tui")]
	{
		smol::block_on(async {
			auto_re::tui::run_tui().await
		})?;
	}

	#[cfg(not(any(feature = "ida", feature = "tui")))]
	{
		println!("auto-re: no features enabled. Use --features ida,tui");
	}

	Ok(())
}
