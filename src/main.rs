use auto_re::{
	AutoRE,
	AutoREResult,
};

fn main() -> AutoREResult<()> {
	smol::block_on( async {
		AutoRE::new().run().await?;
		Ok(())
	})
}
