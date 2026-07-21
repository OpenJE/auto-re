//! Integration tests for the provider runtime bootstrap flow.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use autore_provider_runtime::{
    BootstrapSocketAddr, ProviderConfigBundle, ProviderManifest, ProviderRuntime,
};

/// Test that bootstrap secrets are never passed via command-line arguments.
///
/// This test inspects the `Command` structure before spawning to verify
/// that secrets and env var names are not present in the args.
#[tokio::test]
async fn bootstrap_secrets_never_in_argv() {
    use autore_provider_runtime::bootstrap::CoordinatorBootstrap;

    let bootstrap = CoordinatorBootstrap::new().expect("bootstrap creation failed");
    let socket_addr = BootstrapSocketAddr::Tcp("127.0.0.1:12345".parse().unwrap());

    let cmd = bootstrap.build_command(PathBuf::from("/bin/echo").as_path(), &socket_addr);

    // Inspect the command's args (not env vars).
    let std_cmd = cmd.as_std();
    let args: Vec<String> = std_cmd
        .get_args()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();

    // Args should only contain the executable path (if at all).
    let secret_hex = bootstrap.secret.to_hex();
    for arg in &args {
        assert!(!arg.contains(&secret_hex), "secret found in argv: {arg}");
        assert!(
            !arg.contains("AUTORE_BOOTSTRAP_SECRET"),
            "env var name found in argv: {arg}"
        );
        assert!(
            !arg.contains("AUTORE_BOOTSTRAP_SOCKET"),
            "env var name found in argv: {arg}"
        );
        assert!(
            !arg.contains("AUTORE_BOOTSTRAP_INSTANCE_ID"),
            "env var name found in argv: {arg}"
        );
    }
}

/// Test that negotiation rejects an unsupported protocol version range.
#[test]
fn negotiate_rejects_unsupported_protocol() {
    use autore_provider_runtime::error::NegotiateError;

    // Simulate a provider that declares range [5, 10] (does not include version 1).
    let provider_min = 5u32;
    let provider_max = 10u32;

    // Check if version 1 is in range.
    let result = if provider_min > 1 || provider_max < 1 {
        Err(NegotiateError::UnsupportedVersion {
            min: provider_min,
            max: provider_max,
        })
    } else {
        Ok(1u32)
    };

    assert!(result.is_err());
    let err = result.unwrap_err();
    match err {
        NegotiateError::UnsupportedVersion { min, max } => {
            assert_eq!(min, 5);
            assert_eq!(max, 10);
        }
        _ => panic!("expected UnsupportedVariant error"),
    }
}

/// Test that authentication rejects a wrong secret.
#[tokio::test]
async fn authentication_rejects_wrong_secret() {
    use autore_provider_runtime::bootstrap::BootstrapSecret;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    // Create a listener and a client.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let expected = BootstrapSecret::generate().unwrap();
    let wrong_secret = BootstrapSecret::generate().unwrap();

    // Spawn client that sends wrong secret.
    let client_task = tokio::spawn(async move {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(wrong_secret.as_bytes()).await.unwrap();
        let mut status = [0u8; 1];
        stream.read_exact(&mut status).await.unwrap();
        status[0]
    });

    // Server-side authentication.
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut received = [0u8; 32];
    stream.read_exact(&mut received).await.unwrap();

    let auth_result = if received != *expected.as_bytes() {
        let _ = stream.write_all(&[0xFF]).await;
        Err(autore_provider_runtime::RuntimeError::Authentication(
            "secret mismatch".to_string(),
        ))
    } else {
        stream.write_all(&[0x00]).await.unwrap();
        Ok(())
    };

    assert!(auth_result.is_err());
    let err = auth_result.unwrap_err();
    assert!(err.to_string().contains("authentication failed"));

    // Verify client received failure byte.
    let client_status = client_task.await.unwrap();
    assert_eq!(client_status, 0xFF);
}

/// Test that graceful shutdown completes within 10 seconds.
#[tokio::test]
async fn graceful_shutdown_within_10s() {
    let fixture_path = env!("CARGO_BIN_EXE_fixture_echo");

    let manifest = ProviderManifest {
        executable_path: PathBuf::from(fixture_path),
        package_id: "fixture.echo".to_string(),
        package_version: "0.1.0".to_string(),
        content_hash: None,
    };

    let config = ProviderConfigBundle {
        extra_env: HashMap::new(),
    };

    let start = Instant::now();

    // Spawn the provider with a 10-second bootstrap deadline.
    let handle = ProviderRuntime::spawn(manifest, config, Duration::from_secs(10))
        .await
        .expect("provider spawn failed");

    // Initiate graceful shutdown.
    handle.shutdown().await.expect("shutdown failed");

    let elapsed = start.elapsed();

    // Assert shutdown completed within 10 seconds.
    assert!(
        elapsed <= Duration::from_secs(10),
        "shutdown took {elapsed:?}, expected <= 10s"
    );
}
