use auto_re::Result;
use idalib::IDB;

fn main() -> Result<()> {
	let idb = IDB::open("/path/to/binary")?;
	Ok(())
}
