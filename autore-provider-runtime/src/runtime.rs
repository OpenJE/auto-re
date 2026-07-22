//! Provider runtime: spawn, manage lifecycle, and enforce limits.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use autore_provider_protocol::v1::{
    NegotiateRequest, NegotiateResponse, provider_client::ProviderClient,
};
use autore_schema::ProviderInstanceId;

use crate::bootstrap::CoordinatorBootstrap;
use crate::error::{NegotiateError, RuntimeError};
use crate::listener::{BootstrapStream, bind_bootstrap_socket};
use crate::shutdown::GracefulShutdownSeq;

/// Describes a provider executable to spawn.
pub struct ProviderManifest {
    /// Path to the provider executable binary.
    pub executable_path: PathBuf,
    /// Package identifier (e.g., "fixture.echo").
    pub package_id: String,
    /// Package version string.
    pub package_version: String,
    /// Optional BLAKE3 content hash of the executable for integrity verification.
    pub content_hash: Option<Vec<u8>>,
}

/// Configuration passed to the provider via environment variables.
pub struct ProviderConfigBundle {
    /// Extra environment variables to pass to the provider (key → value).
    pub extra_env: HashMap<String, String>,
}

/// Handle to a running provider instance.
///
/// Provides access to the gRPC client, child process, and concurrency limits.
pub struct ProviderInstanceHandle {
    /// gRPC client for the provider.
    pub client: ProviderClient<tonic::transport::Channel>,
    /// Child process handle.
    pub child: Child,
    /// Cancellation token for coordinated shutdown.
    pub cancel: CancellationToken,
    /// Unique instance identifier.
    pub instance_id: ProviderInstanceId,
    /// Negotiated protocol version.
    pub negotiated_version: u32,
    /// Capabilities the provider exposes.
    pub capabilities: Vec<autore_provider_protocol::v1::CapabilityDescriptor>,
    /// Per-capability concurrency limits (semaphores).
    pub concurrency_limits: HashMap<String, Arc<Semaphore>>,
}

impl ProviderInstanceHandle {
    /// Initiates graceful shutdown of the provider.
    pub async fn shutdown(self) -> Result<(), RuntimeError> {
        GracefulShutdownSeq::execute(self).await
    }
}

/// Orchestrates provider spawning and bootstrap.
pub struct ProviderRuntime;

impl ProviderRuntime {
    /// Spawns a provider instance and completes the bootstrap handshake.
    ///
    /// The provider must:
    /// 1. Connect to the bootstrap socket (from env var).
    /// 2. Authenticate by echoing the secret.
    /// 3. Declare protocol version range (must include version 1).
    /// 4. Start a gRPC server and report its address.
    ///
    /// Returns a handle for interacting with the provider.
    pub async fn spawn(
        manifest: ProviderManifest,
        config: ProviderConfigBundle,
        deadline: Duration,
    ) -> Result<ProviderInstanceHandle, RuntimeError> {
        // 1. Create bootstrap credentials.
        let bootstrap = CoordinatorBootstrap::new()?;

        // 2. Bind bootstrap socket (UDS or TCP fallback).
        let (listener, socket_addr) = bind_bootstrap_socket().await?;

        // 3. Build child command (secrets in env, NOT args).
        let mut cmd = bootstrap.build_command(&manifest.executable_path, &socket_addr);
        for (k, v) in &config.extra_env {
            cmd.env(k, v);
        }

        // 4. Spawn child process.
        let mut child = cmd
            .spawn()
            .map_err(|e| RuntimeError::Spawn(e.to_string()))?;

        // 5. Accept bootstrap connection with deadline.
        let stream = match tokio::time::timeout(deadline, listener.accept()).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                // Timeout — kill child and return error.
                let _ = child.kill().await;
                return Err(RuntimeError::Timeout(
                    "provider did not connect within deadline".to_string(),
                ));
            }
        };

        // 6. Authenticate: provider sends 32-byte secret, we verify.
        let mut stream = stream;
        authenticate(&bootstrap.secret, &mut stream).await?;

        // 7. Negotiate: provider sends min/max version range, we check if 1 is in range.
        let (_provider_min, _provider_max) = negotiate_raw(&mut stream).await?;

        // 8. Provider sends gRPC server address.
        let grpc_addr = read_grpc_address(&mut stream).await?;

        // 9. Connect to provider's gRPC server.
        let channel = tonic::transport::Channel::from_shared(grpc_addr)
            .map_err(|e| RuntimeError::Spawn(e.to_string()))?
            .connect()
            .await?;
        let mut client = ProviderClient::new(channel);

        // 10. Call Negotiate RPC for full capability exchange.
        let negotiate_resp = negotiate_grpc(&mut client, &bootstrap.instance_id).await?;

        // 11. Verify package identity matches manifest.
        verify_package_identity(&negotiate_resp, &manifest)?;

        // 12. Build concurrency limits from response.
        let concurrency_limits = build_concurrency_limits(&negotiate_resp)?;

        // 13. Create cancellation token (propagated into streams on Execute calls).
        let cancel = CancellationToken::new();

        Ok(ProviderInstanceHandle {
            client,
            child,
            cancel,
            instance_id: bootstrap.instance_id,
            negotiated_version: negotiate_resp.accepted_version,
            capabilities: negotiate_resp.capabilities,
            concurrency_limits,
        })
    }
}

/// Authenticates the provider by verifying the echoed secret.
async fn authenticate(
    expected: &crate::bootstrap::BootstrapSecret,
    stream: &mut BootstrapStream,
) -> Result<(), RuntimeError> {
    // Read 32-byte secret from provider.
    let mut received = [0u8; 32];
    stream.read_exact(&mut received).await?;

    // Byte-equal compare.
    if received != *expected.as_bytes() {
        // Send failure byte.
        let _ = stream.write_all(&[0xFF]).await;
        return Err(RuntimeError::Authentication("secret mismatch".to_string()));
    }

    // Send success byte.
    stream.write_all(&[0x00]).await?;
    Ok(())
}

/// Reads provider's protocol version range and validates it includes version 1.
async fn negotiate_raw(stream: &mut BootstrapStream) -> Result<(u32, u32), RuntimeError> {
    // Read min_supported (u32 big-endian).
    let min = stream.read_u32().await?;
    // Read max_supported (u32 big-endian).
    let max = stream.read_u32().await?;

    // Check if version 1 is in range.
    if min > 1 || max < 1 {
        // Send failure byte.
        let _ = stream.write_all(&[0xFF]).await;
        return Err(RuntimeError::Negotiate(
            NegotiateError::UnsupportedVersion { min, max },
        ));
    }

    // Send success byte.
    stream.write_all(&[0x00]).await?;
    Ok((min, max))
}

/// Reads the gRPC server address from the provider.
async fn read_grpc_address(stream: &mut BootstrapStream) -> Result<String, RuntimeError> {
    // Read 2-byte length (u16 big-endian).
    let len = stream.read_u16().await? as usize;
    // Read UTF-8 address.
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    String::from_utf8(buf).map_err(|e| RuntimeError::Spawn(e.to_string()))
}

/// Calls the Negotiate RPC for full capability exchange.
async fn negotiate_grpc(
    client: &mut ProviderClient<tonic::transport::Channel>,
    instance_id: &ProviderInstanceId,
) -> Result<NegotiateResponse, RuntimeError> {
    let req = NegotiateRequest {
        min_supported: 1,
        max_supported: 1,
        coordinator_id: instance_id.to_string(),
    };
    let resp = client.negotiate(req).await?.into_inner();
    Ok(resp)
}

/// Verifies that the provider's declared package identity matches the manifest.
fn verify_package_identity(
    response: &NegotiateResponse,
    manifest: &ProviderManifest,
) -> Result<(), RuntimeError> {
    if response.package_id != manifest.package_id {
        return Err(RuntimeError::PackageIdentity(format!(
            "package_id mismatch: expected {}, got {}",
            manifest.package_id, response.package_id
        )));
    }
    if response.package_version != manifest.package_version {
        return Err(RuntimeError::PackageIdentity(format!(
            "package_version mismatch: expected {}, got {}",
            manifest.package_version, response.package_version
        )));
    }
    Ok(())
}

/// Builds per-capability concurrency semaphores from the negotiate response.
fn build_concurrency_limits(
    response: &NegotiateResponse,
) -> Result<HashMap<String, Arc<Semaphore>>, RuntimeError> {
    let mut limits = HashMap::new();

    // Parse max_concurrency JSON map. Integer values are treated as per-capability
    // concurrency limits; non-integer entries (e.g. backend metadata strings) are
    // ignored so providers can attach extension metadata without breaking the
    // runtime.
    if !response.max_concurrency.is_empty() {
        let map: HashMap<String, serde_json::Value> =
            serde_json::from_slice(&response.max_concurrency)
                .map_err(|e| RuntimeError::Spawn(format!("invalid max_concurrency JSON: {e}")))?;
        for (cap_id, max) in map {
            if let Some(max_u) = max.as_u64() {
                limits.insert(cap_id, Arc::new(Semaphore::new(max_u as usize)));
            }
        }
    }

    Ok(limits)
}
