#[cfg(feature = "ida")]
use idalib::IDAError;

#[cfg(feature = "ida")]
#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error(transparent)]
	IdaError(#[from] IDAError),
}

#[cfg(not(feature = "ida"))]
#[derive(Debug)]
pub enum Error {}

pub type Result<T> = std::result::Result<T, Error>;
