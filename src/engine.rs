mod graph;

use graph::REGraph;

#[derive( Error, Debug )]
pub enum EngineError {
	#[error( "IO error: {0}" )]
	StdIoError( #[from] io::Error ),
	#[error( "IDA error: {0}" )]
	IdaError( #[from] IDAError ),
}

pub type EngineResult<T> = Result<T, EngineError>;

pub struct Engine {
	idb: Option<IDB>,
	graph: REGraph,
}

impl Engine {
	pub fn new() -> Self {
		Self {
			idb: None,
			graph: REGraph
		}
	}

	pub fn open( &mut self, path: impl AsRef<Path> ) -> Result<Self, EngineError> {
		self.idb = match IDB::open( path ) {
			Ok( idb ) => Some( idb ),
			Err( error ) => return Err( EngineError::IdaError( error ) ),
		};
	}

	pub async fn
}
