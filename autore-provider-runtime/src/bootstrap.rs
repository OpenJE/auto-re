//! Coordinator bootstrap: generates instance ID and secrets for provider authentication.

use autore_schema::ProviderInstanceId;

use crate::error::RuntimeError;

/// A 32-byte cryptographic secret used for provider authentication during bootstrap.
#[derive(Clone)]
pub struct BootstrapSecret([u8; 32]);

impl BootstrapSecret {
    /// Generates a new random 32-byte secret using `getrandom`.
    pub fn generate() -> Result<Self, RuntimeError> {
        let mut secret = [0u8; 32];
        getrandom::getrandom(&mut secret).map_err(|e| RuntimeError::Spawn(e.to_string()))?;
        Ok(BootstrapSecret(secret))
    }

    /// Returns the secret bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the secret as a hex-encoded string for environment variable passing.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl std::fmt::Debug for BootstrapSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BootstrapSecret([REDACTED])")
    }
}

/// Coordinates the bootstrap of a provider instance.
///
/// Generates a unique instance ID (UUIDv7) and a cryptographic secret,
/// then provides a command builder that passes these via environment variables
/// (never via command-line arguments).
pub struct CoordinatorBootstrap {
    /// Unique instance identifier (UUIDv7).
    pub instance_id: ProviderInstanceId,

    /// Cryptographic secret for authentication.
    pub secret: BootstrapSecret,
}

impl CoordinatorBootstrap {
    /// Creates a new bootstrap with a fresh instance ID and secret.
    pub fn new() -> Result<Self, RuntimeError> {
        Ok(CoordinatorBootstrap {
            instance_id: ProviderInstanceId::new(),
            secret: BootstrapSecret::generate()?,
        })
    }

    /// Builds a `tokio::process::Command` for launching the provider executable.
    ///
    /// Secrets are passed ONLY via environment variables, never via command-line arguments.
    pub fn build_command(
        &self,
        executable_path: &std::path::Path,
        socket_addr: &crate::listener::BootstrapSocketAddr,
    ) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(executable_path);

        // Pass bootstrap information ONLY via environment variables.
        cmd.env("AUTORE_BOOTSTRAP_SOCKET", socket_addr.to_string());
        cmd.env("AUTORE_BOOTSTRAP_SECRET", self.secret.to_hex());
        cmd.env("AUTORE_BOOTSTRAP_INSTANCE_ID", self.instance_id.to_string());

        cmd
    }
}
