use idalib::{
	IDB,
	IDAError,
};
use thiserror::Error;

pub enum Error {
	IdaError( #[error] IDAError )
}

pub type Result<T> = std::result::Result<T, Error>;
