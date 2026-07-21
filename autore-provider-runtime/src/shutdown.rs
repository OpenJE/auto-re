//! Graceful shutdown sequence for provider instances.

use std::time::Duration;

use autore_provider_protocol::v1::ShutdownRequest;

use crate::error::RuntimeError;
use crate::runtime::ProviderInstanceHandle;

/// Executes the graceful shutdown sequence for a provider instance.
///
/// Sequence:
/// 1. Send `GracefulShutdown` RPC with a 5-second timeout.
/// 2. Wait for child process to exit with a 10-second budget.
/// 3. If timeout, kill the child and reap.
pub struct GracefulShutdownSeq;

impl GracefulShutdownSeq {
    /// Executes the shutdown sequence on the given handle.
    pub async fn execute(handle: ProviderInstanceHandle) -> Result<(), RuntimeError> {
        let ProviderInstanceHandle {
            mut client,
            mut child,
            cancel,
            ..
        } = handle;

        // 1. Send GracefulShutdown RPC (5-second timeout).
        let shutdown_req = ShutdownRequest {
            reason: "coordinator-initiated shutdown".to_string(),
            grace_period_seconds: 10,
        };
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            client.graceful_shutdown(shutdown_req),
        )
        .await;

        // 2. Wait for child exit with 10-second budget.
        match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
            Ok(Ok(_status)) => {
                // Child exited normally.
            }
            Ok(Err(e)) => {
                // Wait failed, but child may have already exited.
                tracing::warn!("child wait error during shutdown: {e}");
            }
            Err(_) => {
                // Timeout — force kill.
                tracing::warn!("provider did not exit within 10s, killing");
                let _ = child.kill().await;
                let _ = child.wait().await; // Reap.
            }
        }

        // 3. Cancel the token to clean up any background tasks.
        cancel.cancel();

        Ok(())
    }
}
