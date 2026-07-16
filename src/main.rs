use auto_re::Result;

#[tokio::main]
async fn main() -> Result<()> {
	#[cfg(feature = "tui")]
	{
		auto_re::tui::run_tui().await?;
		return Ok(());
	}

	#[cfg(not(feature = "tui"))]
	{
		println!("auto-re CLI not yet implemented");
		Ok(())
	}
}
