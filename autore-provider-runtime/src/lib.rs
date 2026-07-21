//! Provider runtime lifecycle management.
//!
//! This crate implements the coordinator-side provider bootstrap, authentication,
//! protocol negotiation, runtime management, and graceful shutdown.

pub mod bootstrap;
pub mod error;
pub mod listener;
pub mod package;
pub mod runtime;
pub mod shutdown;

pub use bootstrap::{BootstrapSecret, CoordinatorBootstrap};
pub use error::{NegotiateError, RuntimeError};
pub use listener::{BootstrapSocketAddr, BootstrapStream, bind_bootstrap_socket};
pub use package::{
    PackageInstallationIntent, PackageManifest, PackageValidationError, ProviderPackageDiscovery,
};
pub use runtime::{
    ProviderConfigBundle, ProviderInstanceHandle, ProviderManifest, ProviderRuntime,
};
pub use shutdown::GracefulShutdownSeq;
