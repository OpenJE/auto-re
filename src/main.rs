use auto_re::Result;
#[cfg(feature = "ida")]
use idalib::IDB;

fn main() -> Result<()> {
	#[cfg(feature = "ida")]
	{
		let _ = IDB::open("/path/to/binary")?;
	}
	Ok(())
}
